//! Sources and sinks (design §4.4).
//!
//! The engine is generic over [`Source`]/[`Sink`]; the app holds the [`AnySource`]/[`AnySink`]
//! enums. Traits are written with explicit `impl Future` returns (RPITIT) rather than
//! `async fn` so that a publicly reachable trait does not trip `async_fn_in_trait`, and so no
//! `Send` bound leaks in — JS futures on wasm are not `Send`.
//!
//! Native file bodies (`FsSource`, `FsSink`) and the wasm bodies in [`web`] are filled in a
//! later step; their signatures are frozen here.

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
    // One policy for every origin: take the current snapshot, compare it the same way, then
    // open. The platforms differ only in how a snapshot is taken, which is `snapshot_of`.
    if !snapshot.matches(&snapshot_of(origin)?) {
        return Err(changed_file());
    }
    match origin {
        #[cfg(not(target_arch = "wasm32"))]
        FileOrigin::Path(path) => FsSource::open(path).map(AnySource::Fs),
        #[cfg(target_arch = "wasm32")]
        FileOrigin::Web(file) => Ok(AnySource::Web(web::WebFileSource::new((**file).clone()))),
        FileOrigin::Memory(bytes) => Ok(AnySource::Mem(MemSource::new(bytes.clone()))),
    }
}

/// What this origin looks like right now — the platform-specific half of the change check.
fn snapshot_of(origin: &FileOrigin) -> io::Result<FileSnapshot> {
    match origin {
        #[cfg(not(target_arch = "wasm32"))]
        FileOrigin::Path(path) => snapshot_path(path),
        #[cfg(target_arch = "wasm32")]
        FileOrigin::Web(file) => Ok(snapshot_web(file)),
        FileOrigin::Memory(bytes) => Ok(FileSnapshot {
            size: bytes.len() as u64,
            modified_ms: None,
        }),
    }
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
        let want = HASH_READ.min((size - offset) as usize);
        let chunk = src.read(offset, want).await?;
        if chunk.is_empty() {
            break;
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
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64);
    Ok(FileSnapshot {
        size: meta.len(),
        modified_ms,
    })
}

/// Snapshot a picked browser file. `File` freezes both values at pick time.
#[cfg(target_arch = "wasm32")]
pub fn snapshot_web(file: &web_sys::File) -> FileSnapshot {
    FileSnapshot {
        size: file.size() as u64,
        modified_ms: Some(file.last_modified() as u64),
    }
}

/// Native file source. Body arrives with the streaming step; the signature is frozen here.
#[cfg(not(target_arch = "wasm32"))]
pub struct FsSource {
    size: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl FsSource {
    pub fn open(path: &std::path::Path) -> io::Result<Self> {
        let _ = path;
        Err(io::Error::other("native file source not yet implemented"))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Source for FsSource {
    fn size(&self) -> u64 {
        self.size
    }

    fn read(&mut self, offset: u64, len: usize) -> impl Future<Output = io::Result<Bytes>> {
        let _ = (self.size, offset, len);
        async move { Err(io::Error::other("native file source not yet implemented")) }
    }
}

/// Native file sink. Writes to `name.part` and renames on finish; body arrives with the
/// streaming step.
#[cfg(not(target_arch = "wasm32"))]
pub struct FsSink {
    written: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl FsSink {
    pub fn create(dir: &std::path::Path, name: &str) -> io::Result<Self> {
        let _ = (dir, name);
        Err(io::Error::other("native file sink not yet implemented"))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Sink for FsSink {
    fn bytes_written(&self) -> u64 {
        self.written
    }

    fn write(&mut self, data: &[u8]) -> impl Future<Output = io::Result<()>> {
        let _ = (self.written, data);
        async move { Err(io::Error::other("native file sink not yet implemented")) }
    }

    async fn finish(self) -> io::Result<SavedFile> {
        Err(io::Error::other("native file sink not yet implemented"))
    }

    async fn abort(self) {}
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

    fn read(&mut self, offset: u64, len: usize) -> impl Future<Output = io::Result<Bytes>> {
        let start = offset.min(self.0.len() as u64) as usize;
        let end = (start + len).min(self.0.len());
        let slice = self.0.slice(start..end);
        async move { Ok(slice) }
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
