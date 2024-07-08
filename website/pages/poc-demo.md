[⟵ Back](../../README.md#poc)

# How it works for now

**`One-way` sync:**
- have 2 mounted folders with rclone (`src`, `dst`)
- build changes tree for `src`
- apply changes from `src` to `dst` for these operations:
    - `Add`, `Modify`, `Delete`, `Rename`
- make sure changes are reflected in `dst` remote

**We use git to catch changes, how it works:**
- we keep a git repo for the folder in a `repo` folder
- we have 2 special directories for src and dst
    - `mnt`: where the actual files are
    - `repo`: the git repo that must persist between runs
- inside the `repo` we create a `tree` directory and create the tree structure from `mnt`
- in the files content we keep the `size`, `mtime` and a `MD5 hash` of the content of the file, if we use `--checksum` param on run, see below. Please not it's considerably slower with this enabled
- we do `git add .`
- then `git status -s` shows what's changed, we use `git2` crate to interact with git
- after we have the changes tree we apply them to `dst` `mnt`
    - on `Add` and `Modify` we check if the file is already present in `dst` and if it's the same as in `src` we skip it
    - comparation between the files is made using `size`, `mtime` and `MD5 hash`, if enabled
    - on `Rename` if the `old` file is not present in the `dst` to move it, we copy from `src`

# Using CLI

You can take the binaries from here for target [x86_64-unknown-linux-gnu](https://drive.google.com/file/d/1UnWR5rnPfOW3OBLu21xJySPDVHkEbb-v/view?usp=sharing).  
For other targets you could clone the repo and build it.

You can run `syncoxiders -h` to see all args. The basic usage is like this:

```bash
syncoxiders --src-mnt <SRC-MNT> --src-repo <SRC-REPO> --dst-mnt <DST-MNT> --src-repo <DST-REPO>
```

`<SRC-MNT>`: where the actual files are for `src` side
`<SRC-REPO>`: a folder that should persist between runs, it creates a `git` repo with metadata from files from `src`
`<DST-MNT>`: where the actual files are for `dst` side
`<DST-REPO>`: a folder that should persist between runs, it creates a `git` repo with metadata from files from `dst`

For now, it does `One-way` sync propagating these operations from `src` to `dst`:
- `Add`, `Modify`, `Delete`, `Rename`
- on `Add` and `Modify` we check if the file is already present in `dst` and if it's the same as in `src` we skip it
- comparation between the files is made using `size`, `mtime` and `MD5 hash`, if enabled, see `--checksum` arg below
- on `Rename` if the `old` file is not present in the `dst` to move it, we copy from `src`


By default it detects changes in files based on `size` and `mtime`. After copying to `dst` it will set also `atime` and `mtime` for the files.

Other args:
- `--dry-run`: it will not youch any files in `<SRC-MNT>`, it will just print the operations
- `--checksum`: (disabled by default): If specified it will calculate `MD5` for each file and keep it in the git repo files. It will participate in detecting changes along with `size` and `mtime`. **Please note, it will be much slower when activated.**
- `--no-crc`: (disabled by default): If specified it will skip checking `CRC` after file was transfered. Normally it compare `CRC` of file in `src` before coping and the file in `dst` after copying, this ensures the transfer was ok. **Checking `CRC` is mostly useful if disk is accessed over the network.`

# Work in progress

- have 2 mounted folders with rclone (`src`, `dst`)
- build changes tree for both of them
- merge changes trees and resolve conflicts
- apply changes to both `src` and `dst`
- make sure changes are reflected on both sremotes

Basic changes:  
[![Watch the video](https://img.youtube.com/vi/Z45mxYbojoc/0.jpg)](https://youtu.be/Z45mxYbojoc)

Rename:  
[![Watch the video](https://img.youtube.com/vi/Gdo7Igrg9QE/0.jpg)](https://youtu.be/Gdo7Igrg9QE)
