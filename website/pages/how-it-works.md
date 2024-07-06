[⟵ Back](../../README.md#how-it-works)

# How it works
There are several ways to interact with our service:
- `browser app`: we expose an app built for WebAssembly than can manage all operations and transfers
- `local app`: we also have a local app which is very similar to the browser app, in fact they share the same code. The same, will handle all operations and transfers with the service and with other P2P apps. Will expose files with FUSE or other technologies
- `mobile app`: similar to local app but for Android and iOS
- `clients`:
  - `CLI app`: a command line interface to interact with our service. It can also expose files with FUSE or other technologies
  - `libs`: we have libs in several languages. They can also expose files with FUSE or other technologies
- `API`: we expose a REST API and gRPC service that you can use to manage all operations and transfers. It uses WebSockets to notify you about changes

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/clients.png?raw=true)

## Sync

### Files

#### Between providers

- You setup your providers on our site, you login and grant access to all providers that you need
- Define sync rules, like just Copy from one provider to another or Two-way sync between them
- From this point on we’ll handle syncing all changes between providers (no local app needed for this, it’s all happening in the cloud)
- We also offer a cloud storage solution, you can setup our service as a provider and use a local app to sync the files. Then you can take full advantage of syncing those files between providers

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-providers.png?raw=true)

#### Local files

- In case you have some local files you want to keep in sync between multiple devices, without any file storage providers, you can use our local app on each device which will handle sync in P2P manner using QUIC. Apps need to be running for this. This is similar to Resilio Sync
- All devices are authenticated using web of trust to make sure are trusted by you

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-local-files.png?raw=true)

#### Emails

You can define sync rules between multiple email addresses. This extends mail forwarding as it’s Two-Way sync and you can define specific filters like from what emails, what words, specific actions like where to sync it, which provider has priority, if we should mark it as read or archive it.

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-emails.png?raw=true)

## Share files

### Between service users

- You may share a file from your provider with another person, using our service. After they accept it it will be accessible in their provider, both of you can change the file and the changes are kept in sync
- The service will notify the other user and he will select where to save the files, what provider and folder. You will be notified back when they accepted and also when they changed something
- The user may decide to not add the shared files to their providers but keep it as a link (reference), in that case the shared files will be visible to him in a Shared with me folder in the browser or local app. Any changes he makes in there will synced
- On sharing on the same provider if it supports sharing it will use their build-in sharing functionality. On other cases it will copy the file to destination provider and then keep it in sync between the two of you

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-providers.png?raw=true)

### With external users

- You can share a file from your providers with someone not using our service or any other storage providers. We’ll generate a link, we’ll email to them, or you can send them, and they can get it from browser, with a torrent client or sync it with our local client
- If they are using our browser or local app you will be notified when they changed something and they will be notified when you make changes

### Local files

- You can quickly share a file directly with someone via a link, they can get it from browser, with a torrent client or sync it with our browser or local app
- If both of you are using the local app you can share directly from the app and files will be kept in sync
- The share can be made in 2 ways:
    - `P2P`: in a peer-to-peer manner, your can use our file manager app in browser (uses WebRTC) or local app (uses HTTP, QUIC and BitTorrent), in both cases they need to be running for the user to download the files
    - `Upload to our service`: with the browser app or local app you can first upload the local file to us and user will download it directly from us. The browser app or local app don’t need to be running for the download to happen

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-local-file.png?raw=true)

### Receiving files into provider

Someone might want to send you a file, you setup a Request Files link, send it to them, and we’ll handle how the files gets back to you.

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/receive-file-with-provider.png?raw=true)

### Receiving files locally

User might upload the file to our service and we’ll save it to your provider, or it can send it directly to you via P2P.

You can mix between these, for example you can share a local file and the other person who save it on their provider, both files will be kept in sync.
Or you can create a Request Files link based on a provider folder (or local folder) and others can send you files from their provider or local files using their browser, torrent client or local app.

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/receive-local-file.png?raw=true)

## Encrypt

You may want to keep your files encrypted for privacy. Your provider will not have access to the content of your files, nor metadata. You will need the local app or browser app to access the files. It works with local files too.

Encrypted share: it uses PGP to handle encryption. There are 2 options:
- `share with users on our service`: a session key will be generated then encrypted with destination’s user’s public key. The encrypted key will be sent along with the file when sharing. If user is adding the shared file to provider or local app the file will be decryted on demand using the session key. On the disk or on provider it will be kept encrypted all the time
- `share with external user`: if user is not using our service or downloads the file with browser or torrent, before downloading the file they will need to upload their public key. After that the file is downloaded as encrypted along with the encrypted session key. User can then decrypt the session key with his private key and then decrypt the file, this can be handled with any PGP client like GPG (instructions will be provided on the download page)

### Sync encrypted between providers

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-encrypted.png?raw=true)

### Share encrypted with users on our service

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-encryptyed-with-service-user.png?raw=true)

### Share encrypted with external user

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-encrypte-with-external-user.png?raw=true)

## Backup

We’re using BorgBackup which handles encryption, deduplication and compression. We offer BorgBackup repos that can be used to backup data. Subscription is separate from the Sync and Share services.

### Files

There are 2 sources of backups:
- `files from provider`: from browser app or local app you can schedule backups from provider files. The service will need to read all files and changes from the provider and will backup on our repo. Everything is handled by the service, you don’t need the local app running for this
- `local files`: using the local app you can setup backups schedule for local files and the local app will handle the backup process

When you want to restore some data you can use the local app, you’ll select the archive to restore from and it will be mounted in OS from where you can copy the files. You can also use BorgBackup CLI or Vorta for GUI if you want, setup will be provided for you in the local app, browser app and on our website

### Backup files from provider

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-provider.png?raw=true)

#### Backup local files

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-local-files.png?raw=true)

When you want to restore some data you can use the local app, you’ll select the archive to restore from and it will be mounted in OS from where you can copy the files. You can also use borg CLI or Vorta for GUI if you want, setup will be provided for you in the local app, browser app and on our website.

### Emails

From the browser app or local app you can define backup rules and schedule. There are 2 modes:
- `backup by our service`: running backup in scheduled time will be handled by our service, all emails will be read by us from your email provider and saved to our BorgBackup repo
- `backup locally`: local app will run on defined intervals, read the mails directly from your email provider and backup to our BorgBackup repo

#### Backup by our service

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-emails-with-service.png?raw=true)

#### Backup locally

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-emails-locally.png?raw=true)
