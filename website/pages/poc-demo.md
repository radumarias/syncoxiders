[< Back](../../README.md#poc)

# How it works for now

`One-way` `One-to-Many` sync for `Add`, `Modify`, `Delete`, `Rename` operations. You can see here a short video demo:  
[![Watch the video](https://img.youtube.com/vi/JHQC1XpCzQw/0.jpg)](https://www.youtube.com/watch?v=JHQC1XpCzQw)

We'll exemplify for 2 paths, from `path1` to `path2` but it works with multiple paths, it will Sync from `path1` to
others.

**`One-way` sync:**

- have 2 mounted folders with rclone (`path1`, `path2`)
- build changes tree for `path1`
- apply changes from `path1` to `path2` for these operations:
    - `Add`, `Modify`, `Delete`, `Rename`

We use `git` to catch the changes, how it works:

- we have a special directory `repo` shared for both endpoints. This will be used to create a git repo for each path and
  tracks changes. It should persist between runs
- inside the `repo` for each path we create a `tree` directory and create the tree structure from `path` in there
- in the files content we keep `size` and `mtime` or `MD5 hash` if enabled
- we do `git add .`, then `git status -s` shows what's changed, we use `git2` crate to interact with git
- after we have the changes tree we apply them to `path2`
    - on `Add` and `Modify` we check if the file is already present in `path2` and if it's the same as in `path1` we
      skip it
    - comparison between the files is made using `size`, `mtime` or `MD5 hash`, if enabled
    - on `Rename` if the `old` file is not present in the `path2` to move it, we copy it from `path1`
- changes are applied wth WAL logic, we use git changes as WAL
    - after we build the changes tree we unstage all changes
    - after we applied a change for a file we stage that file in git
    - after each `64MB` we write we are checkpointing (we do `git commit`)
    - after applying all changes we commit remainig staged ones
    - then we delete the history and keep just an index of all files so e can catch new changes
    - like this if the process is suddenly interrupted the next time it runs it will see there are changes and will
      apply them
        - this hapens until all pending changes are applied

# Using CLI

**For now you need to have the `git` client installed locally.**

You can find the binaries [here](https://github.com/radumarias/syncoxiders/actions/workflows/ci.yml).
Select the last successful run and at the bottom you should have binaries for multiple OSs.

For other targets you could clone the repo and build it, see below how.

You can run `syncoxiders -h` to see all args. The basic usage is like this:

```bash
syncoxiders --repo <REPO> <PATH1> <PATH2>
```

- `inputs` (`<PATH1> <PATH2>`): a lists of paths that will be synced
- `<REPO>`: a directory that should persist between runs, we create a `git` repo with metadata of files from all paths.
  **MUST NOT BE IN ANY OF THE PATHS**. If it doesn't persist, next time it runs it will see all files as `Add`ed, but
  will skip copying them if already the same as on the other side

For now, it does `One-way` sync propagating the changes from `path1` to other paths:

- `Add`, `Modify`, `Delete`, `Rename`
- on `Add` and `Modify` we check if the file is already present in `path2` and if it's the same as in `path1` we skip it
- comparison between the files is made using `size` and `mtime` or `MD5 hash` (if enabled, see `--checksum` param below)
- on `Rename` if the `old` file is not present in the `path2` to move it, we copy it from `path1`

By default it detects changes in files based on `size` and `mtime`. After copying to `path2` it will set `atime`
and `mtime` for the files.

Other args:

- `--checksum`: (disabled by default): if specified it will calculate `MD5 hash` for files when detecting changes and
  when comparing file in `path1` with the file in `path2` when applying `Add` and `Modify` operations.  
  **This is especially useful if any pf the paths is accessed over the network and doesn't support `mtime` or
  even `size` or if the clocks are out of sync**
  **It will be considerably slower when enabled**
- `--no-crc`: (disabled by default): if specified it will skip `CRC` check after file was transferred. Without this it
  compares the `CRC` of the file in `path1` with the `CRC` of the file in `path2` after transferred. This ensures the
  file integrity after transfered.
  **Checking `CRC` is highly recommend if any of the paths is accessed over the network.**
- `--dry-run`: this simulates the sync. Will not apply any changes to the paths, will just print the operations that
  would have normally be applied to paths
- `--log-all-changes`: by default it doesn't log each change that is applied, but every 100th change so it won't clutter
  the logs. Setting this will log all changes

## Limitations

- conflicts are not handled yet. If the file is changed in both `path1` and `path2` the winner is the one from `path1`.
  It's like `master-slave` sync where `path1` is the master
- it doesn't sync any of `Add`, `Delete`, or `Rename` operations on empty folders. This is actually a limitation
  of `git` as it works only on files. The directory tree will be recreated based on the file parent, but folders with no
  files in them will not be synced

## Troubleshooting

In case you experience any inconsistencies in the way the files are synced, or not synced, you can delete the `repo`
directory and run it again. It will see all files as new but will not copy them to the oher sides if already present and
the same, it will just copy the new or changed ones.

## Compile it from source code

### Windows

You need to have `gcc` installed locally.

### Clone the repo

```bash
git clone git@github.com:radumarias/syncoxiders.git
cd syncoxiders
```

### Install rust

[Install Rust](https://www.rust-lang.org/tools/install)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### Configuring the PATH environment variable

In the Rust development environment, all tools are installed to the `~/.cargo/bin` directory, and this is where you will
find the Rust toolchain, including rustc, cargo, and rustup.

Accordingly, it is customary for Rust developers to include this directory in their `PATH` environment variable. During
installation rustup will attempt to configure the `PATH`. Because of differences between platforms, command shells, and
bugs in rustup, the modifications to PATH may not take effect until the console is restarted, or the user is logged out,
or it may not succeed at all.

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
target/release/syncoxiders --repo <REPO> <PATH1> <PATH2>
```

# Work in progress

- resolve conflicts
- `One-to-Many` sync optimization (read one time from `path1` when applying changes to multiple paths)
- apply changes by chunks in parallel
- apply changes to both `path1` and `path2`, `Two-Way` sync
- apply changes between multiple paths, `N-Way` sync
