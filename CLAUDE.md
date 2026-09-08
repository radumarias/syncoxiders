# CLAUDE.md

This is the `syncoxiders` Cargo workspace. Per-crate guidance lives next to each crate.

- `p2p-transfer/` is the workspace's `default-members` crate. Read
  [`p2p-transfer/CLAUDE.md`](p2p-transfer/CLAUDE.md) before working in it; it
  covers the toolchain, build targets, commands, and architecture.
- Other members (`file-change-consumer`, `file-change-router`,
  `file-tree-merge`, `file-watcher`) are native-only tools with no crate-level
  guidance yet.
