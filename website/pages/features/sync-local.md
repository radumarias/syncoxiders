# Sync local files

Useful to sync local files between multiple devices with no remote providers involved. It will do it in a P2P manner using QUIC (will fallback to TCP/IP if firewalls blocks UDP). Your local apps need to be running for the sync to happen. This is similar to Resilio Sync.

All devices are authenticated using web of trust to make sure are trusted by you. When you setup a sync an invite link or QR code is be generated. When you open the other app based on these it will send a handshake for the device to join the sync group and you need to confirm it from any of the other devices in the group. This will give him a cryptographic key synced between all devices and only then other devices will accept sync operations with it.

Sync mode (Copy, Move, One-way, Two-way), compare-mode and conflict resolution are handled as per above.
