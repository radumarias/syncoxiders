[⟵ Back](../../README.md#poc)

# How it works for now

`One-way` sync for `Add`, `Modify`, `Delete`, `Rename` operations. You can see here a short demo:  
[![Watch the video](https://img.youtube.com/vi/JHQC1XpCzQw/0.jpg)](https://www.youtube.com/watch?v=JHQC1XpCzQw)

**`One-way` sync:**
- have 2 mounted folders with rclone (`path1`, `path2`)
- build changes tree for `path1`
- apply changes from `path1` to `path2` for these operations:
    - `Add`, `Modify`, `Delete`, `Rename`

**We use `git` to catch the changes, how it works:**
- we have 2 special directories for `path1` and `path2`
    - `mnt`: where the actual files that needs to be sync are
    - `repo`: a git repo we create that should persist between runs
- inside the `repo` we create a `tree` directory and create the tree structure from `mnt` in there
- in the files content we keep `size` and `mtime`
- we do `git add .`, then `git status -s` shows what's changed, we use `git2` crate to interact with git
- after we have the changes tree we apply them to `path2` `mnt`
    - on `Add` and `Modify` we check if the file is already present in `path2` and if it's the same content as in `path1` we skip it
    - comparison between the files is made using `size`, `mtime` and `MD5 hash`, if enabled
    - on `Rename` if the `old` file is not present in the `path2` to move it, we copy it from `path1`
- changes are applied wth WAL logic, we use git changes as WAL
     - after we build the changes tree we unstage all changes
     - after we applied a change for a file we stage that file in git
     - periodically we commit staged ones
     - after applying all changes we commit remainig staged ones
     - like this if process is suddenly interrupted the next time it runs it will see as chnaged only not applied changes and will apply them
     - this hapens until all pending changes are applied

# Using CLI

You can take the binary from here for target [x86_64-unknown-linux-gnu](https://drive.google.com/file/d/1UnWR5rnPfOW3OBLu21xJySPDVHkEbb-v/view?usp=sharing).  
For other targets you could clone the repo and build it.

You can run `syncoxiders -h` to see all args. The basic usage is like this:

```bash
syncoxiders --path1-mnt <PATH1-MNT> --path2-mnt <PATH2-MNT> --path-repo <PATH-REPO>
```

- `<PATH1-MNT>`: where the actual files are for `path1` side
- `<PATH2-MNT>`: where the actual files are for `path2` side
- `<PATH-REPO>`: a folder that should persist between runs, we create a `git` repo with metadata of files from `path1` and `path2`. **MUST NOT BE INSIDE ANY OF `<PATH1-MNT>` or `<PATH1-MNT>` DIRECTORIES**. If it doesn't persist next time it runs it will see all files in as `Add`ed, but will skip them if are already the same as on the other side

For now, it does `One-way` sync propagating these operations from `path1` to `path2`:
- `Add`, `Modify`, `Delete`, `Rename`
- on `Add` and `Modify` we check if the file is already present in `path2` and if it's the same as in `path1` we skip it
- comparison between the files is made using `size`, `mtime` and `MD5 hash`, if enabled, see `--checksum` arg below
- on `Rename` if the `old` file is not present in the `path2` to move it, we copy it from `path1`

By default it detects changes in files based on `size` and `mtime`. After copying to `path2` it will set also `atime` and `mtime` for the files.

Other args:
- `--dry-run`: this simulates the sync. Will not actually create or change any of the files in `path1` `mnt` or `path2` `mnt`, will just print the operations that would have normally be applied to both ends
- `--checksum`: (disabled by default): if specified it will calculate `MD5 hash` for files when comparing file in `path1` with the file in `path2` when applying `Add` and `Modify` operations. **It will be considerably slower when activated**
- `--no-crc`: (disabled by default): if specified it will skip `CRC` check after file was transferred. Without this it compares the `CRC` of the file in `path1` before transfer with the `CRC` of the file in `path1` after transferred. This ensures the transfer was successful. **Checking `CRC` is highly recommend if any of `path1` or `path2` are accessed over the network.**

## First sync

For a more robust sync, if this is the first time you sync the 2 directories and `<PATH2-MNT>` is not empty, that is if both `<PATH1-MNT>` and `<PATH2-MNT>` have some files but different ones (they are not in sync) you should run the command with `--checksum` first time to compare also the `MD5 hash` when checking for changed files in `<PATH1-MNT>` compared to `<PATH2-MNT>`. This will result in a union from both `<PATH1-MNT>` and `<PATH2-MNT>`, no deletes will be made this first time.  
Similarly you should do if you delete the `<REPO-MNT>` dirctory.  
After that you can run without the flag if you don't want to use the `MD5 hash` to determine changes.

## Limitations

- Conflicts are not handled yet. If the file is changed in both `path1` and `path2` the winner is the one from `path1`. It's like `master-slave` sync where `path1` is the master
- For now it doesn't sync any of `Add`, `Delete`, or `Rename` operations on empty folders. This is actually a limitation of `git` as it works only on files. The directory tree will be recreated in `path2` based on the file parent, but folders with no files in them will not be synced

## Troubleshooting

In case you experience any inconsistencies in the way the files are synced, or not synced for that matter, you can delete the `<PATH-REPO>` directory and run it again. It will see all files as new but will not copy them to the oher side if hey are already present in here and with the same content, it wil just copy the new or changed ones.  
For a more robust first time sync after you removed the `<PATH-REPO>` directory you should run the command with `--checksum` first time to compare also the `MD5 hash` when checking for changed files in `<PATH1-MNT>` compared to `<PATH2-MNT>`. This will result in a union from both `<PATH1-MNT>` and `<PATH2-MNT>`, no deletes will be made this first time.  
After that you can run without the flag if you don't want to use the `MD5 hash` to determine changes.

## Logs

It doesn't print each change in logs, but just one in 100 changes, so it won't clutter the logs.

## Compile it from source code

### Clone the repo

```bash
git clone git@github.com:radumarias/syncoxiders.git
```

### Install rust

[Install Rust](https://www.rust-lang.org/tools/install)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### Configuring the PATH environment variable

In the Rust development environment, all tools are installed to the `~/.cargo/bin` directory, and this is where you will find the Rust toolchain, including rustc, cargo, and rustup.

Accordingly, it is customary for Rust developers to include this directory in their `PATH` environment variable. During installation rustup will attempt to configure the `PATH`. Because of differences between platforms, command shells, and bugs in rustup, the modifications to PATH may not take effect until the console is restarted, or the user is logged out, or it may not succeed at all.

If, after installation, running `rustc --version` in the console fails, this is the most likely reason.

You can try this also:

```bash
$HOME/.cargo/env
```

### Compile the code

```bash
cargo build --release
```
### Run it

```bash
target/release/syncoxiders --path1-mnt <PATH1-MNT> --path2-mnt <PATH2-MNT> --path-repo <PATH-REPO>
```

# Work in progress

- merge changes trees between `path1` and `path2` and resolve conflicts
- apply changes to both `path1` and `path2`
