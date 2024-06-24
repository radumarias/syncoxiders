# syncoxide-rs

A cloud file sync, sharing and backup solution written in Rust.

The purpose of this project is to offer an easy and reliable way to sync files between multiple providers (like Google Drive, Dropbox, S3, SFT servers, ...) and local files, and a simple and quick way for file sharing and backups.  
It offers real time sync (from simple Copy One-way to Two-way Sync) all handled in the cloud, without the explicit need of local clients.

For now it's just in idea phase. You can see the design doc [here](https://www.canva.com/design/DAGI-5FeEEA/2IwzP0vp45dvSarZd_drzA/view?utm_content=DAGI-5FeEEA&utm_campaign=designshare&utm_medium=link&utm_source=editor) or [PDF](https://github.com/radumarias/syncoxide-rs/blob/ff4a57650cc8f97ad382d3b9475ee60a9b95d089/syncoxide.rs.pdf)

Will use [rencfs](https://github.com/radumarias/rencfs) for encryption and in prototype [gdrive-rs](https://github.com/radumarias/gdrive-rs) for accsing Google Drive.

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
- Sync between providers, Copy, Sync, Move, Tho-way sync
- Conflict resolution
- Sync local files between multiple devices
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
- Search and analytics
- Cleanup storage
- Sync, share status, storage overview
- Backups with borg, encrypted, deduplicated, compressed
- Photos manager
- REST API, gRPC, CLI clients and client libs in multiple languages
- Many supported providers
