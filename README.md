# ![](website/resources/syncoxiders-icon-20p.png) SyncOxiders

Cloud file and email Sync, file Sharing, inter-cloud Encryption and Backup solution written in Rust and modern technologies.

The purpose of this project is to offer an easy and reliable way to sync files and emails between multiple providers and share files between multiple storage providers (like Google Drive, Dropbox, S3, SFTP servers, ...) and local files. Also simple way for backup of your files and emails and encryption. 
It offers real time sync (from simple Copy One-way to Two-way Sync) all handled in the cloud, without the explicit need of local clients.

> [!WARNING]  
> For now it's in PoC phase, it has some or the core components, like encryption, basic Google Drive client and a basic [CLI app](website/pages/poc-demo.md#using-cli).

It's using [rencfs](https://github.com/radumarias/rencfs) for encryption and [gdrive-rs](https://github.com/radumarias/gdrive-rs) for accesing Google Drive.

> [!IMPORTANT]  
> It you could take this [**SURVEY**](https://forms.gle/qgnWBJhzCpzPLSmv5) to express your opinion about the current solution and offer your opinion on what features you would want from a service like this it would help a lot.

You can see rhe [results](https://docs.google.com/forms/d/1d4V8BZB7TGp08NhY6_L0kUgcGe0glRFOjp4rjrt7_bs/viewanalytics?chromeless=1) of the survey.

> [!NOTE]  
In many cases we'll use present tense for several functionality, even though they are not yet implemented, it's used to give an idea of what the system could be.

[What's with the name](website/pages/name.md)

# PoC

You can see more [details](website/pages/poc-demo.md) on what's working now, play with the [CLI app](website/pages/poc-demo.md#using-cli) and see a short [demo](https://www.youtube.com/watch?v=JHQC1XpCzQw).

Working on having these in up to 2 months:
- in `Docker` ability to sync 2 folders in the filesystem
- run `rclone` in `Docker` and mount `Google Drive` and `Dropbox` or `MS OneDrive` in 2 folders
- from CLI trigger a sync which will make a Two-Way sync between the folders, first sync will do a union between the 2, no delete or rename will be performed
- do some changes in both local folders and trigger a sync, from now on it will propagate deletes and renames also
- do some changes on the remotes, trigger a sync and make sure changes are propagated in both local folders and on remotes
- save files encrypted using `rencfs`
  - this will save encrypted data on the mount points of `rclone` and expose them with `FUSE`

<img src="website/resources/poc.png" style = "width: 100%; max-width: 1000px; height: auto;">

# MVP

It would be possible to have something in about 6 months with this functionality:
- integration with `Google Drive` and `Dropbox` or `MS OneDrive`
- Sync between the two
- Share files from providers with another user
- browser app with basic functionality like:
  - adding providers
  - setup sync rules
  - share between providers
- some basic functionality of sharing local files, no sync between them
- encryption

For this phase we will still be using `rclone` to access providers, this is to simplify the access. But for future plan is to:
- implement our own clients that will directly communicate with the providers API
- receive changes in close to real-time
- store the changes in `Kafka` and window them (group them) with `Flink`
- feed them as changes tree to the `files tree merge` algorithm which will do the merge, resolve conflicts and applying changes to the other providers or local files

<img src="website/resources/mvp.png" style = "width: 100%; max-width: 1000px; height: auto;">

# The big picture

This is what it's planned to have in the end.

<img src="website/resources/services.png" style = "width: 100%; max-width: 1000px; height: auto;">

[Use cases](website/pages/use-cases.md)

[Features](website/pages/features.md)

[What separates it from other products](website/pages/compare.md)

[How it works](website/pages/how-it-works.md)

[Tech stack](website/pages/stack.md)

# Contribute

Feel free to fork it, change and use it in any way that you want. If you build something interesting and feel like sharing pull requests are always appreciated.

## How to contribute

### Browser

If you want to give it a quick try and not setup anything locally you can  
[![Open in Gitpod](https://gitpod.io/button/open-in-gitpod.svg)](https://gitpod.io/#https://github.com/radumarias/syncoxiders)

[![Open Rustlings On Codespaces](https://github.com/codespaces/badge.svg)](https://github.com/codespaces/new/?repo=radumarias%2Fsyncoxiders&ref=main)

You can compile it, run it, and give it a quick try in browser. After you start it from above

```bash
sudo apt-get update && sudo apt-get install fuse3
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
mkdir sync && mkdir sync/repo sync/a sync/b
```

Add some files in `sync/a` and then run th sync

```bash
cargo run --release --bin syncoxiders -- --repo sync/repo sync/a sync/b
```

Now check `sync/b` it should have same content as `file/a`.

For now this **is working**
- sync files: create, delete, update, move
- sync one to many, you can put several paths, it will sync from path1 to all others

It **DOESN'T** work
- folders sync: create, delete, rename, will be fixed soon

### Locally

#### Getting the sources

```bash
git clone git@github.com:radumarias/syncoxiders.git && cd syncoxiders
````

#### Dependencies

##### Rust

To build from source, you need to have Rust installed, you can see more details on how to install
it [here](https://www.rust-lang.org/tools/install).

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
````

Accordingly, it is customary for Rust developers to include this directory in their `PATH` environment variable.
During installation `rustup` will attempt to configure the `PATH`. Because of differences between platforms, command
shells,
and bugs in `rustup`, the modifications to `PATH` may not take effect until the console is restarted, or the user is
logged out, or it may not succeed at all.

If, after installation, running `rustc --version` in the console fails, this is the most likely reason.
In that case please add it to the `PATH` manually.

Project is setup to use `nightly` toolchain in `rust-toolchain.toml`, on first build you will see it fetch the nightly.

Make sure to add this you your `$PATH` too

```bash
export PATH="$PATH::$HOME/.cargo/bin"
```

##### Other dependencies

Also, these deps are required (or based on your distribution):

###### Arch

```bash
sudo pacman -Syu && sudo pacman -S base-devel act
```

###### Ubuntu

```bash
sudo apt-get update && sudo apt-get install build-essential act
```

###### Fedora

```bash
sudo dnf update && sudo dnf install && dnf install @development-tools act
```

#### Build for debug

```bash
cargo build
```

#### Build release

```bash
cargo build --release
```

### Run

```bash
cargo run --release --bin syncoxiders -- --repo REPO A B
```

### Developing inside a Container

See here how to configure for [RustRover](https://www.jetbrains.com/help/rust/connect-to-devcontainer.html) and for [VsCode](https://code.visualstudio.com/docs/devcontainers/containers).

You can use the `.devcontainer` directory from the project to start a container with all the necessary tools to build
and run the app.

Please see [CONTRIBUTING.md](https://github.com/radumarias/rencfs/blob/main/CONTRIBUTING.md).

# Minimum Supported Rust Version (MSRV)

The minimum supported version is `1.75`.
