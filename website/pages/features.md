[⟵ Back](../../README.md#features)

# Features

All sync operations and changes are applied with WAL (Write-Ahead Logging) to ensure file integrity on crash or power loss. That is, any changes are first written to WAL and then applied to the file. On crash or power loss, the next time the process starts it will apply remaining changes, this repeats until all changes are successfully applied.

MD5, SHA1 hashes are checked at all times for file integrity. After we transfer the files we will compare local hash with remote hash to ensure data integrity.

All transfers are multi-threaded and resumable. I can handles very large files efficiently with parallel and resumable downloads.

- [Sync between providers, Copy, Sync, Move, Two-way sync](features/sync-providers.md#sync-between-providers)
- [Conflict resolution](features/sync-providers.md#conflict-resolution)
- [Sync local files between multiple authenticated devices](features/sync-local.md)
- [MD5, SHA1 hashes are checked at all times for file integrity](features.md#features)
- Multi-threaded transfers
- [Make sync (and changes) with WAL (Write-Ahead Logging) to ensure file integrity](features.md#features)
- [Share between multiple providers like Google Drive, Dropbox, S3, ...](features/share-providers.md)
- [Share local files](features/share-local.md)
- [Easy way to share files with external users, via a link or notification](features/share-external.md)
- [Receiving files, like S3 presigned URLs but create a dest folder where others can upload more files and even folders](features/receive.md)
  - deadline, late uploads, password
- [Encrypted files and folders, with files saved in provider or with local files](features/encrypt.md)
  - also encrypted sharing with PGP
- [Browser app built in WebAssembly compatible with all major browsers](features/browser-app.md)
- [Local app for desktop and mobile, access files via FUSE on Linux and macOS, WinFSP (or others) on Windows and file picker on mobile](features/local-app.md)
  - encrypted cache, full copy kept in sync, notifications
- [Handles very large files efficiently with concurrent and resumable transfers](features/large-files.md)
- [Local views: My Drive, Computers, Shared, Shared with me, Recents, Starred, Trash](features/local-view.md)
- [Global view from all providers and local files, My Drive, Computers, Shared, Shared with me, Recents, Starred, Trash](features/global-view.md)
- [Internal links to other files](features/links.md)
- [Search capabilities](features/search.md)
- [Analytics](features/analytics.md)
- [File history and versioning](features/versioning.md)
- [Cleanup storage](features/cleanup.md)
- [Sync, share status, storage overview](features/status.md)
- [Backups with Borg, encrypted, deduplicated and compressed](features/backup.md)
- [Keep emails in sync between multiple providers](features/emails.md)
- [Backup your emails locally or on our Borg repo](features/emails.md)
- [Anti-Virus scanning](features/antivirus.md)
- [Automation, convert to PDF, convert image, unzip, convert audio/video, watermark files](features/automation.md)
- [Photos manager](features/photos.md)
- [REST API, gRPC, CLI clients and client libs in multiple languages](features/clients.md)
- [Many supported providers](features/supported-providers.md)
- [Integration with other systems](features/integrations.md)
