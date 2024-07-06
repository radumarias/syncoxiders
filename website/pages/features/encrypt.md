[⟵ Back](../features.md#features)

# Encrypt

In transit data (between our apps, service, provider, browser, torrent clients) is always encrypted using TLS 1.3 when possible. But you may decide to keep your files encrypted (at rest), for privacy reasons. Your provider will not have access to the content of your files, nor metadata. You will need the local app or browser app to access the files. Works with local files too.

You can choose to encrypt the whole provider content or just selected folders.

## Encrypted share

It uses `PGP` to handle encryption. There are 2 options:
- **share with users in our service**: a session key will be generated then encrypted with destination’s user public key. The encrypted key will be sent along when sharing. If user is adding the share to providers or local app the decryption and encryption will be handled automatically. The file will be decryted in memory on demand using the session key (decrypting only the chunks that are read). On the disk or in provider it will be kept encrypted all the time
- **share with external users**: if the other user is not using our service or they download the file with browser or torrent, before they can download they will need to upload their public key. After that the file is downloaded as encrypted along with the encrypted session key. Users can then decrypt the session key with their private key and then decrypt the file, this can be handled with any PGP client like GPG

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/diagram-encrypt-share.png?raw=true)
