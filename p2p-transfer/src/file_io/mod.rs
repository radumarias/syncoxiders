//! Sources and sinks (design §4.4).
//!
//! The engine is generic over [`Source`]/[`Sink`]; the app holds the [`AnySource`]/[`AnySink`]
//! enums. Traits are written with explicit `impl Future` returns (RPITIT) rather than
//! `async fn` so that a publicly reachable trait does not trip `async_fn_in_trait`, and so no
//! `Send` bound leaks in — JS futures on wasm are not `Send`.
//!
//! Native files use per-session handles and same-directory staging. Bulk filesystem I/O
//! runs on Tokio's blocking pool, not on the async executor.

use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use tokio::sync::watch;

use crate::blob_store::BlobHash;
use crate::protocol::FileMeta;

#[cfg(target_arch = "wasm32")]
pub mod web;

/// Longest name we ever hand to a filesystem or a download.
const MAX_NAME_BYTES: usize = 255;
/// Fallback when a peer's name sanitises to nothing.
const FALLBACK_NAME: &str = "file.bin";
/// Read granularity of [`hash_source`].
const HASH_READ: usize = 1024 * 1024;
/// Upper bound of a single source read, independent of the transport's chunk size.
pub(crate) const MAX_SOURCE_READ: usize = 4 * 1024 * 1024;

/// Clamp before converting to `usize`: a remaining length of 4 GiB must not become zero
/// on wasm32. Zero-length reads and offsets at or beyond the captured EOF are empty.
pub(crate) fn read_length(size: u64, offset: u64, len: usize) -> io::Result<usize> {
    if len > MAX_SOURCE_READ {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source reads are limited to 4 MiB",
        ));
    }
    Ok(size.saturating_sub(offset).min(len as u64) as usize)
}

/// One file this node offers, as metadata plus a handle — never bytes.
#[derive(Clone, Debug)]
pub struct SharedFile {
    pub meta: FileMeta,
    pub origin: FileOrigin,
    pub snapshot: FileSnapshot,
}

/// What the file looked like when it was hashed.
///
/// Recorded right before hashing and re-checked by [`open_source`] on **every** open (that is,
/// at every `Request`): a file that changed since it was shared is refused with
/// [`io::ErrorKind::InvalidData`] rather than served as a stale mix of old and new bytes.
/// `modified_ms` is `None` where the filesystem has no mtime, and then only the size is
/// compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSnapshot {
    pub size: u64,
    pub modified_ms: Option<u64>,
}

impl FileSnapshot {
    /// Whether `current` is still the file this snapshot was taken of.
    pub fn matches(&self, current: &FileSnapshot) -> bool {
        if self.size != current.size {
            return false;
        }
        match (self.modified_ms, current.modified_ms) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
    }
}

/// The shared file list. A `std` mutex with short critical sections, never held across `.await`.
pub type SharedFiles = Arc<Mutex<Vec<SharedFile>>>;

/// Where a shared file's bytes come from.
#[derive(Clone, Debug)]
pub enum FileOrigin {
    #[cfg(not(target_arch = "wasm32"))]
    Path(std::path::PathBuf),
    #[cfg(target_arch = "wasm32")]
    Web(send_wrapper::SendWrapper<web_sys::File>),
    /// Tests and tiny files.
    Memory(Bytes),
}

/// A readable, seekable source of one file's bytes.
pub trait Source {
    fn size(&self) -> u64;
    /// Exactly `len` bytes at `offset` unless EOF truncates. Never more than 4 MiB per call.
    /// Requests larger than 4 MiB are rejected; truncation of a captured file is an error.
    fn read(&mut self, offset: u64, len: usize) -> impl Future<Output = io::Result<Bytes>>;
}

/// A sequential, append-only destination for one received file.
pub trait Sink {
    fn bytes_written(&self) -> u64;
    fn write(&mut self, data: &[u8]) -> impl Future<Output = io::Result<()>>;
    /// Flush and close; reports where the file went.
    fn finish(self) -> impl Future<Output = io::Result<SavedFile>>;
    /// Best-effort cleanup: delete the partial file, close the stream.
    fn abort(self) -> impl Future<Output = ()>;
}

/// Where a received file ended up. `location` absorbs the native/wasm difference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedFile {
    pub name: String,
    pub size: u64,
    pub location: String,
}

/// Why a sink could not be created or could not take more bytes.
#[derive(Debug)]
pub enum SinkError {
    /// The user dismissed a picker. Never a reason to fall back to another sink.
    Cancelled,
    /// This browser or build cannot offer this sink; the caller may try the next one.
    Unsupported(String),
    Js(String),
    Io(io::Error),
}

