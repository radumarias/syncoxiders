# SyncOxiders

Cloud file and email Sync, file Sharing, Backup and Encryption solution written in Rust.

The purpose of this project is to offer an easy and reliable way to sync files and emails between multiple providers and share files between multiple storage providers (like Google Drive, Dropbox, S3, SFTP servers, ...) and local files. Also simple way for backup of your files and emails and encryption. 
It offers real time sync (from simple Copy One-way to Two-way Sync) all handled in the cloud, without the explicit need of local clients.

> [!WARNING]  
> For now it's in prototyping phase, it has some or the core components, like encryption and basic Google Drive client.  
> You can see the design doc [here](https://www.canva.com/design/DAGI-5FeEEA/2IwzP0vp45dvSarZd_drzA/view?utm_content=DAGI-5FeEEA&utm_campaign=designshare&utm_medium=link&utm_source=editor) or [PDF](https://github.com/radumarias/syncoxiders/blob/0be3968005be6332214593046b6f54809aa13134/SyncOxiders.pdf)

Will use [rencfs](https://github.com/radumarias/rencfs) for encryption and [gdrive-rs](https://github.com/radumarias/gdrive-rs) for accesing Google Drive.

It you could take this [**SURVEY**](https://forms.gle/qgnWBJhzCpzPLSmv5) to express your opinion about the current solution and offer your opinion on what features you would want from a service like this it would help a lot.

> [!NOTE]  
In many cases we'll use present tense for several functionality, even though it's not yet implemented, it's used to give an idea of what the system could be.

[What's with the name](website/pages/name.md)

# POC

Working on having these in up to 2 months:
- in Docker ability to sync 2 folders in the filesystem
- run rclone in Docker and mount Google Drive and Dropbox or MS OneDrive in 2 folders
- from CLI trigger a sync which will make a Two-Way sync between the folders, first sync will do a union between the 2, no deletes will be performed
- make sure files are synced between the two folders and on the remote storage providers
- do some changes in both folders and trigger a sync, this and from now on will propagate deletes also, make sure folders are in sync and also on remotes
- do some changes on the remotes, trigger a sync and make sure changes are propagated in both folders and on remotes
- save files encrypted using rencfs and have sync working
  - this will save encrypted data on the mount points of rclone and expose them with FUSE

You can some [demos](website/pages/poc-demo.md) so far.

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/poc.png?raw=true)

# MVP

It would be possible to have something in about 6 months with this functionality:
- integration with Google Drive and Dropbox or MS OneDrive
- Sync between the two
- Share files from providers with another user
- browser app with basic functionality like:
  - adding providers
  - setup sync rules
  - share between providers
- some basic functionality of sharing local files, no sync between them
- Encryption

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/mvp.png?raw=true)

# The big picture

This is what it's planned to have in the end.

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/services.png?raw=true)

[Use cases]("website/pages/use-cases.md)
[Features]("website/pages/features.md)
[What separates it from other products]("website/pages/compare.md)
[How it works]("website/pages/how-it-worsks.md)
[How it works]("website/pages/stack.md)
