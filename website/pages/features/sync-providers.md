[⟵ Back](../features.md)

# Sync between providers

After you setup the providers and Sync rules the sync process is automatically handled in the cloud. All your files will stay in sync in real-time and you can access them directly on providers solutions (local client or in browser) or on our local app or browser app.

We also offer a cloud storage solution based on S3 for now, you can setup our service as a provider and use the local app to sync the files. Then you can take full advantage of syncing those files between providers.

This is useful when you want to keep files in sync between multiple and/or different providers.

## Sync modes

There are 4 modes to sync the files:
1. `Copy`: it will copy new files and changes from source to destination. No deletes or renames will occur. In case the file is changed on destination also (see how this is determined based on **compare-mode** below) it will resolve the conflict based on conflict resolution (see below)
2. `One-way`: like Copy but will also delete and rename files o destination. Conflicts are handled based on conflict resolution
3. `Move`: will move the changed files from source to destination, deleting them from source after transferred. Conflicts are handled based on conflict resolution
4. `Two-way`: will propagate changes in both directions. In case of changes on both sides conflicts are handled based on conflict resolution

## compare-mode

Takes a comma-separated list, with the currently supported values being `size`, `modtime`, and `checksum`. For example, you could compare size and modtime but not the checksum.

## Conflict resolution

If configured first it will try to `auto-merge` (see below). This is disabled by default as it needs to download the remote file to perform the merge. if it doesn’t succeed it will keep the content based on `keep-content-mode` and will copy the other file to a new file with suffix in name like `conflict-<other>-date` indicating the other identifier and the date when the change was made.

## auto-merge

- `text files`: it will try to merge the changes similar to Git. If there are changes on different lines the files will be merged, if the changes are on the same line it’s a conflict and will not merge anything. This is disabled by default as it needs to download the remote file
- `binary files`: it will not do any auto-merge

## keep-content-mode
- `newer (default)`: the newer file (by modtime) is considered the winner, regardless of which side it came from. This may result in having a mix of some winners from `Path1`, and some winners from `Path2`
- `older`: same as newer, except the older file is considered the winner
- `larger`: the larger file (by `size`) is considered the winner (regardless of `modtime`, if any). This can be a useful option for remotes without `modtime` support, or with the kinds of files (such as logs) that tend to grow but not shrink, over time
`smaller`: the smaller file (by `size`) is considered the winner (regardless of `modtime`, if any)
`path1`: the version from `Path1` is unconditionally considered the winner (regardless of `modtime` and `size`, if any). This can be useful if one side is more trusted or up-to-date than the other
`path2`: same as `path1`, except the `path2` version is considered the winner

If either of the underlying remotes lacks support for the chosen method, it will be ignored and will fall back to the default of `newer`. If `modtime` is not supported either by the remote it will fallback to `path1`.

![](https://github.com/radumarias/syncoxiders/blob/main/website/resources/diagram-sync-provider.png?raw=true)
