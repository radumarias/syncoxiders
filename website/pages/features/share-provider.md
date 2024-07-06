[⟵ Back](../features.md#features)

# Share between service users

You may share a file from your provider with another person, using our service. After they accept it it will be accessible in their provider, both of you can change the file and the changes are kept in sync.

The service will notify the other user and he will select where to save the files, what provider and folder. You will be notified back when they accepted and also when they changed something.

The user may decide to not add the shared files to their providers but keep it as a link (reference), in that case the shared files will be visible to him in a Shared with me folder in the browser or local app. Any changes he makes in there will synced.

On sharing on the same provider if it supports sharing it will use its build-in sharing functionality when possible.

In other cases, it will copy the file to destination provider and then keep it in sync between the two of you. Both of you need to use our service with the providers setup in our service for this to work. Sync mode (Copy, Move, One-way, Two-way), compare-mode and conflict resolution are handled as per above.

The service will notify the user. You will be notified back when they accepted and when they changed something, they will be notified when you changed something.

When they access the link will have these options:
- **download with browser**: download will be handled by their browser, it’s a `One-way` one time transfer
- **download with torrent**: they will use a torrent client to get the file. We will act as the torrent tracker. This is a One-way transfer
- **use browser app**: will start the file manager browser app from where they can add the files to a provider, save it as link in `Shared with me` folder or save them locally. Files will be kept in sync between the two of you
- **use local app**: they can install and/or use the local app from where they can add the files to a provider, save it as link in `Shared with me` folder or save them locally. Files will be kept in sync between the two of you
    - a QR code will also be presented if they want to use the mobile apps
- **preview**: for some types we offer preview, like for images, videos, audios, text files, PDFs, ...

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-others.png?raw=true)
