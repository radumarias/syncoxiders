[⟵ Back](../features.md#features)

# Receiving files

Someone might want to send you a file. For that you first setup a Request Files link, send it to them, and we’ll handle how the file gets back to you.

This is similar to [S3 presigned URLs](https://docs.aws.amazon.com/AmazonS3/latest/userguide/using-presigned-url.html) but create a dest folder where others can upload more files and even folders.
Useful for `One-way` one time transfers. After upload the files will not be kept in sync.

You can create such an URL from a provider folder or local folder. For local folders the browser app or local app need to be running when the user upload the files.

We’ll notify the user with the link, or you can send it to them. You will be notified when they uploaded new files.

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/frame-receive.png?raw=true)

You can define these:
- title and description
- destination folder: in your provider or locally
- deadline: until the link is valid
- late uploads: if users are allowed to upload after this date
- password: a password user need to use to upload files

When user opens the link it will be presented with:
- **upload with browser**: will use the browser to upload multiple files and folders
- **upload with torrent**: based on a link the user’s torrent client will be started and they can upload by adding files or folders to their local folder linked to the torrent
- **upload with browser app**: will start the browser file manager app (they can create an account if they want) from where they can choose to upload files from a provider (if they have an account on our service), `Shared with me` folder or local files
- **upload with local app**: they can install and/or use the local app from where they can choose to upload a files from a provider (if they have an account on our service), `Shared with me` folder or local files
    - a QR code will also be presented if they want to use the mobile apps

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/diagram-receive.png?raw=true)
