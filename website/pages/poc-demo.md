[⟵ Back](https://github.com/radumarias/syncoxiders/blob/main/README.md#poc)

How it works for now:
- we mount in a folder with rclone
- we use git to catch changes, how it works:
- we keep a git repo for the folder in a `repo` folder
- inside in a `tree` folder we create the tree structure from `mnt`
- in the files we keep the size and hash of the content of the file
- we create files for directories also as git doesn't catch changes to directories
- we do `git add .`
- then `git status -s` show what's changed, we use `git2` crate to interact with git

Work in progress:
- have 2 mounted folders with rclone (src, dst)
- build changes tree for each of them
- merge changes trees and resolve conflicts
- apply changes to both src and dst
- make sure changes are reflected on both sremotes

Basic changes:  
[![Watch the video](https://img.youtube.com/vi/Z45mxYbojoc/0.jpg)](https://youtu.be/Z45mxYbojoc)

Rename:  
[![Watch the video](https://img.youtube.com/vi/Gdo7Igrg9QE/0.jpg)](https://youtu.be/Gdo7Igrg9QE)
