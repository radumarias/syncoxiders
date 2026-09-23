//! Browser sources and sinks (design §4.4.2–§4.4.3), wasm32 only.
//!
//! Signatures are frozen here so the app lane can wire the Save flow against them. Only the
//! memory route is live at this step: [`pick_sinks`] already serves `SinkPref::Mem` — and the
//! `Auto` fallback, since the picker and service-worker branches report `Unsupported` until
//! their bodies land — so a browser receive completes end to end through [`trigger_download`].

use std::future::Future;
use std::io;

use bytes::Bytes;
use js_sys::{Array, Promise, Reflect, Uint8Array};
use send_wrapper::SendWrapper;
use wasm_bindgen::{prelude::wasm_bindgen, JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};

use crate::file_io::{
    read_length, sanitize_name, AnySink, MemSink, SavedFile, Sink, SinkError, Source,
};
use crate::node::SinkPref;
use crate::protocol::FileMeta;

pub mod resume;

/// Largest file the memory route will accept (design §4.4.3(3)).
pub const MEM_SINK_CAP: usize = 256 * 1024 * 1024;
/// What the user is told when no route can take a file this size. It lives beside the cap it
/// explains, so the limit and its wording cannot drift apart.
pub const MEM_SINK_TOO_BIG: &str =
    "this browser cannot stream this download; choose a supported browser or a smaller file";

const FSA_LOCATION: &str = "Selected browser file";
const SW_LOCATION: &str = "Browser downloads";

#[wasm_bindgen(module = "/assets/download-sinks.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = openFsa)]
    fn open_fsa(names: &JsValue) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = openSw)]
    fn open_sw(name: &str, size: &str) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = writeSink)]
    fn write_sink(sink: &JsValue, bytes: &Uint8Array) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = finishSink)]
    fn finish_sink(sink: &JsValue) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = abortSink)]
    fn abort_sink(sink: &JsValue) -> Result<Promise, JsValue>;
    #[wasm_bindgen(js_name = dropSink)]
    fn drop_sink(sink: &JsValue);
}

fn js_message(value: &JsValue) -> String {
    if let Some(message) = value.dyn_ref::<js_sys::Error>().map(js_sys::Error::message) {
        return message.into();
    }
    format!("{value:?}")
}

fn sink_error(value: JsValue) -> SinkError {
    let code = Reflect::get(&value, &JsValue::from_str("code"))
        .ok()
        .and_then(|value| value.as_string());
    let message = js_message(&value);
    match code.as_deref() {
        Some("Cancelled") => SinkError::Cancelled,
        Some("Unsupported") => SinkError::Unsupported(message),
        _ => SinkError::Js(message),
    }
}

fn io_error(value: JsValue) -> io::Error {
    io::Error::other(js_message(&value))
}

async fn abort_handles(handles: Vec<SendWrapper<JsValue>>) {
    for handle in handles {
        let promise = abort_sink(&handle).ok();
        if let Some(promise) = promise {
            let _ = JsFuture::from(promise).await;
        }
    }
}

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
        let length = read_length(self.size, offset, len);
        let start = offset.min(self.size);
        let end = length.map(|length| start + length as u64);

        // `slice` is synchronous and cheap (it only records a range), so it happens here rather
        // than in the future: the future then owns the `Blob` and borrows nothing from `self`.
        // f64 offsets, not i32, because these files run past 2 GiB.
        let slice = end.and_then(|end| {
            self.file
                .slice_with_f64_and_f64(start as f64, end as f64)
                .map_err(|error| io::Error::other(format!("could not slice the file: {error:?}")))
        });

        async move {
            let blob = slice?;
            // The only whole-file buffer is this one slice: at most `len` bytes, which the
            // callers cap at 1 MiB (hashing) or the negotiated chunk budget (sending).
            let buffer = JsFuture::from(blob.array_buffer()).await.map_err(|e| {
                // A `File` whose underlying bytes changed after the user picked it rejects here
                // rather than returning short. The snapshot check in `open_source` catches that
                // between transfers; this is the mid-read case, and it stays a plain read error
                // until the sender's error classification is confirmed for it.
                io::Error::other(format!("could not read the file: {e:?}"))
            })?;
            Ok(Bytes::from(Uint8Array::new(&buffer).to_vec()))
        }
    }
}

/// File System Access sink (Chromium): streams straight to the location the user picked.
pub struct FsaSink {
    handle: Option<SendWrapper<JsValue>>,
    name: String,
    size: u64,
    written: u64,
}

impl Sink for FsaSink {
    fn bytes_written(&self) -> u64 {
        self.written
    }

    async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| io::Error::other("destination is closed"))?;
        let next = self
            .written
            .checked_add(data.len() as u64)
            .filter(|next| *next <= self.size)
            .ok_or_else(|| io::Error::other("download exceeded its declared size"))?;
        let bytes = Uint8Array::new_with_length(
            data.len()
                .try_into()
                .map_err(|_| io::Error::other("download chunk is too large"))?,
        );
        bytes.copy_from(data);
        let promise = write_sink(handle, &bytes).map_err(io_error)?;
        JsFuture::from(promise).await.map_err(io_error)?;
        self.written = next;
        Ok(())
    }

    async fn finish(mut self) -> io::Result<SavedFile> {
        if self.written != self.size {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "download ended before its declared size",
            ));
        }
        let handle = self
            .handle
            .take()
            .ok_or_else(|| io::Error::other("destination is closed"))?;
        let result = finish_sink(&handle).map_err(io_error).map(JsFuture::from);
        match result {
            Ok(result) => {
                if let Err(error) = result.await {
                    drop_sink(&handle);
                    return Err(io_error(error));
                }
            }
            Err(error) => {
                drop_sink(&handle);
                return Err(error);
            }
        }
        Ok(SavedFile {
            name: self.name.clone(),
            size: self.written,
            location: FSA_LOCATION.to_string(),
        })
    }

    async fn abort(mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        if let Ok(promise) = abort_sink(&handle) {
            let _ = JsFuture::from(promise).await;
        }
    }
}

