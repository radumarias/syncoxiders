[⟵ Back](../../README.md)

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

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-providers.png?raw=true)

#### Local files

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-local-files.png?raw=true)

#### Emails

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-emails.png?raw=true)

## Share files

### Between service users

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-providers.png?raw=true)

### With external users

- you can share a file from your providers with someone not using our service or any other storage providers. We’ll generate a link, we’ll email to them, or you can send them, and they can get it from browser, with a torrent client or sync it with our local client
- if they are using our browser or local app you will be notified when they changed something and they will be notified when you make changes

### Local files

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-local-file.png?raw=true)

### Receiving files into provider

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/receive-file-with-provider.png?raw=true)

### Receiving files locally

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/receive-local-file.png?raw=true)

You can mix between these, for example you can share a local file and the other person saves it on their provider, both files will be kept in sync.  
Or you can create a Request Files link based on a provider folder (or local folder) and others can send you files from their provider or local files using their browser, torrent client or local app.

## Encrypt

### Sync encrypted between providers

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/sync-encrypted.png?raw=true)

### Share encrypted with users on our service

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-encryptyed-with-service-user.png?raw=true)

### Share encrypted with external user

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/share-encrypte-with-external-user.png?raw=true)

## Backup

### Files

### Backup files from provider

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-provider.png?raw=true)

#### Backup local files

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-local-files.png?raw=true)

When you want to restore some data you can use the local app, you’ll select the archive to restore from and it will be mounted in OS from where you can copy the files. You can also use borg CLI or Vorta for GUI if you want, setup will be provided for you in the local app, browser app and on our website.

### Emails

#### Backup by our service

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-emails-with-service.png?raw=true)

#### Backup locally

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/backup-emails-locally.png?raw=true)
