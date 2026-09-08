//! Browser sources and sinks (design §4.4.2–§4.4.3), wasm32 only.
//!
//! Signatures are frozen here so the app lane can wire the Save flow against them. Only the
//! memory route is live at this step: [`pick_sinks`] already serves `SinkPref::Mem` — and the
//! `Auto` fallback, since the picker and service-worker branches report `Unsupported` until
//! their bodies land — so a browser receive completes end to end through [`trigger_download`].

use std::future::Future;
use std::io;

use bytes::Bytes;
use js_sys::Uint8Array;
use send_wrapper::SendWrapper;
use wasm_bindgen::JsCast;
use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};

use crate::file_io::{sanitize_name, AnySink, MemSink, SavedFile, Sink, SinkError, Source};
use crate::node::SinkPref;
use crate::protocol::FileMeta;

/// Largest file the memory route will accept (design §4.4.3(3)).
pub const MEM_SINK_CAP: usize = 256 * 1024 * 1024;
/// What the user is told when no route can take a file this size. It lives beside the cap it
/// explains, so the limit and its wording cannot drift apart.
pub const MEM_SINK_TOO_BIG: &str =
    "this browser cannot stream downloads; use Chrome or a smaller file";

const NOT_YET: &str = "not yet implemented";

/// Reads slices of a picked `File` on demand — the whole-file `FileReader` read is gone.
pub struct WebFileSource {
    file: SendWrapper<web_sys::File>,
    size: u64,
}

impl WebFileSource {
    pub fn new(file: web_sys::File) -> Self {
        let size = file.size() as u64;
        Self {
            file: SendWrapper::new(file),
            size,
        }
    }
}

impl Source for WebFileSource {
    fn size(&self) -> u64 {
        self.size
    }

    fn read(&mut self, offset: u64, len: usize) -> impl Future<Output = io::Result<Bytes>> {
        let _ = (&*self.file, offset, len);
        async move { Err(io::Error::other(NOT_YET)) }
    }
}

/// File System Access sink (Chromium): streams straight to the location the user picked.
pub struct FsaSink {
    written: u64,
}

impl Sink for FsaSink {
    fn bytes_written(&self) -> u64 {
        self.written
    }

    fn write(&mut self, data: &[u8]) -> impl Future<Output = io::Result<()>> {
        let _ = (self.written, data);
        async move { Err(io::Error::other(NOT_YET)) }
    }

    async fn finish(self) -> io::Result<SavedFile> {
        Err(io::Error::other(NOT_YET))
    }

    async fn abort(self) {}
}

/// Service-worker sink: posts chunks to a worker that serves them as a streamed download.
pub struct SwSink {
    written: u64,
}

impl Sink for SwSink {
    fn bytes_written(&self) -> u64 {
        self.written
    }

    fn write(&mut self, data: &[u8]) -> impl Future<Output = io::Result<()>> {
        let _ = (self.written, data);
        async move { Err(io::Error::other(NOT_YET)) }
    }

    async fn finish(self) -> io::Result<SavedFile> {
        Err(io::Error::other(NOT_YET))
    }

    async fn abort(self) {}
}

/// Build one sink per manifest entry, trying the routes of design §4.4.3 in order.
///
/// Must be the first await of the Save click's task: the pickers need transient user
/// activation, which survives that first await. A `pref` other than `Auto` starts the chain at
/// that sink and does not fall back past it, which is how QA drives a specific route.
pub async fn pick_sinks(manifest: &[FileMeta], pref: SinkPref) -> Result<Vec<AnySink>, SinkError> {
    match pref {
        SinkPref::Fsa => Err(SinkError::Unsupported(format!(
            "the file picker route is {NOT_YET}"
        ))),
        SinkPref::Sw => Err(SinkError::Unsupported(format!(
            "the streaming download route is {NOT_YET}"
        ))),
        SinkPref::Mem | SinkPref::Auto => mem_sinks(manifest),
    }
}

fn mem_sinks(manifest: &[FileMeta]) -> Result<Vec<AnySink>, SinkError> {
    manifest
        .iter()
        .map(|meta| {
            if meta.size > MEM_SINK_CAP as u64 {
                Err(SinkError::Unsupported(MEM_SINK_TOO_BIG.to_string()))
            } else {
                Ok(AnySink::Mem(MemSink::new(
                    sanitize_name(&meta.name),
                    MEM_SINK_CAP,
                )))
            }
        })
        .collect()
}

/// Hand `data` to the browser as a download named `name` (the memory route's last hop).
pub fn trigger_download(name: &str, data: &[u8]) -> Result<(), SinkError> {
    // Note for the step that finishes this route: copying into a fresh `Uint8Array` and then
    // into the `Blob` puts roughly three copies of the file in memory at once (the sink's
    // buffer, the array, the blob). `Uint8Array::view` would drop one of them, but it is
    // `unsafe` and only sound while nothing can grow wasm linear memory in between, so it
    // wants its own review rather than riding along with the interface commit.
    let js_err = |e: wasm_bindgen::JsValue| SinkError::Js(format!("{e:?}"));

    let window = web_sys::window().ok_or_else(|| SinkError::Js("no window".to_string()))?;
    let document = window
        .document()
        .ok_or_else(|| SinkError::Js("no document".to_string()))?;

    let array = Uint8Array::new_with_length(data.len() as u32);
    array.copy_from(data);
    let parts = js_sys::Array::new();
    parts.push(&array);
    let props = BlobPropertyBag::new();
    props.set_type("application/octet-stream");
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &props).map_err(js_err)?;

    let url = Url::create_object_url_with_blob(&blob).map_err(js_err)?;
    let result = (|| {
        let anchor: HtmlAnchorElement = document
            .create_element("a")
            .map_err(js_err)?
            .dyn_into()
            .map_err(|_| SinkError::Js("anchor cast failed".to_string()))?;
        anchor.set_href(&url);
        anchor.set_download(name);
        anchor.click();
        Ok(())
    })();
    let _ = Url::revoke_object_url(&url);
    result
}