impl Drop for FsaSink {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            drop_sink(&handle);
        }
    }
}

/// Service-worker sink: posts chunks to a worker that serves them as a streamed download.
pub struct SwSink {
    handle: Option<SendWrapper<JsValue>>,
    name: String,
    size: u64,
    written: u64,
}

impl Sink for SwSink {
    fn bytes_written(&self) -> u64 {
        self.written
    }

    async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        let handle = self
            .handle
            .as_ref()
            .ok_or_else(|| io::Error::other("download stream is closed"))?;
        let next = self
            .written
            .checked_add(data.len() as u64)
            .filter(|next| *next <= self.size)
            .ok_or_else(|| io::Error::other("download exceeded its declared size"))?;
        let bytes = Uint8Array::new_with_length(
            data.len()
                .try_into()
                .map_err(|_| io::Error::other("download chunk is too large"))?,
        );
        bytes.copy_from(data);
        let promise = write_sink(handle, &bytes).map_err(io_error)?;
        JsFuture::from(promise).await.map_err(io_error)?;
        self.written = next;
        Ok(())
    }

    async fn finish(mut self) -> io::Result<SavedFile> {
        if self.written != self.size {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "download ended before its declared size",
            ));
        }
        let handle = self
            .handle
            .take()
            .ok_or_else(|| io::Error::other("download stream is closed"))?;
        let result = finish_sink(&handle).map_err(io_error).map(JsFuture::from);
        match result {
            Ok(result) => {
                if let Err(error) = result.await {
                    drop_sink(&handle);
                    return Err(io_error(error));
                }
            }
            Err(error) => {
                drop_sink(&handle);
                return Err(error);
            }
        }
        Ok(SavedFile {
            name: self.name.clone(),
            size: self.written,
            location: SW_LOCATION.to_string(),
        })
    }

    async fn abort(mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        if let Ok(promise) = abort_sink(&handle) {
            let _ = JsFuture::from(promise).await;
        }
    }
}

impl Drop for SwSink {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            drop_sink(&handle);
        }
    }
}

/// Build one sink per manifest entry, trying the routes of design §4.4.3 in order.
///
/// Invoke promptly after the Save gesture, without preceding asynchronous work. A `pref` other
/// than `Auto` selects exactly that sink and does not fall back, which is how QA drives routes.
pub async fn pick_sinks(manifest: &[FileMeta], pref: SinkPref) -> Result<Vec<AnySink>, SinkError> {
    match pref {
        SinkPref::Fsa => fsa_sinks(manifest).await,
        SinkPref::Sw => sw_sinks(manifest).await,
        SinkPref::Mem => mem_sinks(manifest),
        SinkPref::Auto => match fsa_sinks(manifest).await {
            Ok(sinks) => Ok(sinks),
            Err(SinkError::Unsupported(_)) => match sw_sinks(manifest).await {
                Ok(sinks) => Ok(sinks),
                Err(SinkError::Unsupported(_)) => mem_sinks(manifest),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        },
    }
}

async fn fsa_sinks(manifest: &[FileMeta]) -> Result<Vec<AnySink>, SinkError> {
    let names = Array::new();
    for meta in manifest {
        names.push(&JsValue::from_str(&sanitize_name(&meta.name)));
    }
    let promise = open_fsa(&names).map_err(sink_error)?;
    let value = JsFuture::from(promise).await.map_err(sink_error)?;
    if !Array::is_array(&value) {
        return Err(SinkError::Js(
            "save picker returned an invalid result".to_string(),
        ));
    }
    let handles = Array::from(&value);
    if handles.length() as usize != manifest.len() {
        let handles = handles
            .iter()
            .map(SendWrapper::new)
            .collect::<Vec<SendWrapper<JsValue>>>();
        abort_handles(handles).await;
        return Err(SinkError::Js(
            "save picker returned the wrong number of destinations".into(),
        ));
    }
    Ok(handles
        .iter()
        .zip(manifest)
        .map(|(handle, meta)| {
            AnySink::Fsa(FsaSink {
                handle: Some(SendWrapper::new(handle)),
                name: sanitize_name(&meta.name),
                size: meta.size,
                written: 0,
            })
        })
        .collect())
}

async fn sw_sinks(manifest: &[FileMeta]) -> Result<Vec<AnySink>, SinkError> {
    let mut sinks = Vec::with_capacity(manifest.len());
    for meta in manifest {
        let name = sanitize_name(&meta.name);
        let handle = match open_sw(&name, &meta.size.to_string()) {
            Ok(promise) => JsFuture::from(promise).await.map_err(sink_error),
            Err(error) => Err(sink_error(error)),
        };
        let handle = match handle {
            Ok(handle) => handle,
            Err(error) => {
                for sink in sinks {
                    Sink::abort(sink).await;
                }
                return Err(error);
            }
        };
        sinks.push(AnySink::Sw(SwSink {
            handle: Some(SendWrapper::new(handle)),
            name,
            size: meta.size,
            written: 0,
        }));
    }
    Ok(sinks)
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
