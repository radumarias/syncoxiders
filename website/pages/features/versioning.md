[⟵ Back](../features.md#features)

# File history and versioning

## History

Many providers offer a history of changes and operations for each file. Where supported we will use that, where not we will try to implement and make it available in apps and clients. The same will offer this for local files.

This mostly refers to name change and activity history, not convent history, see versioning for that.

## Versioning

Many providers support having multiple versions of the same file, as you copy a new file with same name or change it, it will be a new version. When supported by providers we will use it, when not we will try to implement it our own and make it available in apps and clients. Works also for local files.

Implementation note: this could be a good use case of git repos with LFS. Git objects are compressed and deduplicated inside same object.
