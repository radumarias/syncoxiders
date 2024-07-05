# SyncOxiders

Cloud file and email Sync, file Sharing, Backup and Encryption solution written in Rust.

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/services2.png?raw=true)

The purpose of this project is to offer an easy and reliable way to sync files and emails between multiple providers and share files between multiple storage providers (like Google Drive, Dropbox, S3, SFTP servers, ...) and local files. Also simple way for backup of your files and emails and encryption. 
It offers real time sync (from simple Copy One-way to Two-way Sync) all handled in the cloud, without the explicit need of local clients.

> [!WARNING]  
> For now it's in prototyping phase, it has some or the core components, like encryption and basic Google Drive client.  
> You can see the design doc [here](https://www.canva.com/design/DAGI-5FeEEA/2IwzP0vp45dvSarZd_drzA/view?utm_content=DAGI-5FeEEA&utm_campaign=designshare&utm_medium=link&utm_source=editor) or [PDF](https://github.com/radumarias/syncoxiders/blob/0be3968005be6332214593046b6f54809aa13134/SyncOxiders.pdf)

Will use [rencfs](https://github.com/radumarias/rencfs) for encryption and [gdrive-rs](https://github.com/radumarias/gdrive-rs) for accesing Google Drive.

It you could take this [**SURVEY**](https://forms.gle/qgnWBJhzCpzPLSmv5) to express your opinion about the current solution and offer your opinion on what features you would want from a service like this it would help a lot.

> [!NOTE]  
In many cases we'll use present tense for several functionality, even though it's not yet implemented, it's used to give an idea of what the system could be.

[What's with the name?](website/pages/name.md)

# POC

Working on having these in up to 3 months:
- in Docker ability to sync any 2 folders in the filesystem
- run rclone in Docker and mount Google Drive and MS OneDrive or Dropbox in 2 folders
- from CLI trigger a sync which will make a Two-Way sync between the folders, first sync will do a union between the 2, no deletes will be performed
- make sure files are synced between the two folders and on the remote storage providers
- do some changes in both folders and trigger a sync, this and from now on will propagate deletes also, make sure folders are in sync and also on remotes
- do some changes on the remotes, trigger a sync and make sure changes are propagated in both folders and on remotes
- save files encrypted using rencfs and have sync working
  - this will save encrypted data on the mount points of rclone and expose them with FUSE

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/poc.png?raw=true)

# MVP

It would be possible to have something in about 6 months with this functionality:
- integration with Google Drive and Dropbox or MS OneDrive
- Sync between the two
- Share files in providers with another user
- browser app with basic functionality like:
  - adding providers
  - setup sync rules
  - share between providers
- some basic functionality of sharing local files, no sync between them
- Encryption

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/mvp.png?raw=true)

# Use cases
- You have various cloud providers and you need to keep files in sync between them
- You want to quickly share a local file with someone with minimal tools involved
- You want to share a Dropbox file with someone that doesn’t have a Dropbox account but want to keep that file in sync with them
- Someone want to send you a file but doesn’t have any cloud providers accounts
- Keep local files in sync between multiple devices, without any other storage providers, directly with P2P
- You want a global view of files among all providers, My Drive, shared, starred, recents and trash folders
- You want to access your remote providers files locally in the filesystem
- Encrypt local files or files stored on remote providers
- Search and analytics, what files you most often access, what types of files do you have, ...
- Quick and secure backups
- Keep emails in sync between multiple providers
- Backup your emails
- Photos manager

# Features
- Sync between providers, Copy, Sync, Move, Two-way sync
- Conflict resolution
- Sync local files between multiple authenticated devices
- MD5, SHA1 hashes are checked at all times for file integrity
- Multi-threaded transfers
- Make sync (and changes) with WAL (Write-Ahead Logging) to ensure file integrity
- Share between multiple providers like Google Drive, Dropbox, S3, ...
- Share local files
- Easy way to share files with external users, via a link or notification.
- Handles very large files efficiently with concurrent and resumable transfers
- Receiving files, like S3 presigned URLs but create a dest folder where others can upload more files and even folders
  - deadline, late uploads, password
- Encrypted files and folders, with files saved in provider or with local files
  - also encrypted sharing with PGP
- Browser app built in WebAssembly compatible with all major browsers
- Local app for desktop and mobile, access files via FUSE on Linux and macOS, WinFSP (or others) on Windows and file picker on mobile
  - encrypted cache, full copy kept in sync, notifications
- Local views: My Drive, Computers, Shared, Shared with me, Recents, Starred, Trash
- Global view from all providers and local files, My Drive, Computers, Shared, Shared with me, Recents, Starred, Trash
- Internal links to other files
- Search capabilities
- Analytics
- File history and versioning
- Cleanup storage
- Sync, share status, storage overview
- Backups with Borg, encrypted, deduplicated and compressed
- Keep emails in sync between multiple providers
- Backup your emails locally or on our Borg repo
- Anti-Virus scanning
- Automation, convert to PDF, convert image, unzip, convert audio/video, watermark files
- Photos manager
- REST API, gRPC, CLI clients and client libs in multiple languages
- Many supported providers

# What separates it from other products

What other solutions are there: MultCloud, S3 Drive, rclone, odrive, Resilio Sync.
Some of them are hard to use especially for non-technical users and also they lack multiple features.

- MultCloud seems to be very similar for Sync and Backup but lacks sync with local files. Share is only read only, the files are not kept in sync. Also it’s very buggy, login with Google doesn’t work when you access the shared link, reset pass doesn’t work well, their Google Drive app is still under review and for some reason their Android app is not in store but distributed as APK on their site. Also it lacks encryption
- S3 Drive is a bit harder to setup as it requires some manual config in some cases providing access tokens manually. Also lacks many features like Share, Backup, Encryption. The sync is also quite slow
- rclone is great but it’s mostly for geeks, you need to do some manual config and start it, mostly from the console. For Google Drive they have a problem, it is not quite real time Sync from remote to local. It has delay for auto-sync (even if you are listing the content of a folder it doesn’t immediately pickup changes) and does sync on some specific operations
- odrive doesn’t have sync between providers but just offers a unified view between all your providers. For sharing it offers the other person just a view of the shared files in odrive apps, they cannot add the files to their provider for accessing in there and keeping them in sync, or save them locally and keep them in sync
- Resilio Sync is great but is not of the scale I'm proposing, it only handles your local files sync, not between storage providers and also doesn't have such an easy sharing

What all are missing is an easy way to send a local file to someone with just the browser for example and Share between storage providers.

What we offer better or additionally:
- True real-time Sync and Share between storage providers and local files
- Simple and quick way to share files with someone using minimal tools, ideally only by browser
- Receiving files, like S3 presigned URLs but create a dest folder where others can upload more files and even folders
- Can combine both local files Sync and Share with files on storage providers
- Encryption for stored files
- Backup solution with borgbackup and repo for it
- Built very efficiently with Rust
- Search and analytics
- WAL (Write-Ahead Logging) to ensure file integrity
- Handles very large files efficiently with concurrent and resumable transfers
- Similar browser app and desktop app, also mobile apps
- Extensive cilents, libs, CLI app, REST API and gRPC service

# How it works
There are several ways to interact with our service:
- `browser app`: we expose an app built for WebAssembly than can manage all operations and transfers
- `local app`: we also have a local app which is very similar to the browser app, in fact they share the same code. The same, will handle all operations and transfers with the service and with other P2P apps. Will expose files with FUSE or other technologies
- `mobile app`: similar to local app but for Android and iOS
- `clients`:
  - `CLI app`: a command line interface to interact with our service. It can also expose files with FUSE or other technologies
  - `libs`: we have libs in several languages. They can also expose files with FUSE or other technologies
- `API`: we expose a REST API and gRPC service that you can use to manage all operations and transfers. It uses WebSockets to notify you about changes

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/clients.png?raw=true)

## Sync

### Files

#### Between providers

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-providers.png?raw=true)

