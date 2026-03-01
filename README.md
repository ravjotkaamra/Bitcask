Cloned the basic template and tests from pingcap https://github.com/pingcap/talent-plan/tree/master/courses/rust/projects/project-2

The key value store is built on top of the requirements mentioned by pingcap.

For both data persistence and crash recovery, a write ahead log (wal) is used. When the KvStore is opened, the log file is opened and replayed to build an in-memory index (key: String, value: Log Pointer).

In-memory index is chosen like that to reduce memory footprint. It is technically not that fast, since for every get call you'll need to read the WAL at the offset, and then perform Json deserialization.

It is also quite interesting that WAL file is both used for crash recovery and for data persistence. Usually both are decoupled and WAL file is just used for recovery. This makes the design super simple with less memory and disk usage, but then the performance is affected.

For log compaction, the algorithm iterates over all the values of the hashmap in increasing order (after sorting the values vector). Doing this means that, I will be reading the file sequentially (sort of) when I fetch the "useful" lines from the WAL. I create a temporary file, and write that file line by line, optimised using bufwriter. Even if it crashes, it's fine, I'll just use the old WAL file. After the compaction process is done, and the file is ready, I just rename it back to WAL.log. It deletes the old file, and replaces it with a new compacted log file. 

It is single threaded, so everything is sequential. But if there was parallelism, then the compaction should've happened in a background thread, and the moment it is done, the file handle should point to the new file. Also, I think we should probably keep the log files(two or more(?) maybe), always there. Instead of deleting them, we should focus on picking a current file and just overwrite the contents.
