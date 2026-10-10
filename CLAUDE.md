# CLAUDE.md

This is the `syncoxiders` Cargo workspace. Per-crate guidance lives next to each crate.

- `p2p-transfer/` is the workspace's `default-members` crate. Read
  [`p2p-transfer/CLAUDE.md`](p2p-transfer/CLAUDE.md) before working in it; it
  covers the toolchain, build targets, commands, and architecture. Cloudflare
  hosting for that crate uses the `cf` CLI (`cf deploy --prebuilt`), not Wrangler.
- `.oss-scanner/` enrolls `p2p-transfer` (only) in Anthropic's OSS Scanner: a
  `Dockerfile` the scanner builds from the repository root, and a
  `threat_model.md`. Keep its trunk and wasm-pack versions in step with
  `p2p-transfer/build-web.sh` and CI, and its build and test steps with
  `p2p-transfer/check.sh`.
- Other members (`file-change-consumer`, `file-change-router`,
  `file-tree-merge`, `file-watcher`) are native-only tools with no crate-level
  guidance yet.
