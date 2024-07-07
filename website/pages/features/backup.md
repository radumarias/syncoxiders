[⟵ Back](../features.md#features)

# Backups with BorgBackup, encrypted, deduplicated and compressed

We’re using [BorgBackup](https://www.borgbackup.org) which handles encryption, deduplication and compression. We offer BorgBackup repos that can be used to backup data. Subscription is separate from the Sync and Share services.

There are 2 scopes of backups:
- **local data**: using the local app or browser app you can setup backup schedule for local files, this will be done with cron jobs, scheduled tasks on Windows, termux-job-scheduler on Android and ibimobiledevice in iOS
- **provider files**: from browser app or local app you can schedule backups from provider files. The service will need to read all files and changes from the provider and will backup on our repo. All the process is handled by the service, you don’t need the local app running for this.

When you want to restore some data you can use the local app or browser app, you’ll select the archive to restore from, you can then access files in the app and copy them locally or to a provider. In the local app you can also mount it in the OS from where you can copy the files. You can also use BorgBackup CLI or `Vorta` for GUI if you want, setup will be provided for you in the local app, browser app and on our website.