impl std::fmt::Display for SinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "cancelled"),
            Self::Unsupported(m) => write!(f, "{m}"),
            Self::Js(m) => write!(f, "browser error: {m}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SinkError {}

impl From<io::Error> for SinkError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Every kind of source, so the app can hold one without `dyn`.
pub enum AnySource {
    #[cfg(not(target_arch = "wasm32"))]
    Fs(FsSource),
    #[cfg(target_arch = "wasm32")]
    Web(web::WebFileSource),
    Mem(MemSource),
}

impl Source for AnySource {
    fn size(&self) -> u64 {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Fs(s) => s.size(),
            #[cfg(target_arch = "wasm32")]
            Self::Web(s) => s.size(),
            Self::Mem(s) => s.size(),
        }
    }

    async fn read(&mut self, offset: u64, len: usize) -> io::Result<Bytes> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Fs(s) => s.read(offset, len).await,
            #[cfg(target_arch = "wasm32")]
            Self::Web(s) => s.read(offset, len).await,
            Self::Mem(s) => s.read(offset, len).await,
        }
    }
}

/// Every kind of sink. The enum is closed and frozen, so the test sinks are variants of it.
pub enum AnySink {
    #[cfg(not(target_arch = "wasm32"))]
    Fs(FsSink),
    #[cfg(target_arch = "wasm32")]
    Fsa(web::FsaSink),
    #[cfg(target_arch = "wasm32")]
    Sw(web::SwSink),
    Mem(MemSink),
    /// Test-only: yields for a fixed delay on every write.
    #[cfg(test)]
    Slow(SlowSink),
    /// Test-only: a write that never returns.
    #[cfg(test)]
    Stuck(StuckSink),
}

impl Sink for AnySink {
    fn bytes_written(&self) -> u64 {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Fs(s) => s.bytes_written(),
            #[cfg(target_arch = "wasm32")]
            Self::Fsa(s) => s.bytes_written(),
            #[cfg(target_arch = "wasm32")]
            Self::Sw(s) => s.bytes_written(),
            Self::Mem(s) => s.bytes_written(),
            #[cfg(test)]
            Self::Slow(s) => s.bytes_written(),
            #[cfg(test)]
            Self::Stuck(s) => s.bytes_written(),
        }
    }

    async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Fs(s) => s.write(data).await,
            #[cfg(target_arch = "wasm32")]
            Self::Fsa(s) => s.write(data).await,
            #[cfg(target_arch = "wasm32")]
            Self::Sw(s) => s.write(data).await,
            Self::Mem(s) => s.write(data).await,
            #[cfg(test)]
            Self::Slow(s) => s.write(data).await,
            #[cfg(test)]
            Self::Stuck(s) => s.write(data).await,
        }
    }

    async fn finish(self) -> io::Result<SavedFile> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Fs(s) => s.finish().await,
            #[cfg(target_arch = "wasm32")]
            Self::Fsa(s) => s.finish().await,
            #[cfg(target_arch = "wasm32")]
            Self::Sw(s) => s.finish().await,
            Self::Mem(s) => s.finish().await,
            #[cfg(test)]
            Self::Slow(s) => s.finish().await,
            #[cfg(test)]
            Self::Stuck(s) => s.finish().await,
        }
    }

    async fn abort(self) {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Fs(s) => s.abort().await,
            #[cfg(target_arch = "wasm32")]
            Self::Fsa(s) => s.abort().await,
            #[cfg(target_arch = "wasm32")]
            Self::Sw(s) => s.abort().await,
            Self::Mem(s) => s.abort().await,
            #[cfg(test)]
            Self::Slow(s) => s.abort().await,
            #[cfg(test)]
            Self::Stuck(s) => s.abort().await,
        }
    }
}

