//! Durable browser staging in OPFS.
//!
//! The dedicated worker owns every synchronous access handle and checkpoints only after a
//! write has been flushed. This Rust layer reconstructs BLAKE3 state in bounded reads before
//! exposing a sink to the transfer engine.

use std::io;

use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::{prelude::wasm_bindgen, JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::file_io::{sanitize_name, AnySink, SavedFile, Sink, SinkError};
use crate::protocol::FileMeta;
use crate::transfer::ResumeFile;

const REHASH_CHUNK: u32 = 1024 * 1024;
const LOCATION: &str = "Resumable copy in this browser";
const WORKER_SOURCE: &str = include_str!("../../../assets/resume-worker.js");

#[wasm_bindgen(module = "/assets/resume-store.js")]
extern "C" {
    #[wasm_bindgen(js_name = configureResumeWorker)]
    fn configure_resume_worker(source: &str);
    #[wasm_bindgen(catch, js_name = prepareResume)]
    fn prepare_resume(id: &str, manifest: &JsValue) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = readResume)]
    fn read_resume(id: &str, index: u32, offset: &str, len: u32) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = writeResume)]
    fn write_resume(id: &str, index: u32, bytes: &Uint8Array) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = finishResume)]
    fn finish_resume(id: &str, index: u32) -> Result<Promise, JsValue>;
    #[wasm_bindgen(js_name = releaseResume)]
    fn release_resume(id: &str, index: u32);
    #[wasm_bindgen(catch, js_name = listResumes)]
    fn list_resumes() -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = discardResume)]
    fn discard_resume(id: &str) -> Result<Promise, JsValue>;
    #[wasm_bindgen(catch, js_name = exportResume)]
    fn export_resume(id: &str, index: u32) -> Result<Promise, JsValue>;
}

fn configure() {
    configure_resume_worker(WORKER_SOURCE);
}

fn message(value: JsValue) -> String {
    value
        .dyn_ref::<js_sys::Error>()
        .map(js_sys::Error::message)
        .map(Into::into)
        .or_else(|| value.as_string())
        .unwrap_or_else(|| format!("{value:?}"))
}

fn sink_error(value: JsValue) -> SinkError {
    SinkError::Js(message(value))
}

fn io_error(value: JsValue) -> io::Error {
    io::Error::other(message(value))
}

fn string_field(value: &JsValue, name: &str) -> Result<String, SinkError> {
    Reflect::get(value, &JsValue::from_str(name))
        .map_err(sink_error)?
        .as_string()
        .ok_or_else(|| SinkError::Js(format!("resume metadata is missing {name}")))
}

fn bool_field(value: &JsValue, name: &str) -> Result<bool, SinkError> {
    Reflect::get(value, &JsValue::from_str(name))
        .map_err(sink_error)?
        .as_bool()
        .ok_or_else(|| SinkError::Js(format!("resume metadata is missing {name}")))
}

fn u64_field(value: &JsValue, name: &str) -> Result<u64, SinkError> {
    string_field(value, name)?
        .parse()
        .map_err(|_| SinkError::Js(format!("resume metadata has an invalid {name}")))
}

fn manifest_value(manifest: &[FileMeta]) -> Result<JsValue, SinkError> {
    let files = Array::new();
    for meta in manifest {
        let file = Object::new();
        Reflect::set(
            &file,
            &JsValue::from_str("name"),
            &JsValue::from_str(&sanitize_name(&meta.name)),
        )
        .map_err(sink_error)?;
        Reflect::set(
            &file,
            &JsValue::from_str("size"),
            &JsValue::from_str(&meta.size.to_string()),
        )
        .map_err(sink_error)?;
        Reflect::set(
            &file,
            &JsValue::from_str("hash"),
            &JsValue::from_str(&meta.hash.to_hex()),
        )
        .map_err(sink_error)?;
        files.push(&file);
    }
    Ok(files.into())
}

