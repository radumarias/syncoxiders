[<< Back](../README.md)

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
