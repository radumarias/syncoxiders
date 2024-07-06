[⟵ Back](../features.md#features)

# Local app for desktop and mobile

We have local apps for all major desktop OSs and on mobile for Android and iOS.

The desktop app is very similar to browser app, in fact they share the same code. The same, will handle all operations and transfers with the service and with other P2P apps. Will expose files with FUSE on Linux and macOS, WinFSP (or others) on Windows and file picker on mobile. Concurrent and resumable transfers.

It operates in 2 modes:
- **encrypted cache (default)**: it downloads the files on demand in real-time and keep them in a local encrypted cache with a maximum size
- **full copy kept in sync**: it will download all files for each provider and keeps them in `Two-way` sync

It supports notifications that you receive for several events and changes, like when someone is sharing a file with you, someone accepted your share, someone changed a shared file and others.

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/app.png?raw=true)
