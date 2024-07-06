[⟵ Back](../features.md#features)

# Handles very large files efficiently with concurrent and resumable transfers for the browser app, local app and shared files

All transfers are made concurrently and are resumable. This is true for browser app, local app (if they crash or closed while transferring the next time they star the transfer will continue) and for shared files (browser transfers and with torrent clients).

When possible we use `BitTorrent` with `QUIC` or `Apache Arrow` format to sync files, this is true between local files for ex, but for provider files we use the supported provider protocols.
