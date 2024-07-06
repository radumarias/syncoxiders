[⟵ Back](../features.md#features)

# Share Local files

You can quickly share a file directly with someone via a link, they can get it from browser, with a torrent client or sync it with our apps.

If both of you are using any of our apps you can share it directly from the app and files will be kept in sync.

The file transfer can be handled in 2 ways:
- **P2P**: in a `peer-to-peer` manner, your can use our file manager app in browser (uses `WebRTC`) or local app (uses `HTTP`, `QUIC` and `BitTorrent`). In both cases the apps need to be running for the user to download the files
- **Upload to our service**: with the browser app or local app you can first upload the local files to us and user will download them directly from us. The browser app or local app doesn't need to be running for download to happen

Sync mode (`Copy`, `Move`, `One-way`, `Two-way`), `compare-mode` and `conflict resolution` are handled as per above.

You can define these options when sharing:
- password
- expiry date

You always have the option to stop sharing.

You will be notified when they accepted or change something and they will be notified when you make changes.

When they access the link they have the same options as above for sharing between other service users or with external users.

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/diagram-share-local.png?raw=true)
