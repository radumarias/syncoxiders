[⟵ Back](../../README.md#poc)

# How it works for now

`One-way` sync for `Add`, `Modify`, `Delete`, `Rename` operations. You can see here a short demo:  
[![Watch the video](https://img.youtube.com/vi/JHQC1XpCzQw/0.jpg)](https://www.youtube.com/watch?v=JHQC1XpCzQw)

**`One-way` sync:**
- have 2 mounted folders with rclone (`path1`, `path1`)
- build changes tree for `path1`
- apply changes from `path1` to `path2` for these operations:
    - `Add`, `Modify`, `Delete`, `Rename`

**We use `git` to catch the changes, how it works:**
- we have 2 special directories for `path1` and `path1`
    - `mnt`: where the actual files that needs to be sync are
    - `repo`: a git repo we create that should persist between runs
- inside the `repo` we create a `tree` directory and create the tree structure from `mnt` in there
- in the files content we keep `size` and `mtime`
- we do `git add .`, then `git status -s` shows what's changed, we use `git2` crate to interact with git
- after we have the changes tree we apply them to `path2` `mnt`
    - on `Add` and `Modify` we check if the file is already present in `path2` and if it's the same content as in `path1` we skip it
    - comparison between the files is made using `size`, `mtime` and `MD5 hash`, if enabled
    - on `Rename` if the `old` file is not present in the `path2` to move it, we copy it from `path1`

# Using CLI

You can take the binary from here for target [x86_64-unknown-linux-gnu](https://drive.google.com/file/d/1UnWR5rnPfOW3OBLu21xJySPDVHkEbb-v/view?usp=sharing).  
For other targets you could clone the repo and build it.

You can run `syncoxiders -h` to see all args. The basic usage is like this:

```bash
syncoxiders --path1-mnt <PATH1-MNT> --path1-repo <PATH1-REPO> --path2-mnt <PATH2-MNT> --path2-repo <PATH2-REPO>
```

- `<PATH1-MNT>`: where the actual files are for `path1` side
- `<PATH1-REPO>`: a folder that should persist between runs, we create a `git` repo with metadata from files from `path1`. **MUST NOT BE INSIDE ANY OF THE `MNT` DIRECTORIES**. If it doesn't persist next time it runs it will see all files in `path1` as `Add`ed, but will skip them if are already the same as in `path1`
- `<PATH2-MNT>`: where the actual files are for `path2` side
- `<PATH2-REPO>`: a folder that should persist between runs, we create a `git` repo with metadata from files from `path2`. **MUST NOT BE INSIDE ANY OF THE `MNT` DIRECTORIES**. If it doesn't persist next time it runs it will see all files in `path2` as `Add`ed, but will skip them if are already the same as in `path2`

For now, it does `One-way` sync propagating these operations from `path1` to `path2`:
- `Add`, `Modify`, `Delete`, `Rename`
- on `Add` and `Modify` we check if the file is already present in `path2` and if it's the same as in `path1` we skip it
- comparison between the files is made using `size`, `mtime` and `MD5 hash`, if enabled, see `--checksum` arg below
- on `Rename` if the `old` file is not present in the `path2` to move it, we copy it from `path1`

By default it detects changes in files based on `size` and `mtime`. After copying to `path2` it will set also `atime` and `mtime` for the files.

Other args:
- `--dry-run`: it will not youch any files in `<PATH2-MNT>`, it will just print the operations
- `--checksum`: (disabled by default): if specified it will calculate `MD5 hash` for files when comparing file in `path1` with the file in `path2` when applying `Add` and `Modify` operation. **Please note, it will be considerably slower when activated**
- `--no-crc`: (disabled by default): if specified it will skip `CRC` check after file was transfered. Normally it compares the `CRC` of the file in `path1` before coping with the `CRC` of the file in `path1` after transferred. This ensures the transfer was successful. **Checking `CRC` is higly recommend if any of `path1` or `path2` are accessed over the network.**

## Limitations

- Conflicts are not handled yet. If the file is changed in both `path1` and `path2` the winner is the one from `path1`. It's like `master-slave` sync where `path1` is the master
- For now it doesn't sync any of `Add`, `Delete`, or `Rename` operations on empty folders. This is actually a limitation of `git` as it works only on files. The directory tree will be recreated in `path2` based on the file parent, but folders with no files in them will not be synced

# Work in progress

- merge changes trees between `path1` and `path2` and resolve conflicts
- apply changes to both `path1` and `path2`
