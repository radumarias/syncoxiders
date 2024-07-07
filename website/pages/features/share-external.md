[⟵ Back](../features.md#features)

# Share with external users

You can share a file from your providers with someone not using our service or any other storage providers. We’ll generate a link, we’ll email to them, or you can send them, and they can get it from browser, with a torrent client or sync it with local our client.

You can initiated the share in 2 ways:
- **from browser app**: using our file manager app in browser you will pick-up a file from a provider
- **from local app**: you will initiate the share from local app

Sync mode (`Copy`, `Move`, `One-way`, `Two-way`), `compare-mode` and `conflict resolution` are handled as per above.

You can define these options when sharing:
- password
- expiry date

You always have the option to stop sharing.

You will be notified when they accepted or change something and they will be notified when you make changes.

When they access the link will have these options:
- **download with browser**: download will be handled by their browser, it’s a `One-way` one time transfer
- **download with torrent**: they will use a torrent client to get the file. We will handle the torrent tracker. This is a `One-way` transfer
- **use browser app**: will start the file manager browser app (they can create an account if they want) from where they can add the files to a provider (if they have an account on our service), save it as link in `Shared with me` folder or save them locally. Files will be kept in sync between the two of you
- **use local app**: they can install and/or use the local app from where they can add the files to a provider (if they have an account on our service), save it as link in `Shared with me` folder or sync them locally. Files will be kept in sync between the two of you
    - a QR code will also be presented if they want to use the mobile apps
- **preview**: for some types we offer preview, like for images, videos, audios, text files, PDFs, ...
- 
Sync mode (`Copy`, `Move`, `One-way`, `Two-way`), `compare-mode` and `conflict resolution` are handled as per above.

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-external.png?raw=true)

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/diagram-share-external.png?raw=true)