/// Open a per-session reader for a shared file: each connection gets its own handle, so
/// concurrent receivers are independent.
///
/// Re-validates `snapshot` before returning; a file that changed since it was hashed is
/// [`io::ErrorKind::InvalidData`], never a stale read.
pub async fn open_source(origin: &FileOrigin, snapshot: &FileSnapshot) -> io::Result<AnySource> {
    let (source, current) = match origin {
        #[cfg(not(target_arch = "wasm32"))]
        FileOrigin::Path(path) => {
            let path = path.clone();
            let source = tokio::task::spawn_blocking(move || FsSource::open(&path))
                .await
                .map_err(io::Error::other)??;
            // Inspect the opened handle, not the path before opening it: a rename between
            // metadata(path) and open(path) must not defeat the snapshot check.
            let current = source.snapshot;
            (AnySource::Fs(source), current)
        }
        #[cfg(target_arch = "wasm32")]
        FileOrigin::Web(file) => (
            AnySource::Web(web::WebFileSource::new((**file).clone())),
            snapshot_web(file),
        ),
        FileOrigin::Memory(bytes) => (
            AnySource::Mem(MemSource::new(bytes.clone())),
            FileSnapshot {
                size: bytes.len() as u64,
                modified_ms: None,
            },
        ),
    };
    if !snapshot.matches(&current) {
        return Err(changed_file());
    }
    Ok(source)
}

/// The one error text the sender turns into a file-scoped `Error` frame.
pub(crate) fn changed_file() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "file changed since it was shared",
    )
}

/// Streaming BLAKE3 of a source, 1 MiB at a time, reporting progress in `[0, 1]`.
///
/// This is the same hasher the receiver runs incrementally, so a whole-file hash and a
/// chunk-by-chunk hash agree.
pub async fn hash_source<S: Source>(
    src: &mut S,
    progress: &watch::Sender<f32>,
) -> io::Result<BlobHash> {
    let size = src.size();
    let mut hasher = blake3::Hasher::new();
    let mut offset = 0u64;
    while offset < size {
        let want = read_length(size, offset, HASH_READ)?;
        let chunk = src.read(offset, want).await?;
        if chunk.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "source ended before its declared size",
            ));
        }
        if chunk.len() > want {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "source returned more bytes than requested",
            ));
        }
        hasher.update(&chunk);
        offset += chunk.len() as u64;
        progress.send_replace(offset as f32 / size as f32);
    }
    progress.send_replace(1.0);
    Ok(BlobHash(*hasher.finalize().as_bytes()))
}

/// Make a peer-supplied name safe to hand to a filesystem or a download.
///
/// Strips directory components (both separators, so `../../etc/passwd` becomes `passwd`) and
/// control characters, rejects `""`, `"."` and `".."`, and caps the result at 255 bytes on a
/// character boundary. Never returns an empty string.
pub fn sanitize_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return FALLBACK_NAME.to_string();
    }
    if cleaned.len() <= MAX_NAME_BYTES {
        return cleaned.to_string();
    }
    // A character is at most 4 bytes, so the walk stops at 252 at the very worst and the
    // result can never come back empty.
    let mut end = MAX_NAME_BYTES;
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    cleaned[..end].to_string()
}

/// Snapshot a path for the change check of [`open_source`].
#[cfg(not(target_arch = "wasm32"))]
pub fn snapshot_path(path: &std::path::Path) -> io::Result<FileSnapshot> {
    let meta = std::fs::metadata(path)?;
    Ok(snapshot_metadata(&meta))
}

#[cfg(not(target_arch = "wasm32"))]
fn snapshot_metadata(meta: &std::fs::Metadata) -> FileSnapshot {
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64);
    FileSnapshot {
        size: meta.len(),
        modified_ms,
    }
}

/// Snapshot a picked browser file. `File` freezes both values at pick time.
#[cfg(target_arch = "wasm32")]
pub fn snapshot_web(file: &web_sys::File) -> FileSnapshot {
    FileSnapshot {
        size: file.size() as u64,
        modified_ms: Some(file.last_modified() as u64),
    }
}

/// A per-session native handle, with the size captured when it was opened.
#[cfg(not(target_arch = "wasm32"))]
pub struct FsSource {
    file: Arc<Mutex<std::fs::File>>,
    snapshot: FileSnapshot,
}

#[cfg(not(target_arch = "wasm32"))]
impl FsSource {
    pub fn open(path: &std::path::Path) -> io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source is not a regular file",
            ));
        }
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
            snapshot: snapshot_metadata(&metadata),
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Source for FsSource {
    fn size(&self) -> u64 {
        self.snapshot.size
    }

    async fn read(&mut self, offset: u64, len: usize) -> io::Result<Bytes> {
        let len = read_length(self.size(), offset, len)?;
        if len == 0 {
            return Ok(Bytes::new());
        }
        let file = self.file.clone();
        tokio::task::spawn_blocking(move || {
            use std::io::{Read, Seek, SeekFrom};
            // Seeking and reading under one lock works on Unix and Windows. If a read
            // future is cancelled, its blocking job completes before the next seek.
            let mut file = file.lock().map_err(|_| io::Error::other("source lock poisoned"))?;
            file.seek(SeekFrom::Start(offset))?;
            let mut bytes = vec![0; len];
            file.read_exact(&mut bytes)?;
            Ok(Bytes::from(bytes))
        })
        .await
        .map_err(io::Error::other)?
    }
}

