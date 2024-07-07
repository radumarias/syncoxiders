[⟵ Back](../features.md#features)

# Many supported providers

Initially we will use [rclone](https://rclone.org#providers) on the service side and we will support all their supported providers.

In the future we plan to implement our own clients for several providers which might offer more granular and efficient sync. It helps us for example to catch remote changes almost in real-time (as soon as we are notified by the remote), and propagate the changes to other destinations, without needing to use file watchers or rely on the sync logic of rclone provider implementation.