/// Stable identity for exact names, sizes, hashes and ordering. It intentionally contains no
/// ticket or capability and is safe to persist.
pub fn manifest_id(manifest: &[FileMeta]) -> Result<String, SinkError> {
    let mut hasher = blake3::Hasher::new_derive_key("oxfer resumable manifest v1");
    for meta in manifest {
        let name = sanitize_name(&meta.name);
        hasher.update(&(name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
        hasher.update(&meta.size.to_le_bytes());
        hasher.update(&meta.hash.0);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Open or create durable staging and reconstruct every prefix hash with bounded reads.
pub async fn prepare(manifest: &[FileMeta]) -> Result<Vec<ResumeFile>, SinkError> {
    configure();
    let id = manifest_id(manifest)?;
    let files = manifest_value(manifest)?;
    let descriptors = prepare_resume(&id, &files).map_err(sink_error)?;
    let descriptors = JsFuture::from(descriptors).await.map_err(sink_error)?;
    let mut opened = PreparedHandles {
        id: id.clone(),
        count: manifest.len(),
        armed: true,
    };
    if !Array::is_array(&descriptors) {
        return Err(SinkError::Js(
            "resume worker returned invalid metadata".to_string(),
        ));
    }
    let descriptors = Array::from(&descriptors);
    if descriptors.length() as usize != manifest.len() {
        return Err(SinkError::Js(
            "resume worker returned the wrong number of files".to_string(),
        ));
    }

    let mut result = Vec::with_capacity(manifest.len());
    for (index, (descriptor, meta)) in descriptors.iter().zip(manifest).enumerate() {
        let written = u64_field(&descriptor, "written")?;
        if written > meta.size {
            return Err(SinkError::Js(
                "saved progress exceeds the declared file size".to_string(),
            ));
        }
        let mut hasher = blake3::Hasher::new();
        let mut offset = 0;
        while offset < written {
            let len = (written - offset).min(REHASH_CHUNK as u64) as u32;
            let bytes =
                read_resume(&id, index as u32, &offset.to_string(), len).map_err(sink_error)?;
            let bytes = JsFuture::from(bytes).await.map_err(sink_error)?;
            let bytes = Uint8Array::new(&bytes).to_vec();
            if bytes.len() != len as usize {
                return Err(SinkError::Js(
                    "saved progress ended before its checkpoint".to_string(),
                ));
            }
            hasher.update(&bytes);
            offset += bytes.len() as u64;
        }
        result.push(ResumeFile {
            meta: meta.clone(),
            sink: AnySink::Persistent(PersistentSink {
                id: id.clone(),
                index: index as u32,
                name: sanitize_name(&meta.name),
                size: meta.size,
                written,
                released: false,
            }),
            hasher,
        });
    }
    opened.armed = false;
    Ok(result)
}

struct PreparedHandles {
    id: String,
    count: usize,
    armed: bool,
}

impl Drop for PreparedHandles {
    fn drop(&mut self) {
        if self.armed {
            for index in 0..self.count {
                if let Ok(index) = index.try_into() {
                    release_resume(&self.id, index);
                }
            }
        }
    }
}

pub struct PersistentSink {
    id: String,
    index: u32,
    name: String,
    size: u64,
    written: u64,
    released: bool,
}

impl Sink for PersistentSink {
    fn bytes_written(&self) -> u64 {
        self.written
    }

    async fn write(&mut self, data: &[u8]) -> io::Result<()> {
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
        let promise = write_resume(&self.id, self.index, &bytes).map_err(io_error)?;
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
        let promise = finish_resume(&self.id, self.index).map_err(io_error)?;
        JsFuture::from(promise).await.map_err(io_error)?;
        self.released = true;
        Ok(SavedFile {
            name: self.name.clone(),
            size: self.size,
            location: LOCATION.to_string(),
        })
    }

    async fn abort(mut self) {
        release_resume(&self.id, self.index);
        self.released = true;
    }
}

impl Drop for PersistentSink {
    fn drop(&mut self) {
        if !self.released {
            release_resume(&self.id, self.index);
        }
    }
}

#[derive(Clone)]
pub struct StoredTransfer {
    pub id: String,
    pub files: Vec<StoredFile>,
}

#[derive(Clone)]
pub struct StoredFile {
    pub name: String,
    pub size: u64,
    pub written: u64,
    pub verified: bool,
}

pub async fn list() -> Result<Vec<StoredTransfer>, SinkError> {
    configure();
    let value = list_resumes().map_err(sink_error)?;
    let value = JsFuture::from(value).await.map_err(sink_error)?;
    if !Array::is_array(&value) {
        return Err(SinkError::Js(
            "resume worker returned an invalid library".to_string(),
        ));
    }
    Array::from(&value)
        .iter()
        .map(|transfer| {
            let files = Reflect::get(&transfer, &JsValue::from_str("files")).map_err(sink_error)?;
            if !Array::is_array(&files) {
                return Err(SinkError::Js(
                    "resume worker returned invalid file metadata".to_string(),
                ));
            }
            let files = Array::from(&files)
                .iter()
                .map(|file| {
                    Ok(StoredFile {
                        name: string_field(&file, "name")?,
                        size: u64_field(&file, "size")?,
                        written: u64_field(&file, "written")?,
                        verified: bool_field(&file, "verified")?,
                    })
                })
                .collect::<Result<Vec<_>, SinkError>>()?;
            Ok(StoredTransfer {
                id: string_field(&transfer, "id")?,
                files,
            })
        })
        .collect()
}

pub async fn discard(id: &str) -> Result<(), SinkError> {
    configure();
    let promise = discard_resume(id).map_err(sink_error)?;
    JsFuture::from(promise).await.map_err(sink_error)?;
    Ok(())
}

pub async fn export(id: &str, index: usize) -> Result<(), SinkError> {
    configure();
    let index = index
        .try_into()
        .map_err(|_| SinkError::Js("file index is too large".to_string()))?;
    let promise = export_resume(id, index).map_err(sink_error)?;
    JsFuture::from(promise).await.map_err(sink_error)?;
    Ok(())
}
