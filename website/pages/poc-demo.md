[⟵ Back](../../README.md#poc)

How it works for now:

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
    - in `Add/Modify` we check if the file in `dst` is the same as in `src` we skip it
    - comparation between the files is made using `size`, `atime`, `mtime` and `hash`, if present
    - on `Rename` if the `old` file is not present in the `dst` to move it, we copy from `src`

**Work in progress:**
- have 2 mounted folders with rclone (`src`, `dst`)
- build changes tree for both of them
- merge changes trees and resolve conflicts
- apply changes to both `src` and `dst`
- make sure changes are reflected on both sremotes

Basic changes:  
[![Watch the video](https://img.youtube.com/vi/Z45mxYbojoc/0.jpg)](https://youtu.be/Z45mxYbojoc)

Rename:  
[![Watch the video](https://img.youtube.com/vi/Gdo7Igrg9QE/0.jpg)](https://youtu.be/Gdo7Igrg9QE)
