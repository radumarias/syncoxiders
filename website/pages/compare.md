[⟵ Back](../../README.md#what-separates-it-from-other-products)

# What separates it from others

## What other solutions are there

MultCloud, S3 Drive, rclone, odrive, Resilio Sync.  
Some of them are hard to use especially for non-technical users and also they lack multiple features.

- `MultCloud` seems to be very similar for Sync and Backup but lacks sync with local files. Share is only read only, the files are not kept in sync. Also it’s very buggy, login with Google doesn’t work when you access the shared link, reset pass doesn’t work well, their Google Drive app is still under review and for some reason their Android app is not in store but distributed as APK on their site. Also it lacks encryption
- `S3 Drive` is a bit harder to setup as it requires some manual config in some cases providing access tokens manually. Also lacks many features like Share, Backup, Encryption. The sync is also quite slow
- `rclone` is great but it’s mostly for geeks, you need to do some manual config and start it, mostly from the console. For Google Drive they have a problem, it is not quite real time Sync from remote to local. It has delay for auto-sync (even if you are listing the content of a folder it doesn’t immediately pickup changes) and does sync on some specific operations
- `odrive` doesn’t have sync between providers but just offers a unified view between all your providers. For sharing it offers the other person just a view of the shared files in odrive apps, they cannot add the files to their provider for accessing in there and keeping them in sync, or save them locally and keep them in sync
- `Resilio Sync` is also great but is not of the scale I'm proposing, it only handles your local files sync, not between storage providers and also doesn't have such an easy sharing

**What all are missing is:**
- **An easy way to send a file to someone with just the browser**
- **Share between storage providers**

## What we offer better or additionally

**Main 2 ones are:**
- **True real-time `Sync` and especially `Share` between storage providers (but also for local files)**
- **Simple and quick way to `Share` files with someone using minimal tools, ideally only a browser**

Additionally:
- Receiving files, like S3 presigned URLs but it creates a dest folder where others can upload more files and even folders
- Can combine both local files for Sync and Share and files on storage providers
- Encryption for stored files
- Backup solution with borgbackup and repo
- Performant, efficient, and secure (implemented in RUST and other modern technologies)
- Search and analytics
- WAL (Write-Ahead Logging) to ensure file integrity
- Handles very large files efficiently with concurrent and resumable transfers
- Unified browser, desktop and mobile apps
- Extensive cilents and access, libs, CLI app, REST API and gRPC service
