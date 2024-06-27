# syncoxide.rs

Cloud files Sync, Sharing, Backup and encrypted solution written in Rust.

The purpose of this project is to offer an easy and reliable way to sync files between multiple providers (like Google Drive, Dropbox, S3, SFTP servers, ...) and local files, and a simple and quick way for file sharing, backup and encryption.  
It offers real time sync (from simple Copy One-way to Two-way Sync) all handled in the cloud, without the explicit need of local clients.

For now it's in prototyping phase, has some or the core components, like encryption and a basic Google Drive client.  
You can see the design doc [here](https://www.canva.com/design/DAGI-5FeEEA/2IwzP0vp45dvSarZd_drzA/view?utm_content=DAGI-5FeEEA&utm_campaign=designshare&utm_medium=link&utm_source=editor) or [PDF](https://github.com/radumarias/syncoxide-rs/blob/ff4a57650cc8f97ad382d3b9475ee60a9b95d089/syncoxide.rs.pdf)

Will use [rencfs](https://github.com/radumarias/rencfs) for encryption and [gdrive-rs](https://github.com/radumarias/gdrive-rs) for accesing Google Drive.

It you could take this survey to express your oppinion about the current solution and offer insights on what features you would like from a service like this it would help a lot  
https://forms.gle/qgnWBJhzCpzPLSmv5

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
- Backups with borg, encrypted, deduplicated, compressed
- Automation, convert to PDF, convert image, unzip, convert audio/video, watermark files
- Photos manager
- REST API, gRPC, CLI clients and client libs in multiple languages
- Many supported providers

# Key differences compared to other products

Compared to S3Drive, rclone, Resilio Sync, Nextcloud, Syncthing, ownCloud, Seafile

- Sync and Share between cloud providers
- Simple and quick way to share files with someone using minimal tools, ideally only by browser
- Receiving files, like S3 presigned URLs but create a dest folder where others can upload more files and even folders
- Can combine both local files Sync and Share with remote providers
- rclone is not quite real time Sync from remote to local, it has delay for auto-sync (even if you are listing the content of a folder it doesn’t immediately pickup changes) and does sync on some specific operations
- Encryption for stored files
- Backup solution with borgbackup and repo for it
- Built very efficient with Rust
- Search and analytics
- WAL (Write-Ahead Logging) to ensure file integrity
- Handles very large files efficiently with concurrent and resumable transfers
- Seamlessly browser app and desktop apps, also mobile apps
- Extensive cilents, libs, CLI, REST and gRPC

# How it works
There are several ways to interact with our service:
- browser app: we expose a file manager app built for WebAssembly than can manage all operations and transfers
- local app: we also have a local file manager app which is very similar to the browser app, in fact they share the same code. The same, will handle all operations and transfers with the service and with other P2P apps. Will expose files with FUSE or other technologies
- clients:
  - CLI: a command line interface to interact with our service. It can also expose files with FUSE or other technologies
  - libs: we have libs in several languages. They can also expose files with FUSE or other technologies
- API: we expose a REST API and gRPC that you can use to manage all operations and transfers. It uses WebSockets to notify you about changes

## Sync

### Between providers

- You setup your providers on our site, you login and grant access to all providers that you need
- Define sync rules, like just Copy from one provider to another or Two-way sync between them
- From this point on we’ll handle syncing all changes between providers (no local app needed for this, it’s all happening in the cloud)
- We also offer a cloud storage solution, you can setup our service as a provider and use a local app to sync the files. Then you can take full advantage of syncing those files between providers

### Local files
- In case you have some local files you want to keep in sync between multiple devices, without any file storage providers, you can use our local app on each device which will handle sync in P2P manner using QUIC. Apps need to be running for this. This is similar to Resilio Sync
- All devices are auhenticated using web of trust to make sure are trusted by you

You can mix these 2, for example you could setup a sync between a local folder and a provider. The local app will push changes to our service which will sync them with the provider and the service will push changes from provider back to local app.

## Share

### Between providers

- You may share a file from your provider with another person, using our service. They will see the file in their provider, both of you can change the file and the changes are kept in sync. You can also setup read-only sharing
- The service will notify the user. You will be notified back when they accepted and also when they changed something
- If provider supports sharing it will use their build-in sharing functionality, if not it will copy the file to destination provider and then keep it in sync between the two of you

### With external users

- You can share a file from your providers with someone not using our service or any other storage providers. We’ll generate a link, we’ll email to them, or you can send them, and they can get it from browser, with a torrent client or sync it with local our client
- If they are using our local app you will be notified when they changed something and they will be notified when you make changes

### Local files

- You can quickly share a file directly with someone via a link, they can get it from browser, with a torrent client or sync it with our browser or local app
- If both of you are using the local app you can share directly from the app and files will be kept in sync
- The share can be made in 2 ways:
  - P2P: in a peer-to-peer manner, your can use our file manager app in browser (uses WebRTC) or local app (uses HTTP, QUIC and BitTorrent), in both cases they need to be running for the user to download the files
  - Upload to our service: with the browser app or local app you can first upload the local file to us and user will download it directly from us. The browser app or local app don’t need to be running for the download to happen

### Receiving files

- Someone might want to send you a file, you setup a Request Files link, send it to them, and we’ll handle how the files gets back to you
- You can mix between these, for example you can share a local file and the other person who save it on their provider, both files will be kept in sync.
- Or you can create a Request Files link based on a provider folder (or local folder) and others can send files to you from their browser, torrent client, local app or from their provider.

## Encrypt
- You may want to keep your files encrypted for privacy. Your provider will not have access to the content of your files, nor metadata. You will need the local app or browser app to access the files. It works with local files too
- Encrypted share: it uses PGP to handle encryption. There are 2 options:
  - share with users on our service: a session key will be generated then encrypted with destination’s user’s public key. The encrypted key will be sent along with the file when sharing. If user is adding the share to providers or local app the file will be decryted on demand using the session key. On the disk or on provider it will be kept encrypted all the time
  - share with external user: if user is not using our service or downloads the file with browser or torrent, before downloading the file they will need to upload their public key. After that the file is downloaded as encrypted along with the encrypted session key. User can then decrypt the session key with his private key and then decrypt the file, this can be handled with any PGP client like GPG (instructions will be provided on the download page).

## Backup

- We’re using borg which handles encryption, deduplication and compression. We offer borg repos that can be used to backup data. Subscription is separate from the Sync and Share services.

There are 2 sources of backups:
- local files: using the local app you can setup backups schedule for local files and the local app will handle the backup process
- provider files: from browser app or local app you can schedule backups from provider files. The service will need to read all files and changes from the provider and will backup on our repo. Everything is handled by the service, you don’t need the local app running for this.

When you want to restore some data you can use the local app, you’ll select the archive to restore from and it will be mounted in OS from where you can copy the files. You can also use borg CLI or Vorta for GUI if you want, setup will be provided for you in the local app, browser app and on our website.
