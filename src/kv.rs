#![allow(missing_docs)]

use crate::{KvsError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::PathBuf;

const THRESHOLD: u64 = 256;

#[derive(Serialize, Deserialize)]
enum Command {
    Set { key: String, value: String },
    Remove { key: String },
}

/// KvStore
pub struct KvStore {
    kv: HashMap<String, u64>,
    wal_file: File,
    cur_ofst: u64,
    dir_path: PathBuf,
    wasted: u64,
}

impl KvStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        // Create a mutable instance of KvStore
        let mut kv: HashMap<String, u64> = HashMap::new();

        // Open the WAL log file
        let dir_path = path.into();
        let wal_path = dir_path.clone().join("wal.log");
        let wal_file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(wal_path)
            .map_err(|_| KvsError::UnknownError)?;

        // Iterate over each line of WAL
        let mut cur_ofst: u64 = 0;
        let reader = BufReader::new(&wal_file);
        for line in reader.lines() {
            let line = line.map_err(|error| KvsError::IoError { error })?;

            // Convert the JSON string back to a Command.
            let command: Command =
                serde_json::from_str(&line).map_err(|e| KvsError::DeserializedError {
                    line: format!("Open Failed at cur_ofst {} for string {}", cur_ofst, line),
                    error: e,
                })?;
            match command {
                Command::Set { key, value: _ } => kv.insert(key, cur_ofst),
                Command::Remove { key } => kv.remove(&key),
            };

            cur_ofst += line.len() as u64 + 1;
        }

        Ok(Self {
            kv,
            wal_file,
            cur_ofst,
            dir_path,
            wasted: 0,
        })
    }

    pub fn set(&mut self, key: String, value: String) -> Result<()> {
        // pub fn to_writer<W, T>(writer: W, value: &T) -> Result<()>
        let command = Command::Set {
            key: key.clone(),
            value,
        };
        // If the key is being overwritten, then increase the wasted count
        if self.kv.insert(key, self.cur_ofst).is_some() {
            self.wasted += 1;
        };
        self.write_wal(command)?;

        // Assuming each line is of 256Bytes, so overall
        // if data wasted is 64KiB, perform compaction
        // Ideally, size of each line should be stored in hashmap
        if self.wasted > THRESHOLD {
            return self.compaction();
        };
        Ok(())
    }

    pub fn get(&mut self, key: String) -> Result<Option<String>> {
        // From in-memory index find the log pointer for the key
        let Some(offset) = self.kv.get(&key) else {
            return Ok(None);
        };

        // Read the whole line at the offset
        let line = self.read_line(*offset)?;

        // Deserialize the line from json to get the value
        let command: Command =
            serde_json::from_str(&line).map_err(|e| KvsError::DeserializedError {
                line: line.to_owned(),
                error: e,
            })?;

        match command {
            // Could create a lot of new error scenarios here, but I will skip.
            Command::Set { key: _, value } => Ok(Some(value)),
            Command::Remove { key: _ } => Err(KvsError::KeyNotFound),
        }
    }

    pub fn remove(&mut self, key: String) -> Result<()> {
        let command = Command::Remove { key: key.clone() };
        self.write_wal(command)?;
        match self.kv.remove(&key) {
            Some(_) => {
                self.wasted += 2; // "rm" as well as the "set" both are invalid now
                if self.wasted > THRESHOLD {
                    return self.compaction();
                }
                Ok(())
            }
            None => Err(KvsError::KeyNotFound),
        }
    }

    fn read_line(&mut self, offset: u64) -> Result<String> {
        // Seek to the offset
        self.wal_file
            .seek(SeekFrom::Start(offset))
            .map_err(|e| KvsError::IoError { error: e })?;

        // Wrap in BufReader for line reading
        let mut reader = BufReader::new(&mut self.wal_file);

        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| KvsError::IoError { error: e })?;
        
        if line.ends_with('\n') {
            line.pop();
        }
        Ok(line)
    }

    fn write_wal(&mut self, command: Command) -> Result<()> {
        // Seek to the end of WAL before writing
        self.wal_file
            .seek(SeekFrom::Start(self.cur_ofst))
            .map_err(|e| KvsError::IoError { error: e })?;

        // Serialize the command into a json string
        let cmd_str =
            serde_json::to_string(&command).map_err(|e| KvsError::SerializeError { error: e })?;

        // Write the Json String representing the command to WAL
        writeln!(&self.wal_file, "{}", cmd_str).map_err(|e| KvsError::IoError { error: e })?;

        self.cur_ofst += cmd_str.len() as u64 + 1;
        Ok(())
    }

    /// Compacts the bigger log into smaller one by removing overwritten keys
    /// and "remove" commands which are just tombstone markers.
    /// In this project, compaction happens synchronously, ideally it should be done
    /// in a parallel background thread.
    fn compaction(&mut self) -> Result<()> {
        // Create and open a new WAL log file
        let wal_path = self.dir_path.join("wal_temp.log");
        let mut wal_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&wal_path)
            .map_err(|_| KvsError::UnknownError)?;
        let mut cur_ofst: u64 = 0;

        // Wrap the file with a BufWriter for buffered writes
        let mut writer = BufWriter::new(&mut wal_file);

        // Collect all the log pointers into a Vec, and sort them
        let mut vals: Vec<u64> = self.kv.values().copied().collect();
        vals.sort_unstable();

        // Iterate over all the valid key-value pairs in the log file
        for ofst in vals {
            // Transfer the line to new compacted file
            let line = self.read_line(ofst)?;
            writeln!(&mut writer, "{}", line).map_err(|e| KvsError::IoError { error: e })?;

            // Update the in-memory index
            let command: Command =
                serde_json::from_str(&line).map_err(|e| KvsError::DeserializedError {
                    line: format!("reading from ofst {}, line is {}", ofst, line),
                    error: e,
                })?;
            if let Command::Set { key, value: _ } = command {
                self.kv.insert(key, cur_ofst);
            }
            cur_ofst += line.len() as u64 + 1;
        }

        // Flush the buffered writes to the underlying file
        writer.flush().map_err(|e| KvsError::IoError { error: e })?;
        drop(writer);

        // Drop the old data in KvStore, and point it to new
         std::fs::rename(wal_path, self.dir_path.join("wal.log"))
            .map_err(|e| KvsError::IoError { error: e })?;
        self.wal_file = wal_file;
        self.cur_ofst = cur_ofst;
        self.wasted = 0;
       
        Ok(())
    }
}