/// Native staging sink. Only `finish`, called after the engine verifies length and hash,
/// publishes the file. Publication uses a hard link, never a rename that can overwrite.
///
/// The destination directory must support hard links (e.g. NTFS, ext4, APFS; not FAT).
/// Staging is in that same directory, so publication cannot cross filesystems. Names are
/// private until finish, but this is not a crash-recovery journal: process death may leave
/// a `.p2p-*.part` file, and directory entries are not fsynced for power-loss durability.
#[cfg(not(target_arch = "wasm32"))]
pub struct FsSink {
    stage: Arc<Mutex<StagingFile>>,
    dir: std::path::PathBuf,
    name: String,
    written: u64,
    ready: bool,
}

/// The last owner closes the handle before unlinking, including when a cancelled blocking
/// job outlives its sink. Closing first matters on Windows.
#[cfg(not(target_arch = "wasm32"))]
struct StagingFile {
    file: Option<std::fs::File>,
    path: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for StagingFile {
    fn drop(&mut self) {
        drop(self.file.take());
        if let Err(error) = std::fs::remove_file(&self.path) {
            if error.kind() != io::ErrorKind::NotFound {
                log::warn!("could not remove staging file {}: {error}", self.path.display());
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl FsSink {
    const COLLISION_ATTEMPTS: u32 = 1024;

    pub fn create(dir: &std::path::Path, name: &str) -> io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

        let dir = dir.canonicalize()?;
        let name = sanitize_name(name);
        for _ in 0..Self::COLLISION_ATTEMPTS {
            // Independent of the peer's name: even a 255-byte name leaves room for staging.
            let id = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!(".p2p-{}-{id}.part", std::process::id()));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        stage: Arc::new(Mutex::new(StagingFile { file: Some(file), path })),
                        dir,
                        name,
                        written: 0,
                        ready: true,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "too many staging name collisions"))
    }

    fn final_name(&self, attempt: u32) -> String {
        if attempt == 0 {
            return self.name.clone();
        }
        let suffix = format!(" ({attempt})");
        let mut end = self.name.len().min(MAX_NAME_BYTES - suffix.len());
        while !self.name.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}{suffix}", &self.name[..end])
    }

    fn check_ready(&self) -> io::Result<()> {
        if !self.ready {
            return Err(io::Error::other("sink write failed or was cancelled; abort this sink"));
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Sink for FsSink {
    fn bytes_written(&self) -> u64 {
        self.written
    }

    async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.check_ready()?;
        let written = self.written.checked_add(data.len() as u64)
            .ok_or_else(|| io::Error::other("sink byte count overflow"))?;
        self.ready = false;
        let stage = self.stage.clone();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut stage = stage.lock().map_err(|_| io::Error::other("sink lock poisoned"))?;
            stage.file.as_mut().ok_or_else(|| io::Error::other("sink is closed"))?
                .write_all(&data)
        })
        .await
        .map_err(io::Error::other)??;
        self.written = written;
        self.ready = true;
        Ok(())
    }

    async fn finish(self) -> io::Result<SavedFile> {
        self.check_ready()?;
        let stage = self.stage.clone();
        tokio::task::spawn_blocking(move || {
            let stage = stage.lock().map_err(|_| io::Error::other("sink lock poisoned"))?;
            stage.file.as_ref().ok_or_else(|| io::Error::other("sink is closed"))?.sync_all()
        })
        .await
        .map_err(io::Error::other)??;

        // No await after this point: a cancelled flush future can only clean up, never
        // publish later from a detached blocking job. Only short namespace operations run
        // here; bulk writes and flushing have already completed on the blocking pool.
        let mut stage = self.stage.lock().map_err(|_| io::Error::other("sink lock poisoned"))?;
        drop(stage.file.take());
        for attempt in 0..Self::COLLISION_ATTEMPTS {
            let name = self.final_name(attempt);
            let path = self.dir.join(&name);
            match std::fs::hard_link(&stage.path, &path) {
                Ok(()) => {
                    return Ok(SavedFile {
                        name,
                        size: self.written,
                        location: path.to_string_lossy().into_owned(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "too many destination name collisions"))
    }

    async fn abort(self) {
        // Await outstanding writes before unlinking. Dropping this future is also safe:
        // the blocking job holds the last owner until it can close and remove the file.
        let stage = self.stage;
        let _ = tokio::task::spawn_blocking(move || {
            drop(stage.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
            drop(stage);
        }).await;
    }
}

/// In-memory source: tests, and tiny files.
pub struct MemSource(Bytes);

impl MemSource {
    pub fn new(bytes: Bytes) -> Self {
        Self(bytes)
    }
}

impl Source for MemSource {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }

    async fn read(&mut self, offset: u64, len: usize) -> io::Result<Bytes> {
        let len = read_length(self.size(), offset, len)?;
        let start = offset.min(self.size()) as usize;
        Ok(self.0.slice(start..start + len))
    }
}

/// In-memory sink with a hard cap. On wasm, `finish` hands the bytes to the browser as a
/// download; natively it is a test sink.
pub struct MemSink {
    buf: Vec<u8>,
    cap: usize,
    name: String,
}

impl MemSink {
    pub fn new(name: String, cap: usize) -> Self {
        Self {
            buf: Vec::new(),
            cap,
            name,
        }
    }

    /// The bytes received so far (tests).
    pub fn contents(&self) -> &[u8] {
        &self.buf
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Sink for MemSink {
    fn bytes_written(&self) -> u64 {
        self.buf.len() as u64
    }

    fn write(&mut self, data: &[u8]) -> impl Future<Output = io::Result<()>> {
        let fits = self.buf.len() + data.len() <= self.cap;
        if fits {
            self.buf.extend_from_slice(data);
        }
        async move {
            if fits {
                Ok(())
            } else {
                // A bound, not routing advice: by the time bytes are flowing the sink was
                // chosen long ago. `pick_sinks` is where the size decides the route.
                Err(io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "this file is larger than the in-memory sink can hold",
                ))
            }
        }
    }

    async fn finish(self) -> io::Result<SavedFile> {
        let saved = SavedFile {
            name: self.name.clone(),
            size: self.buf.len() as u64,
            location: MEM_SINK_LOCATION.to_string(),
        };
        #[cfg(target_arch = "wasm32")]
        web::trigger_download(&self.name, &self.buf)
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(saved)
    }

    async fn abort(self) {}
}

/// Where a memory-route file ends up, as reported to the user.
#[cfg(target_arch = "wasm32")]
const MEM_SINK_LOCATION: &str = "Downloaded to browser";
/// Where a memory-route file ends up, as reported to the user.
#[cfg(not(target_arch = "wasm32"))]
const MEM_SINK_LOCATION: &str = "In memory";

/// Test-only sink that yields for `delay` on every write, so a test can hold the receive
/// window open without blocking the runtime.
#[cfg(test)]
pub struct SlowSink {
    inner: MemSink,
    delay: std::time::Duration,
}

#[cfg(test)]
impl SlowSink {
    pub fn new(name: String, cap: usize, delay: std::time::Duration) -> Self {
        Self {
            inner: MemSink::new(name, cap),
            delay,
        }
    }

    pub fn contents(&self) -> &[u8] {
        self.inner.contents()
    }
}

#[cfg(test)]
impl Sink for SlowSink {
    fn bytes_written(&self) -> u64 {
        self.inner.bytes_written()
    }

    fn write(&mut self, data: &[u8]) -> impl Future<Output = io::Result<()>> {
        let delay = self.delay;
        async move {
            n0_future::time::sleep(delay).await;
            self.inner.write(data).await
        }
    }

    fn finish(self) -> impl Future<Output = io::Result<SavedFile>> {
        self.inner.finish()
    }

    fn abort(self) -> impl Future<Output = ()> {
        self.inner.abort()
    }
}

/// Test-only sink whose `write` never returns, so a test can prove the engine's write
/// deadline and cancellation are real.
#[cfg(test)]
pub struct StuckSink {
    inner: MemSink,
    aborted: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(test)]
impl StuckSink {
    pub fn new(name: String) -> Self {
        Self {
            inner: MemSink::new(name, usize::MAX),
            aborted: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Flips to `true` once `abort` ran; readable after the sink was consumed.
    pub fn abort_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.aborted.clone()
    }
}

#[cfg(test)]
impl Sink for StuckSink {
    fn bytes_written(&self) -> u64 {
        self.inner.bytes_written()
    }

    fn write(&mut self, data: &[u8]) -> impl Future<Output = io::Result<()>> {
        let _ = data;
        std::future::pending()
    }

    fn finish(self) -> impl Future<Output = io::Result<SavedFile>> {
        self.inner.finish()
    }

    async fn abort(self) {
        self.aborted
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