#### Local files

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-local-files.png?raw=true)

#### Emails

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-emails.png?raw=true)

## Share files

### Between service users

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-providers.png?raw=true)

### With external users

- you can share a file from your providers with someone not using our service or any other storage providers. We’ll generate a link, we’ll email to them, or you can send them, and they can get it from browser, with a torrent client or sync it with our local client
- if they are using our browser or local app you will be notified when they changed something and they will be notified when you make changes

### Local files

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-local-file.png?raw=true)

### Receiving files into provider

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/receive-file-with-provider.png?raw=true)

### Receiving files locally

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/receive-local-file.png?raw=true)

You can mix between these, for example you can share a local file and the other person saves it on their provider, both files will be kept in sync.  
Or you can create a Request Files link based on a provider folder (or local folder) and others can send you files from their provider or local files using their browser, torrent client or local app.

## Encrypt

### Sync encrypted between providers

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-encrypted.png?raw=true)

### Share encrypted with users on our service

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-encryptyed-with-service-user.png?raw=true)

### Share encrypted with external user

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-encrypte-with-external-user.png?raw=true)

## Backup

### Files

### Backup files from provider

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-provider.png?raw=true)

#### Backup local files

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-local-files.png?raw=true)

When you want to restore some data you can use the local app, you’ll select the archive to restore from and it will be mounted in OS from where you can copy the files. You can also use borg CLI or Vorta for GUI if you want, setup will be provided for you in the local app, browser app and on our website.

### Emails

#### Backup by our service

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-emails-with-service.png?raw=true)

#### Backup locally

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-emails-locally.png?raw=true)
