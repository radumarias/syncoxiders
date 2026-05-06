use eframe::egui;
use egui::{Button, Color32, CornerRadius, Grid, Label, RichText, Stroke, TextStyle, Ui, Vec2};
use serde::{Deserialize, Serialize};
use crate::node::EchoNode;
use iroh::NodeId;

// ── Stitch Design System — theme-aware color palette ──────────────────────────

#[derive(Clone, Copy)]
struct Tc {
    bg:             Color32,
    surface_lowest: Color32,
    surface_low:    Color32,
    surface:        Color32,
    surface_high:   Color32,
    primary:        Color32,
    on_primary:     Color32,
    secondary:      Color32,
    on_secondary:   Color32,
    on_surface:     Color32,
    on_surface_var: Color32,
    outline:        Color32,
    outline_var:    Color32,
    error:          Color32,
}

impl Tc {
    const fn dark() -> Self {
        Self {
            bg:             Color32::from_rgb(19,  19,  27),   // #13131b
            surface_lowest: Color32::from_rgb(13,  13,  21),   // #0d0d15
            surface_low:    Color32::from_rgb(27,  27,  35),   // #1b1b23
            surface:        Color32::from_rgb(31,  31,  39),   // #1f1f27
            surface_high:   Color32::from_rgb(41,  41,  50),   // #292932
            primary:        Color32::from_rgb(192, 193, 255),  // #c0c1ff
            on_primary:     Color32::from_rgb(16,  0,   169),  // #1000a9
            secondary:      Color32::from_rgb(78,  222, 163),  // #4edea3
            on_secondary:   Color32::from_rgb(0,   56,  36),
            on_surface:     Color32::from_rgb(228, 225, 237),  // #e4e1ed
            on_surface_var: Color32::from_rgb(199, 196, 215),  // #c7c4d7
            outline:        Color32::from_rgb(144, 143, 160),  // #908fa0
            outline_var:    Color32::from_rgb(70,  69,  84),   // #464554
            error:          Color32::from_rgb(255, 180, 171),  // #ffb4ab
        }
    }

    const fn light() -> Self {
        Self {
            bg:             Color32::from_rgb(244, 243, 255),  // #f4f3ff
            surface_lowest: Color32::from_rgb(255, 255, 255),  // #ffffff
            surface_low:    Color32::from_rgb(238, 237, 255),  // #eeedff
            surface:        Color32::from_rgb(230, 229, 249),  // #e6e5f9
            surface_high:   Color32::from_rgb(216, 215, 235),  // #d8d7eb
            primary:        Color32::from_rgb(65,  67,  199),  // #4143c7
            on_primary:     Color32::from_rgb(255, 255, 255),  // #ffffff
            secondary:      Color32::from_rgb(0,   108, 74),   // #006c4a
            on_secondary:   Color32::from_rgb(255, 255, 255),
            on_surface:     Color32::from_rgb(27,  26,  39),   // #1b1a27
            on_surface_var: Color32::from_rgb(71,  69,  85),   // #474555
            outline:        Color32::from_rgb(120, 118, 127),  // #78767f
            outline_var:    Color32::from_rgb(199, 197, 208),  // #c7c5d0
            error:          Color32::from_rgb(186, 26,  26),   // #ba1a1a
        }
    }

    fn for_ui(ui: &Ui) -> Self {
        if ui.visuals().dark_mode { Self::dark() } else { Self::light() }
    }
}

#[derive(Debug, Clone)]
pub struct ReceivedFile {
    pub name: String,
    pub size: u64,
    pub saved_path: String,
    pub timestamp: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TorrentInfo{
    pub magnet_uri : Option<String>,
    pub download_progress: f32,
    pub peers_count: usize,
    pub is_download: bool,
    pub is_seeding: bool,
    pub download_complete: bool
}

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct P2PTransfer {
    #[serde(skip)]
    value: f32,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    file_input_closure: Option<wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>>,
    #[serde(skip)]
    picked_file_name: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    #[serde(skip)]
    picked_file_path: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    #[serde(skip)]
    picked_file_size: std::sync::Arc<std::sync::Mutex<Option<u64>>>,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    picked_file_data: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>,
    #[serde(skip)]
    torrent_info: std::sync::Arc<std::sync::Mutex<TorrentInfo>>,
    #[serde(skip)]
    magnet_input: String,
    #[serde(skip)]
    node: std::sync::Arc<std::sync::Mutex<Option<EchoNode>>>,
    #[serde(skip)]
    node_id: Option<NodeId>,
    #[serde(skip)]
    is_accepting: bool,
    #[serde(skip)]
    connect_command: String,
    #[serde(skip)]
    shared_node_id: std::sync::Arc<std::sync::Mutex<Option<NodeId>>>,
    #[serde(skip)]
    is_receiving: std::sync::Arc<std::sync::Mutex<bool>>,
    #[serde(skip)]
    show_receive_dialog: bool,
    #[serde(skip)]
    receive_hash_input: String,
    #[serde(skip)]
    receive_status: std::sync::Arc<std::sync::Mutex<String>>,
    #[serde(skip)]
    terminal_logs: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    #[serde(skip)]
    show_terminal_view: bool,
    #[serde(skip)]
    received_files: std::sync::Arc<std::sync::Mutex<Vec<ReceivedFile>>>,
    #[serde(skip)]
    shared_files: std::sync::Arc<std::sync::Mutex<Vec<(String, String, u64)>>>, // (name, path, size)
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    shared_files_data: std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>, // (name, data) for WASM
    #[serde(skip)]
    save_directory: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    #[serde(skip)]
    shareable_url: std::sync::Arc<std::sync::Mutex<Option<String>>>,

}

impl Default for P2PTransfer {
    fn default() -> Self {
        Self {
            value: 0.0,
            #[cfg(target_arch = "wasm32")]
            file_input_closure: None,
            picked_file_name: std::sync::Arc::new(std::sync::Mutex::new(None)),
            picked_file_path: std::sync::Arc::new(std::sync::Mutex::new(None)),
            picked_file_size: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(target_arch = "wasm32")]
            picked_file_data: std::sync::Arc::new(std::sync::Mutex::new(None)),
            torrent_info: std::sync::Arc::new(std::sync::Mutex::new(TorrentInfo::default())),
            magnet_input: String::new(),
            node: std::sync::Arc::new(std::sync::Mutex::new(None)),
            node_id: None,
            is_accepting: false,
            connect_command: String::new(),
            shared_node_id: std::sync::Arc::new(std::sync::Mutex::new(None)),
            is_receiving: std::sync::Arc::new(std::sync::Mutex::new(false)),
            show_receive_dialog: false,
            receive_hash_input: String::new(),
            receive_status: std::sync::Arc::new(std::sync::Mutex::new(String::new())),
            terminal_logs: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            show_terminal_view: false,
            received_files: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            shared_files: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            #[cfg(target_arch = "wasm32")]
            shared_files_data: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            save_directory: std::sync::Arc::new(std::sync::Mutex::new(None)),
            shareable_url: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

impl P2PTransfer {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load previous app state (if any)
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }
        Default::default()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn pick_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let file_path = path.display().to_string();
            let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

            if let Ok(mut filename) = self.picked_file_name.lock() {
                *filename = Some(file_name);
            }
            if let Ok(mut filepath) = self.picked_file_path.lock() {
                *filepath = Some(file_path);
            }
            if let Ok(mut filesize) = self.picked_file_size.lock() {
                *filesize = Some(file_size);
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn pick_file(&mut self, ctx: &egui::Context) {
        use wasm_bindgen::JsCast;
        use web_sys::{Event, HtmlInputElement, FileReader};
        use wasm_bindgen_futures::JsFuture;

        self.file_input_closure = None;

        let window = match web_sys::window() {
            Some(w) => w,
            None => {
                web_sys::console::error_1(&"Failed to get window object".into());
                return;
            }
        };

        let document = match window.document() {
            Some(d) => d,
            None => {
                web_sys::console::error_1(&"Failed to get document object".into());
                return;
            }
        };

        let input_elem = match document.create_element("input") {
            Ok(e) => e,
            Err(_e) => {
                web_sys::console::error_1(&format!("Failed to create input element: {:?}", _e).into());
                return;
            }
        };

        let input: HtmlInputElement = match input_elem.dyn_into() {
            Ok(i) => i,
            Err(_e) => {
                web_sys::console::error_1(&format!("Failed to cast to HtmlInputElement: {:?}", _e).into());
                return;
            }
        };

        if let Err(_e) = input.set_attribute("type", "file") {
            web_sys::console::error_1(&format!("Failed to set input type: {:?}", _e).into());
            return;
        }

        let ctx_clone = ctx.clone();
        let shared_filename = self.picked_file_name.clone();
        let shared_filepath = self.picked_file_path.clone();
        let shared_filesize = self.picked_file_size.clone();
        let shared_filedata = self.picked_file_data.clone();

        let closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: Event| {
            let target = match event.target() {
                Some(t) => t,
                None => {
                    web_sys::console::error_1(&"Event target is None".into());
                    return;
                }
            };

            let input: HtmlInputElement = match target.dyn_into() {
                Ok(i) => i,
                Err(_e) => {
                    web_sys::console::error_1(&format!("Failed to cast event target: {:?}", _e).into());
                    return;
                }
            };

            if let Some(files) = input.files() {
                if let Some(file) = files.get(0) {
                    let name = file.name();
                    let size = file.size() as u64;
                    let path = name.clone();

                    web_sys::console::log_1(&format!("Picked file: {} ({} bytes)", name, size).into());

                    // Read file data
                    let reader = match FileReader::new() {
                        Ok(r) => r,
                        Err(_e) => {
                            web_sys::console::error_1(&format!("Failed to create FileReader: {:?}", _e).into());
                            return;
                        }
                    };
                    let reader_clone = reader.clone();
                    let name_clone = name.clone();
                    let ctx_clone2 = ctx_clone.clone();
                    let shared_filename2 = shared_filename.clone();
                    let shared_filepath2 = shared_filepath.clone();
                    let shared_filesize2 = shared_filesize.clone();
                    let shared_filedata2 = shared_filedata.clone();

                    let onload = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: Event| {
                        if let Ok(result) = reader_clone.result() {
                            if let Some(array_buffer) = result.dyn_ref::<js_sys::ArrayBuffer>() {
                                let uint8_array = js_sys::Uint8Array::new(array_buffer);
                                let data: Vec<u8> = uint8_array.to_vec();

                                web_sys::console::log_1(&format!("File data read: {} bytes", data.len()).into());

                                // Update shared states with error logging
                                match shared_filename2.lock() {
                                    Ok(mut filename) => *filename = Some(name_clone.clone()),
                                    Err(_e) => web_sys::console::error_1(&format!("Failed to lock filename: {:?}", _e).into()),
                                }
                                match shared_filepath2.lock() {
                                    Ok(mut filepath) => *filepath = Some(name_clone.clone()),
                                    Err(_e) => web_sys::console::error_1(&format!("Failed to lock filepath: {:?}", _e).into()),
                                }
                                match shared_filesize2.lock() {
                                    Ok(mut filesize) => *filesize = Some(data.len() as u64),
                                    Err(_e) => web_sys::console::error_1(&format!("Failed to lock filesize: {:?}", _e).into()),
                                }
                                match shared_filedata2.lock() {
                                    Ok(mut filedata) => *filedata = Some(data),
                                    Err(_e) => web_sys::console::error_1(&format!("Failed to lock filedata: {:?}", _e).into()),
                                }

                                ctx_clone2.request_repaint();
                            }
                        }
                    }) as Box<dyn FnMut(_)>);

                    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
                    onload.forget();

                    let _ = reader.read_as_array_buffer(&file);
                }
            }
        }) as Box<dyn FnMut(_)>);

        input.set_onchange(Some(closure.as_ref().unchecked_ref()));
        self.file_input_closure = Some(closure);
        input.click();
    }

    fn format_size(&self, size_bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * KB;
        const GB: u64 = 1024 * MB;

        if size_bytes < KB {
            format!("{} bytes", size_bytes)
        } else if size_bytes < MB {
            format!("{:.2} KB", size_bytes as f64 / KB as f64)
        } else if size_bytes < GB {
            format!("{:.2} MB", size_bytes as f64 / MB as f64)
        } else {
            format!("{:.2} GB", size_bytes as f64 / GB as f64)
        }
    }

    fn generate_shareable_url(&self, node_id: NodeId) -> String {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(window) = web_sys::window() {
                if let Ok(location) = window.location().href() {
                    // Remove existing hash if present
                    let base_url = location.split('#').next().unwrap_or(&location);
                    return format!("{}#{}", base_url, node_id);
                }
            }
            // Fallback
            format!("https://yourapp.com/#{}", node_id)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            format!("https://syncoxiders.app/#{}", node_id)
        }
    }

    fn parse_node_id_from_url(&self) -> Option<NodeId> {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(window) = web_sys::window() {
                if let Ok(hash) = window.location().hash() {
                    if hash.starts_with('#') {
                        let node_id_str = &hash[1..];
                        return node_id_str.parse::<NodeId>().ok();
                    }
                }
            }
        }
        None
    }

    fn extract_node_id(&self, input: &str) -> Result<NodeId, String> {
        if let Ok(node_id) = input.parse::<NodeId>() {
            return Ok(node_id);
        }

        if input.contains('#') {
            if let Some(hash_part) = input.split('#').nth(1) {
                if let Ok(node_id) = hash_part.parse::<NodeId>() {
                    return Ok(node_id);
                }
            }
        }

        Err("Invalid node ID or URL format".to_string())
    }

    fn start_accepting(&mut self, ctx: &egui::Context) {
        if self.is_accepting {
            return;
        }

        self.is_accepting = true;

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen_futures::spawn_local;

            let ctx_clone = ctx.clone();
            let node_id_shared = self.shared_node_id.clone();
            let node_shared = self.node.clone();
            let logs_shared = self.terminal_logs.clone();
            let shared_files_data = self.shared_files_data.clone();
            let shareable_url_shared = self.shareable_url.clone();

            spawn_local(async move {
                // Read all files from the shared_files_data list
                let files_to_share: Vec<(String, Vec<u8>)> = match shared_files_data.lock() {
                    Ok(files_data) => files_data.clone(),
                    Err(_e) => {
                        web_sys::console::error_1(&format!("⚠️ Failed to lock shared_files_data: {:?}. Starting with no files.", _e).into());
                        Vec::new()
                    }
                };

                if files_to_share.is_empty() {
                    web_sys::console::log_1(&"⚠️ No files added to share. Please add files first.".into());
                } else {
                    web_sys::console::log_1(&format!("📦 Sharing {} file(s)", files_to_share.len()).into());
                }

                match EchoNode::spawn_with_files(files_to_share).await {
                    Ok(node) => {
                        let node_id = node.endpoint().node_id();
                        let log_msg = format!("🚀 Node spawned with ID: {}", node_id);
                        web_sys::console::log_1(&log_msg.into());

                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(log_msg);
                        }

                        if let Ok(mut nid) = node_id_shared.lock() {
                            *nid = Some(node_id);
                        }

                        // Generate shareable URL
                        if let Some(window) = web_sys::window() {
                            if let Ok(location) = window.location().href() {
                                let base_url = location.split('#').next().unwrap_or(&location);
                                let share_url = format!("{}#{}", base_url, node_id);
                                if let Ok(mut url) = shareable_url_shared.lock() {
                                    *url = Some(share_url.clone());
                                }
                                let log_msg = format!("🔗 Shareable URL: {}", share_url);
                                web_sys::console::log_1(&log_msg.into());
                                if let Ok(mut logs) = logs_shared.lock() {
                                    logs.push(log_msg);
                                }
                            }
                        }

                        // Subscribe to accept events for sender-side logging
                        let mut accept_events = node.subscribe_accept_events();
                        let logs_for_events = logs_shared.clone();
                        let ctx_for_events = ctx_clone.clone();

                        spawn_local(async move {
                            while let Ok(event) = accept_events.recv().await {
                                match event {
                                    crate::node::AcceptEvent::Accepted { node_id } => {
                                        let log_msg = format!("📥 Incoming connection from: {}", node_id);
                                        web_sys::console::log_1(&log_msg.into());
                                        if let Ok(mut logs) = logs_for_events.lock() {
                                            logs.push(log_msg);
                                        }
                                        ctx_for_events.request_repaint();
                                    }
                                    crate::node::AcceptEvent::Echoed { node_id, bytes_sent } => {
                                        let log_msg = format!("✅ Transfer complete to {} ({} bytes, {:.2} MB)",
                                            node_id, bytes_sent, bytes_sent as f64 / 1024.0 / 1024.0);
                                        web_sys::console::log_1(&log_msg.into());
                                        if let Ok(mut logs) = logs_for_events.lock() {
                                            logs.push(log_msg);
                                        }
                                        ctx_for_events.request_repaint();
                                    }
                                    crate::node::AcceptEvent::Closed { node_id, error } => {
                                        let log_msg = if let Some(err) = error {
                                            format!("❌ Connection closed with error from {}: {}", node_id, err)
                                        } else {
                                            format!("🔒 Connection closed with {}", node_id)
                                        };
                                        web_sys::console::log_1(&log_msg.into());
                                        if let Ok(mut logs) = logs_for_events.lock() {
                                            logs.push(log_msg);
                                        }
                                        ctx_for_events.request_repaint();
                                    }
                                }
                            }
                        });

                        // Store the node to keep it alive
                        if let Ok(mut n) = node_shared.lock() {
                            *n = Some(node);
                        }

                        ctx_clone.request_repaint();
                    }
                    Err(e) => {
                        let log_msg = format!("❌ Failed to spawn node: {}", e);
                        web_sys::console::log_1(&log_msg.into());

                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(log_msg);
                        }
                    }
                }
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let ctx_clone = ctx.clone();
            let node_id_shared = self.shared_node_id.clone();
            let node_shared = self.node.clone();
            let logs_shared = self.terminal_logs.clone();
            let shared_files = self.shared_files.clone();
            let shareable_url_shared = self.shareable_url.clone();

            tokio::spawn(async move {
                // Read all files from the shared_files list
                let files_to_share: Vec<(String, Vec<u8>)> = if let Ok(files) = shared_files.lock() {
                    let mut result = Vec::new();
                    for (name, path, _size) in files.iter() {
                        match std::fs::read(path) {
                            Ok(data) => {
                                let log_msg = format!("Read file: {} ({} bytes)", name, data.len());
                                println!("{}", log_msg);
                                if let Ok(mut logs) = logs_shared.lock() {
                                    logs.push(log_msg);
                                }
                                result.push((name.clone(), data));
                            }
                            Err(e) => {
                                let log_msg = format!("Failed to read file {}: {}", name, e);
                                println!("{}", log_msg);
                                if let Ok(mut logs) = logs_shared.lock() {
                                    logs.push(log_msg);
                                }
                            }
                        }
                    }
                    result
                } else {
                    Vec::new()
                };

                match EchoNode::spawn_with_files(files_to_share).await {
                    Ok(node) => {
                        let node_id = node.endpoint().node_id();
                        let log_msg = format!("🚀 Node spawned with ID: {}", node_id);
                        println!("{}", log_msg);

                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(log_msg);
                        }

                        if let Ok(mut nid) = node_id_shared.lock() {
                            *nid = Some(node_id);
                        }

                        // Generate shareable URL
                        let share_url = format!("https://syncoxiders.app/#{}", node_id);
                        if let Ok(mut url) = shareable_url_shared.lock() {
                            *url = Some(share_url.clone());
                        }
                        let log_msg = format!("🔗 Shareable URL: {}", share_url);
                        println!("{}", log_msg);
                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(log_msg);
                        }

                        // Subscribe to accept events for sender-side logging
                        let mut accept_events = node.subscribe_accept_events();
                        let logs_for_events = logs_shared.clone();
                        let ctx_for_events = ctx_clone.clone();

                        tokio::spawn(async move {
                            while let Ok(event) = accept_events.recv().await {
                                match event {
                                    crate::node::AcceptEvent::Accepted { node_id } => {
                                        let log_msg = format!("📥 Incoming connection from: {}", node_id);
                                        println!("{}", log_msg);
                                        if let Ok(mut logs) = logs_for_events.lock() {
                                            logs.push(log_msg);
                                        }
                                        ctx_for_events.request_repaint();
                                    }
                                    crate::node::AcceptEvent::Echoed { node_id, bytes_sent } => {
                                        let log_msg = format!("✅ Transfer complete to {} ({} bytes, {:.2} MB)",
                                            node_id, bytes_sent, bytes_sent as f64 / 1024.0 / 1024.0);
                                        println!("{}", log_msg);
                                        if let Ok(mut logs) = logs_for_events.lock() {
                                            logs.push(log_msg);
                                        }
                                        ctx_for_events.request_repaint();
                                    }
                                    crate::node::AcceptEvent::Closed { node_id, error } => {
                                        let log_msg = if let Some(err) = error {
                                            format!("❌ Connection closed with error from {}: {}", node_id, err)
                                        } else {
                                            format!("🔒 Connection closed with {}", node_id)
                                        };
                                        println!("{}", log_msg);
                                        if let Ok(mut logs) = logs_for_events.lock() {
                                            logs.push(log_msg);
                                        }
                                        ctx_for_events.request_repaint();
                                    }
                                }
                            }
                        });

                        // Store the node to keep it alive
                        if let Ok(mut n) = node_shared.lock() {
                            *n = Some(node);
                        }

                        ctx_clone.request_repaint();
                    }
                    Err(e) => {
                        let log_msg = format!("❌ Failed to spawn node: {}", e);
                        println!("{}", log_msg);

                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(log_msg);
                        }
                    }
                }
            });
        }
    }

    fn stop_accepting(&mut self) {
        self.is_accepting = false;

        if let Ok(mut node) = self.node.lock() {
            *node = None;
        }

        if let Ok(mut nid) = self.shared_node_id.lock() {
            *nid = None;
        }

        // Clear shareable URL
        if let Ok(mut url) = self.shareable_url.lock() {
            *url = None;
        }

        // Clear shared files list
        if let Ok(mut files) = self.shared_files.lock() {
            files.clear();
        }

        // Clear shared files data to prevent memory accumulation
        #[cfg(target_arch = "wasm32")]
        if let Ok(mut files_data) = self.shared_files_data.lock() {
            files_data.clear();
        }

        if let Ok(mut name) = self.picked_file_name.lock() {
            *name = None;
        }
        if let Ok(mut path) = self.picked_file_path.lock() {
            *path = None;
        }
        if let Ok(mut size) = self.picked_file_size.lock() {
            *size = None;
        }

        #[cfg(target_arch = "wasm32")]
        {
            if let Ok(mut data) = self.picked_file_data.lock() {
                *data = None;
            }
            web_sys::console::log_1(&"Stopped accepting connections".into());
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let log_msg = "⏹ Stopped accepting connections".to_string();
            println!("{}", log_msg);

            if let Ok(mut logs) = self.terminal_logs.lock() {
                logs.push(log_msg);
            }
        }
    }

    fn start_receiving(&mut self, ctx: &egui::Context, target_node_id: NodeId) {
        if let Ok(is_recv) = self.is_receiving.lock() {
            if *is_recv {
                return;
            }
        }

        if let Ok(mut is_recv) = self.is_receiving.lock() {
            *is_recv = true;
        }
        if let Ok(mut status) = self.receive_status.lock() {
            *status = "Connecting...".to_string();
        }

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen_futures::spawn_local;

            let ctx_clone = ctx.clone();
            let node_shared = self.node.clone();
            let status_shared = self.receive_status.clone();
            let is_receiving_shared = self.is_receiving.clone();
            let received_files_shared = self.received_files.clone();

            spawn_local(async move {
                match EchoNode::spawn().await {
                    Ok(node) => {
                        web_sys::console::log_1(&format!("Connecting to node: {}", target_node_id).into());

                        // Get events from connecting
                        let dummy_data = b"SEND_FILE".to_vec();
                        let mut events = node.connect(target_node_id, dummy_data, "request".to_string());

                        // Store the node
                        if let Ok(mut n) = node_shared.lock() {
                            *n = Some(node);
                        }

                        // Store file chunks temporarily
                        let mut current_file: Option<(String, Vec<Vec<u8>>)> = None;

                        // Process connection events
                        use n0_future::StreamExt;
                        while let Some(event) = events.next().await {
                            match event {
                                crate::node::ConnectEvent::Connected => {
                                    web_sys::console::log_1(&"✓ Connected! Waiting for files...".into());
                                    if let Ok(mut status) = status_shared.lock() {
                                        *status = "Connected! Waiting for files...".to_string();
                                    }
                                    ctx_clone.request_repaint();
                                }
                                crate::node::ConnectEvent::Sent { .. } => {}
                                crate::node::ConnectEvent::Transfer(transfer_event) => {
                                    match transfer_event {
                                        crate::node::TransferEvent::FileStart { file_name, file_size, total_chunks, blob_hash } => {
                                            let hash_info = blob_hash.as_ref().map(|h| format!(" hash: {}...", &h[..16])).unwrap_or_default();
                                            web_sys::console::log_1(&format!("📥 Starting file: {} ({} bytes, {} chunks{})", file_name, file_size, total_chunks, hash_info).into());
                                            if let Ok(mut status) = status_shared.lock() {
                                                *status = format!("Receiving: {} (0%)", file_name);
                                            }
                                            current_file = Some((file_name, vec![Vec::new(); total_chunks as usize]));
                                            ctx_clone.request_repaint();
                                        }
                                        crate::node::TransferEvent::ChunkReceived { file_name, chunk_index, chunk_data, offset: _ } => {
                                            if let Some((ref name, ref mut chunks)) = current_file {
                                                if name == &file_name && (chunk_index as usize) < chunks.len() {
                                                    chunks[chunk_index as usize] = chunk_data;
                                                    web_sys::console::log_1(&format!("  ✓ Chunk {} received", chunk_index).into());
                                                }
                                            }
                                            ctx_clone.request_repaint();
                                        }
                                        crate::node::TransferEvent::FileComplete { file_name, total_bytes, hash_verified } => {
                                            let verify_status = match hash_verified {
                                                Some(true) => " ✓ verified",
                                                Some(false) => " ⚠ hash mismatch",
                                                None => "",
                                            };
                                            web_sys::console::log_1(&format!("✅ File complete: {} ({} bytes{})", file_name, total_bytes, verify_status).into());

                                            // Combine all chunks and trigger download
                                            if let Some((name, chunks)) = current_file.take() {
                                                if name == file_name {
                                                    let combined_data: Vec<u8> = chunks.into_iter().flatten().collect();

                                                    // Trigger automatic download in browser
                                                    Self::download_file_wasm(&file_name, &combined_data);

                                                    let timestamp = js_sys::Date::now() as u64 / 1000;
                                                    let received_file = ReceivedFile {
                                                        name: file_name.clone(),
                                                        size: total_bytes,
                                                        saved_path: "Downloaded to browser".to_string(),
                                                        timestamp: format!("{}", timestamp),
                                                    };

                                                    if let Ok(mut files) = received_files_shared.lock() {
                                                        files.push(received_file);
                                                    }

                                                    if let Ok(mut status) = status_shared.lock() {
                                                        *status = format!("File downloaded: {}", file_name);
                                                    }
                                                }
                                            }

                                            ctx_clone.request_repaint();
                                        }
                                    }
                                }
                                crate::node::ConnectEvent::Closed { error } => {
                                    let msg = if let Some(err) = &error {
                                        format!("✗ Connection closed with error: {}", err)
                                    } else {
                                        "✓ Connection closed successfully".to_string()
                                    };
                                    web_sys::console::log_1(&msg.into());

                                    if let Some(err) = error {
                                        if let Ok(mut status) = status_shared.lock() {
                                            *status = format!("Error: {}", err);
                                        }
                                    } else {
                                        if let Ok(mut status) = status_shared.lock() {
                                            *status = "Transfer complete!".to_string();
                                        }
                                    }
                                    ctx_clone.request_repaint();
                                    break;
                                }
                            }
                        }

                        ctx_clone.request_repaint();
                    }
                    Err(e) => {
                        web_sys::console::log_1(&format!("Failed to connect: {}", e).into());
                        if let Ok(mut status) = status_shared.lock() {
                            *status = format!("Connection failed: {}", e);
                        }
                        if let Ok(mut is_recv) = is_receiving_shared.lock() {
                            *is_recv = false;
                        }
                        ctx_clone.request_repaint();
                    }
                }
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let ctx_clone = ctx.clone();
            let node_shared = self.node.clone();
            let status_shared = self.receive_status.clone();
            let logs_shared = self.terminal_logs.clone();
            let is_receiving_shared = self.is_receiving.clone();
            let received_files_shared = self.received_files.clone();
            let save_directory_shared = self.save_directory.clone();

            tokio::spawn(async move {
                match EchoNode::spawn().await {
                    Ok(node) => {
                        let log_msg = format!("Connecting to node: {}", target_node_id);
                        println!("{}", log_msg);

                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(log_msg);
                        }

                        // Get events from connecting - send dummy request to trigger file transfer
                        let dummy_data = b"SEND_FILE".to_vec();
                        let mut events = node.connect(target_node_id, dummy_data, "request".to_string());

                        // Store the node
                        if let Ok(mut n) = node_shared.lock() {
                            *n = Some(node);
                        }

                        // Process connection events
                        use n0_future::StreamExt;
                        while let Some(event) = events.next().await {
                            match event {
                                crate::node::ConnectEvent::Connected => {
                                    let log_msg = "✓ Connected! Waiting for files...".to_string();
                                    println!("{}", log_msg);

                                    if let Ok(mut logs) = logs_shared.lock() {
                                        logs.push(log_msg);
                                    }
                                    if let Ok(mut status) = status_shared.lock() {
                                        *status = "Connected! Waiting for files...".to_string();
                                    }
                                    ctx_clone.request_repaint();
                                }
                                crate::node::ConnectEvent::Sent { .. } => {
                                    // Ignore - this is just the dummy request data
                                }
                                crate::node::ConnectEvent::Transfer(transfer_event) => {
                                    match transfer_event {
                                        crate::node::TransferEvent::FileStart { file_name, file_size, total_chunks, blob_hash } => {
                                            let hash_info = blob_hash.as_ref().map(|h| format!(" hash: {}...", &h[..16])).unwrap_or_default();
                                            let log_msg = format!("📥 Starting file: {} ({} bytes, {} chunks{})", file_name, file_size, total_chunks, hash_info);
                                            println!("{}", log_msg);

                                            if let Ok(mut logs) = logs_shared.lock() {
                                                logs.push(log_msg.clone());
                                            }
                                            if let Ok(mut status) = status_shared.lock() {
                                                *status = format!("Receiving: {} (0%)", file_name);
                                            }

                                            // Create/truncate file with the expected size
                                            if let Ok(save_dir_opt) = save_directory_shared.lock() {
                                                if let Some(save_dir) = save_dir_opt.as_ref() {
                                                    let file_path = std::path::Path::new(save_dir).join(&file_name);
                                                    // Pre-allocate file with correct size
                                                    if let Err(e) = std::fs::OpenOptions::new()
                                                        .write(true)
                                                        .create(true)
                                                        .truncate(true)
                                                        .open(&file_path)
                                                        .and_then(|f| f.set_len(file_size))
                                                    {
                                                        let err_msg = format!("Error creating file: {}", e);
                                                        println!("{}", err_msg);
                                                        if let Ok(mut logs) = logs_shared.lock() {
                                                            logs.push(err_msg);
                                                        }
                                                    }
                                                }
                                            }

                                            ctx_clone.request_repaint();
                                        }
                                        crate::node::TransferEvent::ChunkReceived { file_name, chunk_index, chunk_data, offset } => {
                                            // Write chunk at specific offset
                                            if let Ok(save_dir_opt) = save_directory_shared.lock() {
                                                if let Some(save_dir) = save_dir_opt.as_ref() {
                                                    let file_path = std::path::Path::new(save_dir).join(&file_name);

                                                    use std::io::{Seek, SeekFrom, Write};
                                                    match std::fs::OpenOptions::new()
                                                        .write(true)
                                                        .open(&file_path)
                                                        .and_then(|mut f| {
                                                            f.seek(SeekFrom::Start(offset))?;
                                                            f.write_all(&chunk_data)?;
                                                            Ok(())
                                                        })
                                                    {
                                                        Ok(_) => {
                                                            let log_msg = format!("  ✓ Chunk {}: {} bytes at offset {}", chunk_index, chunk_data.len(), offset);
                                                            if let Ok(mut logs) = logs_shared.lock() {
                                                                logs.push(log_msg);
                                                            }
                                                        },
                                                        Err(e) => {
                                                            let err_msg = format!("  ✗ Error writing chunk {}: {}", chunk_index, e);
                                                            println!("{}", err_msg);
                                                            if let Ok(mut logs) = logs_shared.lock() {
                                                                logs.push(err_msg);
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            ctx_clone.request_repaint();
                                        }
                                        crate::node::TransferEvent::FileComplete { file_name, total_bytes, hash_verified } => {
                                            let verify_status = match hash_verified {
                                                Some(true) => " ✓ verified",
                                                Some(false) => " ⚠ hash mismatch",
                                                None => "",
                                            };
                                            let log_msg = format!("✅ File complete: {} ({} bytes{})", file_name, total_bytes, verify_status);
                                            println!("{}", log_msg);

                                            if let Ok(mut logs) = logs_shared.lock() {
                                                logs.push(log_msg.clone());
                                            }

                                            // Add to received files list
                                            if let Ok(save_dir_opt) = save_directory_shared.lock() {
                                                if let Some(save_dir) = save_dir_opt.as_ref() {
                                                    let file_path = std::path::Path::new(save_dir).join(&file_name);
                                                    let saved_path = file_path.to_string_lossy().to_string();

                                                    if let Ok(mut status) = status_shared.lock() {
                                                        *status = format!("File saved: {}", file_name);
                                                    }

                                                    let timestamp = std::time::SystemTime::now()
                                                        .duration_since(std::time::UNIX_EPOCH)
                                                        .unwrap()
                                                        .as_secs();
                                                    let received_file = ReceivedFile {
                                                        name: file_name.clone(),
                                                        size: total_bytes,
                                                        saved_path,
                                                        timestamp: format!("{}", timestamp),
                                                    };

                                                    if let Ok(mut files) = received_files_shared.lock() {
                                                        files.push(received_file);
                                                    }
                                                }
                                            }

                                            ctx_clone.request_repaint();
                                        }
                                    }
                                }
                                crate::node::ConnectEvent::Closed { error } => {
                                    let log_msg = if let Some(err) = &error {
                                        format!("✗ Connection closed with error: {}", err)
                                    } else {
                                        "✓ Connection closed successfully".to_string()
                                    };
                                    println!("{}", log_msg);

                                    if let Ok(mut logs) = logs_shared.lock() {
                                        logs.push(log_msg);
                                    }
                                    if let Some(err) = error {
                                        if let Ok(mut status) = status_shared.lock() {
                                            *status = format!("Error: {}", err);
                                        }
                                    } else {
                                        if let Ok(mut status) = status_shared.lock() {
                                            *status = "Transfer complete!".to_string();
                                        }
                                    }
                                    ctx_clone.request_repaint();
                                    break;
                                }
                            }
                        }

                        ctx_clone.request_repaint();
                    }
                    Err(e) => {
                        let log_msg = format!("✗ Failed to connect: {}", e);
                        println!("{}", log_msg);

                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(log_msg);
                        }
                        if let Ok(mut status) = status_shared.lock() {
                            *status = format!("Connection failed: {}", e);
                        }
                        if let Ok(mut is_recv) = is_receiving_shared.lock() {
                            *is_recv = false;
                        }
                        ctx_clone.request_repaint();
                    }
                }
            });
        }
    }

    fn reconnect_for_files(&mut self, ctx: &egui::Context, target_node_id: NodeId) {
        let ctx_clone = ctx.clone();
        let node_shared = self.node.clone();
        let status_shared = self.receive_status.clone();
        let logs_shared = self.terminal_logs.clone();
        let received_files_shared = self.received_files.clone();
        let save_directory_shared = self.save_directory.clone();

        if let Ok(mut status) = self.receive_status.lock() {
            *status = "Refreshing files...".to_string();
        }

        #[cfg(not(target_arch = "wasm32"))]
        tokio::spawn(async move {
            let log_msg = format!("Refreshing files from node: {}", target_node_id);
            println!("{}", log_msg);

            if let Ok(mut logs) = logs_shared.lock() {
                logs.push(log_msg);
            }

            // Get a reference to the node and connect
            let node_ref = node_shared.clone();
            let events = {
                let node_guard = match node_ref.lock() {
                    Ok(guard) => guard,
                    Err(_e) => {
                        let error_msg = format!("Failed to lock node: {:?}", _e);
                        println!("{}", error_msg);
                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(error_msg);
                        }
                        if let Ok(mut status) = status_shared.lock() {
                            *status = "Error: Failed to access node".to_string();
                        }
                        ctx_clone.request_repaint();
                        return;
                    }
                };
                if node_guard.is_none() {
                    let error_msg = "No node available for reconnection".to_string();
                    println!("{}", error_msg);
                    if let Ok(mut logs) = logs_shared.lock() {
                        logs.push(error_msg);
                    }
                    if let Ok(mut status) = status_shared.lock() {
                        *status = "Error: No node running".to_string();
                    }
                    ctx_clone.request_repaint();
                    return;
                }
                let node = node_guard.as_ref().unwrap();

                let dummy_data = b"SEND_FILE".to_vec();
                node.connect(target_node_id, dummy_data, "request".to_string())
            };

            let mut events = events;

            // Process connection events
            use n0_future::StreamExt;
            while let Some(event) = events.next().await {
                match event {
                    crate::node::ConnectEvent::Connected => {
                        let log_msg = "✓ Reconnected! Fetching files...".to_string();
                        println!("{}", log_msg);

                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(log_msg);
                        }
                        if let Ok(mut status) = status_shared.lock() {
                            *status = "Fetching files...".to_string();
                        }
                        ctx_clone.request_repaint();
                    }
                    crate::node::ConnectEvent::Sent { .. } => {
                        // Ignore - this is just the dummy request data
                    }
                    crate::node::ConnectEvent::Transfer(transfer_event) => {
                        match transfer_event {
                            crate::node::TransferEvent::FileStart { file_name, file_size, total_chunks, blob_hash } => {
                                let hash_info = blob_hash.as_ref().map(|h| format!(" hash: {}...", &h[..16])).unwrap_or_default();
                                let log_msg = format!("📥 Starting file: {} ({} bytes, {} chunks{})", file_name, file_size, total_chunks, hash_info);
                                println!("{}", log_msg);

                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(log_msg.clone());
                        }
                        if let Ok(mut status) = status_shared.lock() {
                            *status = format!("Receiving: {} (0%)", file_name);
                        }

                        // Check if file already exists
                        let file_exists = if let Ok(files) = received_files_shared.lock() {
                            files.iter().any(|f| f.name == file_name)
                        } else {
                            false
                        };

                        if !file_exists {
                            // Create/truncate file with the expected size
                            if let Ok(save_dir_opt) = save_directory_shared.lock() {
                                if let Some(save_dir) = save_dir_opt.as_ref() {
                                    let file_path = std::path::Path::new(save_dir).join(&file_name);
                                    // Pre-allocate file with correct size
                                    if let Err(e) = std::fs::OpenOptions::new()
                                        .write(true)
                                        .create(true)
                                        .truncate(true)
                                        .open(&file_path)
                                        .and_then(|f| f.set_len(file_size))
                                    {
                                        let err_msg = format!("Error creating file: {}", e);
                                        println!("{}", err_msg);
                                        if let Ok(mut logs) = logs_shared.lock() {
                                            logs.push(err_msg);
                                        }
                                    }
                                }
                            }
                        }

                        ctx_clone.request_repaint();
                            }
                            crate::node::TransferEvent::ChunkReceived { file_name, chunk_index, chunk_data, offset } => {
                                // Check if file already exists in received files
                                let file_exists = if let Ok(files) = received_files_shared.lock() {
                                    files.iter().any(|f| f.name == file_name)
                                } else {
                                    false
                                };

                                if !file_exists {
                            // Write chunk at specific offset
                            if let Ok(save_dir_opt) = save_directory_shared.lock() {
                                if let Some(save_dir) = save_dir_opt.as_ref() {
                                    let file_path = std::path::Path::new(save_dir).join(&file_name);

                                    use std::io::{Seek, SeekFrom, Write};
                                    match std::fs::OpenOptions::new()
                                        .write(true)
                                        .open(&file_path)
                                        .and_then(|mut f| {
                                            f.seek(SeekFrom::Start(offset))?;
                                            f.write_all(&chunk_data)?;
                                            Ok(())
                                        })
                                    {
                                        Ok(_) => {
                                            let log_msg = format!("  ✓ Chunk {}: {} bytes at offset {}", chunk_index, chunk_data.len(), offset);
                                            if let Ok(mut logs) = logs_shared.lock() {
                                                logs.push(log_msg);
                                            }
                                        },
                                        Err(e) => {
                                            let err_msg = format!("  ✗ Error writing chunk {}: {}", chunk_index, e);
                                            println!("{}", err_msg);
                                            if let Ok(mut logs) = logs_shared.lock() {
                                                logs.push(err_msg);
                                            }
                                        }
                                    }
                                }
                            }
                        }

                                ctx_clone.request_repaint();
                            }
                            crate::node::TransferEvent::FileComplete { file_name, total_bytes, hash_verified } => {
                                // Check if file already exists
                                let file_exists = if let Ok(files) = received_files_shared.lock() {
                                    files.iter().any(|f| f.name == file_name)
                                } else {
                                    false
                                };

                                if !file_exists {
                                    let verify_status = match hash_verified {
                                        Some(true) => " ✓ verified",
                                        Some(false) => " ⚠ hash mismatch",
                                        None => "",
                                    };
                                    let log_msg = format!("✅ File complete: {} ({} bytes{})", file_name, total_bytes, verify_status);
                            println!("{}", log_msg);

                            if let Ok(mut logs) = logs_shared.lock() {
                                logs.push(log_msg.clone());
                            }

                            // Add to received files list
                            if let Ok(save_dir_opt) = save_directory_shared.lock() {
                                if let Some(save_dir) = save_dir_opt.as_ref() {
                                    let file_path = std::path::Path::new(save_dir).join(&file_name);
                                    let saved_path = file_path.to_string_lossy().to_string();

                                    if let Ok(mut status) = status_shared.lock() {
                                        *status = format!("File saved: {}", file_name);
                                    }

                                    let timestamp = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
                                        .as_secs();
                                    let received_file = ReceivedFile {
                                        name: file_name.clone(),
                                        size: total_bytes,
                                        saved_path,
                                        timestamp: format!("{}", timestamp),
                                    };

                                    if let Ok(mut files) = received_files_shared.lock() {
                                        files.push(received_file);
                                    }
                                }
                            }
                                } else {
                                    if let Ok(mut status) = status_shared.lock() {
                                        *status = format!("File already exists: {}", file_name);
                                    }
                                }

                                ctx_clone.request_repaint();
                            }
                        }
                    }
                    crate::node::ConnectEvent::Closed { error } => {
                        let log_msg = if let Some(err) = &error {
                            format!("✗ Connection closed with error: {}", err)
                        } else {
                            "✓ Refresh complete!".to_string()
                        };
                        println!("{}", log_msg);

                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(log_msg);
                        }
                        if let Some(err) = error {
                            if let Ok(mut status) = status_shared.lock() {
                                *status = format!("Error: {}", err);
                            }
                        } else {
                            if let Ok(mut status) = status_shared.lock() {
                                *status = "Connected! Files up to date.".to_string();
                            }
                        }
                        ctx_clone.request_repaint();
                        break;
                    }
                }
            }

            ctx_clone.request_repaint();
        });

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen_futures::spawn_local;
            use n0_future::StreamExt;

            let received_files_shared = self.received_files.clone();

            spawn_local(async move {
                web_sys::console::log_1(&format!("Refreshing files from node: {}", target_node_id).into());

                // Get a reference to the node and connect
                let node_ref = node_shared.clone();
                let events = {
                    let node_guard = node_ref.lock();
                    if node_guard.is_err() {
                        web_sys::console::log_1(&"Failed to lock node".into());
                        return;
                    }
                    let node_guard = node_guard.unwrap();
                    if node_guard.is_none() {
                        web_sys::console::log_1(&"Node not initialized".into());
                        return;
                    }
                    let node = node_guard.as_ref().unwrap();

                    let dummy_data = b"SEND_FILE".to_vec();
                    node.connect(target_node_id, dummy_data, "request".to_string())
                };

                let mut events = events;
                let mut current_file: Option<(String, Vec<Vec<u8>>)> = None;

                // Process connection events
                while let Some(event) = events.next().await {
                    match event {
                        crate::node::ConnectEvent::Connected => {
                            web_sys::console::log_1(&"✓ Reconnected! Fetching files...".into());
                            if let Ok(mut status) = status_shared.lock() {
                                *status = "Fetching files...".to_string();
                            }
                            ctx_clone.request_repaint();
                        }
                        crate::node::ConnectEvent::Sent { .. } => {
                            // Ignore - this is just the dummy request data
                        }
                        crate::node::ConnectEvent::Transfer(transfer_event) => {
                            match transfer_event {
                                crate::node::TransferEvent::FileStart { file_name, file_size, total_chunks, blob_hash } => {
                                    let hash_info = blob_hash.as_ref().map(|h| format!(" hash: {}...", &h[..16])).unwrap_or_default();
                                    web_sys::console::log_1(&format!("📥 Starting file: {} ({} bytes, {} chunks{})", file_name, file_size, total_chunks, hash_info).into());

                                    if let Ok(mut status) = status_shared.lock() {
                                        *status = format!("Receiving: {} (0%)", file_name);
                                    }

                                    // Check if file already exists
                                    let file_exists = if let Ok(files) = received_files_shared.lock() {
                                        files.iter().any(|f| f.name == file_name)
                                    } else {
                                        false
                                    };

                                    if !file_exists {
                                        current_file = Some((file_name, vec![Vec::new(); total_chunks as usize]));
                                    } else {
                                        web_sys::console::log_1(&format!("File already exists: {}", file_name).into());
                                    }

                                    ctx_clone.request_repaint();
                                }
                                crate::node::TransferEvent::ChunkReceived { file_name, chunk_index, chunk_data, offset: _ } => {
                                    if let Some((ref name, ref mut chunks)) = current_file {
                                        if name == &file_name && (chunk_index as usize) < chunks.len() {
                                            chunks[chunk_index as usize] = chunk_data;
                                            web_sys::console::log_1(&format!("  ✓ Chunk {} received", chunk_index).into());
                                        }
                                    }
                                    ctx_clone.request_repaint();
                                }
                                crate::node::TransferEvent::FileComplete { file_name, total_bytes, hash_verified } => {
                                    // Check if file already exists
                                    let file_exists = if let Ok(files) = received_files_shared.lock() {
                                        files.iter().any(|f| f.name == file_name)
                                    } else {
                                        false
                                    };

                                    if !file_exists {
                                        let verify_status = match hash_verified {
                                            Some(true) => " ✓ verified",
                                            Some(false) => " ⚠ hash mismatch",
                                            None => "",
                                        };
                                        web_sys::console::log_1(&format!("✅ File complete: {} ({} bytes{})", file_name, total_bytes, verify_status).into());

                                        // Combine all chunks and trigger download
                                        if let Some((name, chunks)) = current_file.take() {
                                            if name == file_name {
                                                let combined_data: Vec<u8> = chunks.into_iter().flatten().collect();

                                                // Trigger automatic download in browser
                                                Self::download_file_wasm(&file_name, &combined_data);

                                                let timestamp = js_sys::Date::now() as u64 / 1000;
                                                let received_file = ReceivedFile {
                                                    name: file_name.clone(),
                                                    size: total_bytes,
                                                    saved_path: "Downloaded to browser".to_string(),
                                                    timestamp: format!("{}", timestamp),
                                                };

                                                if let Ok(mut files) = received_files_shared.lock() {
                                                    files.push(received_file);
                                                }

                                                if let Ok(mut status) = status_shared.lock() {
                                                    *status = format!("File downloaded: {}", file_name);
                                                }
                                            }
                                        }
                                    } else {
                                        if let Ok(mut status) = status_shared.lock() {
                                            *status = format!("File already exists: {}", file_name);
                                        }
                                    }

                                    ctx_clone.request_repaint();
                                }
                            }
                        }
                        crate::node::ConnectEvent::Closed { error } => {
                            let msg = if let Some(err) = &error {
                                format!("✗ Connection closed with error: {}", err)
                            } else {
                                "✓ Refresh complete!".to_string()
                            };
                            web_sys::console::log_1(&msg.into());

                            if let Some(err) = error {
                                if let Ok(mut status) = status_shared.lock() {
                                    *status = format!("Error: {}", err);
                                }
                            } else {
                                if let Ok(mut status) = status_shared.lock() {
                                    *status = "Connected! Files up to date.".to_string();
                                }
                            }
                            ctx_clone.request_repaint();
                            break;
                        }
                    }
                }

                ctx_clone.request_repaint();
            });
        }
    }

    fn stop_receiving(&mut self) {
        if let Ok(mut is_recv) = self.is_receiving.lock() {
            *is_recv = false;
        }
        self.show_receive_dialog = false;

        if let Ok(mut status) = self.receive_status.lock() {
            status.clear();
        }

        if let Ok(mut node) = self.node.lock() {
            *node = None;
        }

        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&"Stopped receiving".into());

        #[cfg(not(target_arch = "wasm32"))]
        {
            let log_msg = "⏹ Stopped receiving".to_string();
            println!("{}", log_msg);

            if let Ok(mut logs) = self.terminal_logs.lock() {
                logs.push(log_msg);
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn download_file_wasm(file_name: &str, file_data: &[u8]) {
        use wasm_bindgen::JsCast;
        use web_sys::{Blob, BlobPropertyBag, Url, HtmlAnchorElement};

        let window = match web_sys::window() {
            Some(w) => w,
            None => {
                web_sys::console::error_1(&"Failed to get window object".into());
                return;
            }
        };

        let document = match window.document() {
            Some(d) => d,
            None => {
                web_sys::console::error_1(&"Failed to get document object".into());
                return;
            }
        };

        // Create a Blob from the file data
        let array = js_sys::Uint8Array::new_with_length(file_data.len() as u32);
        array.copy_from(file_data);

        let parts = js_sys::Array::new();
        parts.push(&array);

        let mut blob_props = BlobPropertyBag::new();
        blob_props.type_("application/octet-stream");

        let blob = match Blob::new_with_u8_array_sequence_and_options(&parts, &blob_props) {
            Ok(b) => b,
            Err(_e) => {
                web_sys::console::error_1(&format!("Failed to create blob: {:?}", _e).into());
                return;
            }
        };

        let url = match Url::create_object_url_with_blob(&blob) {
            Ok(u) => u,
            Err(_e) => {
                web_sys::console::error_1(&format!("Failed to create object URL: {:?}", _e).into());
                return;
            }
        };

        // Create a temporary anchor element and trigger download
        let anchor_elem = match document.create_element("a") {
            Ok(a) => a,
            Err(_e) => {
                web_sys::console::error_1(&format!("Failed to create anchor element: {:?}", _e).into());
                let _ = Url::revoke_object_url(&url);
                return;
            }
        };

        let anchor: HtmlAnchorElement = match anchor_elem.dyn_into() {
            Ok(a) => a,
            Err(_e) => {
                web_sys::console::error_1(&format!("Failed to cast to HtmlAnchorElement: {:?}", _e).into());
                let _ = Url::revoke_object_url(&url);
                return;
            }
        };

        anchor.set_href(&url);
        anchor.set_download(file_name);
        anchor.click();

        // Clean up
        let _ = Url::revoke_object_url(&url);
        web_sys::console::log_1(&format!("Download triggered for: {}", file_name).into());
    }

    #[cfg(not(target_arch = "wasm32"))]

    fn show_received_files(&mut self, ui: &mut Ui) {
        let tc = Tc::for_ui(ui);
        if let Ok(files) = self.received_files.lock() {
            if files.is_empty() {
                return;
            }

            ui.add_space(20.0);
            let card = egui::Frame::new()
                .fill(tc.surface_low)
                .rounding(CornerRadius::same(10))
                .stroke(Stroke::new(1.0, tc.outline_var))
                .inner_margin(0.0);
            card.show(ui, |ui| {
                ui.set_width(ui.available_width());

                // Section header
                let header_frame = egui::Frame::new()
                    .fill(tc.surface_high)
                    .rounding(CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 })
                    .inner_margin(egui::Margin { left: 16, right: 16, top: 10, bottom: 10 });
                header_frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Received Files")
                            .color(tc.on_surface).strong().size(15.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("{} items", files.len()))
                                .color(tc.outline).size(12.0));
                        });
                    });
                });

                ui.add_space(4.0);
                for (index, file) in files.iter().enumerate() {
                    let row_frame = egui::Frame::new()
                        .inner_margin(egui::Margin { left: 16, right: 16, top: 10, bottom: 10 });
                    row_frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            // File icon area
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(36.0, 36.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, CornerRadius::same(6), tc.surface_high);
                            ui.painter().text(
                                rect.center(), egui::Align2::CENTER_CENTER,
                                "📥", egui::FontId::proportional(18.0), tc.secondary,
                            );
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&file.name).color(tc.on_surface).strong().size(14.0));
                                ui.label(RichText::new(format!("{} · {}", self.format_size(file.size), &file.saved_path))
                                    .color(tc.outline).size(12.0));
                            });
                        });
                    });
                    if index < files.len() - 1 {
                        ui.add(egui::Separator::default().spacing(0.0).grow(0.0));
                    }
                }
                ui.add_space(4.0);
            });
        }
    }

    fn add_file_to_share(&mut self, _ctx: &egui::Context) {
        let file_info = {
            let name = match self.picked_file_name.lock() {
                Ok(guard) => guard.clone(),
                Err(_e) => {
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::error_1(&format!("Failed to lock picked_file_name: {:?}", _e).into());
                    None
                }
            };
            let path = match self.picked_file_path.lock() {
                Ok(guard) => guard.clone(),
                Err(_e) => {
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::error_1(&format!("Failed to lock picked_file_path: {:?}", _e).into());
                    None
                }
            };
            let size = match self.picked_file_size.lock() {
                Ok(guard) => guard.clone(),
                Err(_e) => {
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::error_1(&format!("Failed to lock picked_file_size: {:?}", _e).into());
                    None
                }
            };
            #[cfg(target_arch = "wasm32")]
            let data = match self.picked_file_data.lock() {
                Ok(guard) => guard.clone(),
                Err(_e) => {
                    web_sys::console::error_1(&format!("Failed to lock picked_file_data: {:?}", _e).into());
                    None
                }
            };

            #[cfg(target_arch = "wasm32")]
            match (name, path, size, data) {
                (Some(n), Some(p), Some(s), Some(d)) => Some((n, p, s, d)),
                _ => None
            }
            #[cfg(not(target_arch = "wasm32"))]
            match (name, path, size) {
                (Some(n), Some(p), Some(s)) => Some((n, p, s)),
                _ => None
            }
        };

        let should_restart = self.is_accepting;

        #[cfg(target_arch = "wasm32")]
        if let Some((name, path, size, data)) = file_info {
            let mut file_added = false;
            if let Ok(mut files) = self.shared_files.lock() {
                if !files.iter().any(|(n, _, _)| n == &name) {
                    files.push((name.clone(), path.clone(), size));
                    file_added = true;
                }
            }

            // Store the actual file data for WASM
            if file_added {
                if let Ok(mut files_data) = self.shared_files_data.lock() {
                    if !files_data.iter().any(|(n, _)| n == &name) {
                        files_data.push((name.clone(), data.clone()));
                        web_sys::console::log_1(&format!("Added file to share: {} ({} bytes)", name, data.len()).into());
                    }
                }

                // If node is running, update its file list directly
                if should_restart {
                    if let Ok(node_guard) = self.node.lock() {
                        if let Some(node) = node_guard.as_ref() {
                            let node_files = node.get_shared_files();
                            if let Ok(mut nf) = node_files.lock() {
                                nf.push((name.clone(), data));
                                web_sys::console::log_1(&format!("Updated running node with file: {}", name).into());
                            }
                        }
                    }
                }
            }

            // Clear the picked file
            if let Ok(mut name) = self.picked_file_name.lock() {
                *name = None;
            }
            if let Ok(mut path) = self.picked_file_path.lock() {
                *path = None;
            }
            if let Ok(mut size) = self.picked_file_size.lock() {
                *size = None;
            }
            if let Ok(mut data) = self.picked_file_data.lock() {
                *data = None;
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        if let Some((name, path, size)) = file_info {
            let mut file_added = false;
            if let Ok(mut files) = self.shared_files.lock() {
                if !files.iter().any(|(_, p, _)| p == &path) {
                    files.push((name.clone(), path.clone(), size));
                    file_added = true;

                    if let Ok(mut logs) = self.terminal_logs.lock() {
                        logs.push(format!("Added file to share: {}", name));
                    }
                }
            }

            // If node is running, update its file list directly
            if file_added && should_restart {
                if let Ok(node_guard) = self.node.lock() {
                    if let Some(node) = node_guard.as_ref() {
                        let node_files = node.get_shared_files();
                        if let Ok(data) = std::fs::read(&path) {
                            if let Ok(mut nf) = node_files.lock() {
                                nf.push((name.clone(), data));
                                if let Ok(mut logs) = self.terminal_logs.lock() {
                                    logs.push(format!("Updated running node with file: {}", name));
                                }
                            }
                        }
                    }
                }
            }

            // Clear the picked file
            if let Ok(mut name) = self.picked_file_name.lock() {
                *name = None;
            }
            if let Ok(mut path) = self.picked_file_path.lock() {
                *path = None;
            }
            if let Ok(mut size) = self.picked_file_size.lock() {
                *size = None;
            }
        }
    }    fn restart_node(&mut self, ctx: &egui::Context) {
        if let Ok(mut node) = self.node.lock() {
            *node = None;
        }

        // Wait a moment and restart
        let ctx_clone = ctx.clone();
        let node_id_shared = self.shared_node_id.clone();
        let node_shared = self.node.clone();
        let logs_shared = self.terminal_logs.clone();

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen_futures::spawn_local;
            let shared_files_data = self.shared_files_data.clone();

            spawn_local(async move {
                // Small delay before restart
                let promise = js_sys::Promise::new(&mut |resolve, _| {
                    web_sys::window()
                        .unwrap()
                        .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 100)
                        .unwrap();
                });
                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;

                let files_to_share: Vec<(String, Vec<u8>)> = if let Ok(files_data) = shared_files_data.lock() {
                    files_data.clone()
                } else {
                    Vec::new()
                };

                match EchoNode::spawn_with_files(files_to_share).await {
                    Ok(node) => {
                        let node_id = node.endpoint().node_id();

                        if let Ok(mut nid) = node_id_shared.lock() {
                            *nid = Some(node_id);
                        }

                        if let Ok(mut n) = node_shared.lock() {
                            *n = Some(node);
                        }

                        web_sys::console::log_1(&"Node restarted with updated files".into());

                        ctx_clone.request_repaint();
                    }
                    Err(e) => {
                        web_sys::console::log_1(&format!("Failed to restart node: {}", e).into());
                    }
                }
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let shared_files = self.shared_files.clone();

            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                let files_to_share: Vec<(String, Vec<u8>)> = if let Ok(files) = shared_files.lock() {
                    let mut result = Vec::new();
                    for (name, path, _size) in files.iter() {
                        match std::fs::read(path) {
                            Ok(data) => {
                                result.push((name.clone(), data));
                            }
                            Err(e) => {
                                if let Ok(mut logs) = logs_shared.lock() {
                                    logs.push(format!("Failed to read file {}: {}", name, e));
                                }
                            }
                        }
                    }
                    result
                } else {
                    Vec::new()
                };

                match EchoNode::spawn_with_files(files_to_share).await {
                    Ok(node) => {
                        let node_id = node.endpoint().node_id();

                        if let Ok(mut nid) = node_id_shared.lock() {
                            *nid = Some(node_id);
                        }

                        if let Ok(mut n) = node_shared.lock() {
                            *n = Some(node);
                        }

                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push("Node restarted with updated files".to_string());
                        }

                        ctx_clone.request_repaint();
                    }
                    Err(e) => {
                        if let Ok(mut logs) = logs_shared.lock() {
                            logs.push(format!("Failed to restart node: {}", e));
                        }
                    }
                }
            });
        }
    }

    fn show_shared_files(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let tc = Tc::for_ui(ui);
        let mut to_remove: Option<usize> = None;
        let should_restart;
        let mut should_start_accepting = false;

        {
            let files = self.shared_files.lock();
            if files.is_err() || files.as_ref().unwrap().is_empty() {
                return;
            }

            let files = files.unwrap();

            ui.add_space(16.0);
            let card = egui::Frame::new()
                .fill(tc.surface_low)
                .rounding(CornerRadius::same(10))
                .stroke(Stroke::new(1.0, tc.outline_var))
                .inner_margin(0.0);
            card.show(ui, |ui| {
                ui.set_width(ui.available_width());

                // Section header
                let header_frame = egui::Frame::new()
                    .fill(tc.surface_high)
                    .rounding(CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 })
                    .inner_margin(egui::Margin { left: 16, right: 16, top: 10, bottom: 10 });
                header_frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Files to Share").color(tc.on_surface).strong().size(15.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !self.is_accepting {
                                let share_btn = ui.add(
                                    Button::new(RichText::new("Start Sharing").color(tc.on_primary).strong())
                                        .fill(tc.primary)
                                        .rounding(CornerRadius::same(6))
                                );
                                share_btn.clone().on_hover_text("Start sharing all files");
                                if share_btn.clicked() {
                                    should_start_accepting = true;
                                }
                            }
                        });
                    });
                });

                for (index, (name, _path, size)) in files.iter().enumerate() {
                    let row_frame = egui::Frame::new()
                        .inner_margin(egui::Margin { left: 16, right: 16, top: 10, bottom: 10 });
                    row_frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            // File icon
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(36.0, 36.0), egui::Sense::hover());
                            ui.painter().rect_filled(rect, CornerRadius::same(6), tc.surface_high);
                            ui.painter().text(
                                rect.center(), egui::Align2::CENTER_CENTER,
                                "📄", egui::FontId::proportional(18.0), tc.primary,
                            );
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(name).color(tc.on_surface).strong().size(14.0));
                                ui.label(RichText::new(self.format_size(*size)).color(tc.outline).size(12.0));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(
                                    Button::new(RichText::new("Remove").color(tc.error).size(13.0))
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::new(1.0, tc.error))
                                        .rounding(CornerRadius::same(6))
                                ).clicked() {
                                    to_remove = Some(index);
                                }
                            });
                        });
                    });
                    if index < files.len() - 1 {
                        ui.add(egui::Separator::default().spacing(0.0).grow(0.0));
                    }
                }
                ui.add_space(4.0);
            });

            should_restart = self.is_accepting;
        }

        if let Some(index) = to_remove {
            // First, remove from shared_files and get the name
            let removed_name = if let Ok(mut files) = self.shared_files.lock() {
                let removed = files.remove(index);

                #[cfg(not(target_arch = "wasm32"))]
                if let Ok(mut logs) = self.terminal_logs.lock() {
                    logs.push(format!("Removed file from shared list: {}", removed.0));
                }

                Some(removed.0)
            } else {
                None
            };

            // Then, remove from WASM data storage (separate lock scope)
            #[cfg(target_arch = "wasm32")]
            if let Some(ref name) = removed_name {
                if let Ok(mut files_data) = self.shared_files_data.lock() {
                    files_data.retain(|(n, _)| n != name);
                    web_sys::console::log_1(&format!("Removed file from shared list: {}", name).into());
                }
            }

            // If node is running, update its file list directly
            // Avoid nested locks by getting data first, then updating node
            if should_restart && removed_name.is_some() {
                // Get the updated file list data before locking node
                #[cfg(target_arch = "wasm32")]
                let updated_files = if let Ok(files_data) = self.shared_files_data.lock() {
                    files_data.clone()
                } else {
                    Vec::new()
                };

                #[cfg(not(target_arch = "wasm32"))]
                let updated_files = if let Ok(files) = self.shared_files.lock() {
                    files.clone()
                } else {
                    Vec::new()
                };

                // Now lock node and update (no nested locks)
                if let Ok(node_guard) = self.node.lock() {
                    if let Some(node) = node_guard.as_ref() {
                        if let Ok(mut nf) = node.get_shared_files().lock() {
                            nf.clear();

                            #[cfg(target_arch = "wasm32")]
                            {
                                for (name, data) in updated_files.iter() {
                                    nf.push((name.clone(), data.clone()));
                                }
                            }

                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                // For non-WASM, read files from disk
                                for (name, path, _size) in updated_files.iter() {
                                    if let Ok(data) = std::fs::read(path) {
                                        nf.push((name.clone(), data));
                                    }
                                }

                                drop(nf);  // Release the lock before acquiring terminal_logs lock
                                if let Ok(mut logs) = self.terminal_logs.lock() {
                                    if let Some(ref name) = removed_name {
                                        logs.push(format!("Updated running node - removed: {}", name));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }        // Handle start accepting after locks are released
        if should_start_accepting {
            self.start_accepting(ctx);
        }
    }

    fn show_connection_status(&mut self, ui: &mut Ui) {
        let tc = Tc::for_ui(ui);
        if !self.is_accepting {
            return;
        }

        let mut should_stop = false;

        ui.add_space(16.0);

        // Active share card – green-tinted border
        let card = egui::Frame::new()
            .fill(tc.surface_low)
            .rounding(CornerRadius::same(10))
            .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(78, 222, 163, 60)))
            .inner_margin(16.0);
        card.show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Status row
            ui.horizontal(|ui| {
                // Pulsing green dot (approximated via a filled circle)
                let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                ui.painter().circle_filled(dot_rect.center(), 5.0, tc.secondary);
                ui.add_space(4.0);
                ui.label(RichText::new("Broadcasting Ready").color(tc.secondary).strong().size(16.0));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(
                        Button::new(RichText::new("Stop Sharing").color(tc.error).size(13.0))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, tc.error))
                            .rounding(CornerRadius::same(6))
                    ).clicked() {
                        should_stop = true;
                    }
                });
            });

            ui.add_space(12.0);

            // URL box
            if let Ok(url_opt) = self.shareable_url.lock() {
                if let Some(share_url) = url_opt.as_ref() {
                    ui.label(RichText::new("SHAREABLE LINK").color(tc.outline).size(11.0));
                    ui.add_space(4.0);

                    let url_box = egui::Frame::new()
                        .fill(tc.surface_lowest)
                        .rounding(CornerRadius::same(6))
                        .stroke(Stroke::new(1.0, tc.outline_var))
                        .inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 });
                    url_box.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(share_url).color(tc.primary).monospace().size(13.0));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(
                                    Button::new(RichText::new("Copy Link").color(tc.on_primary).size(13.0).strong())
                                        .fill(tc.primary)
                                        .rounding(CornerRadius::same(6))
                                ).clicked() {
                                    ui.ctx().copy_text(share_url.clone());
                                }
                            });
                        });
                    });

                    ui.add_space(8.0);
                    ui.label(RichText::new("Send this link to the receiver — they paste it to connect.")
                        .color(tc.outline).size(13.0).italics());
                } else {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Initializing node…").color(tc.outline).size(13.0));
                    });
                }
            }
        });

        if should_stop {
            self.stop_accepting();
        }
    }

    fn show_file_info(&mut self, ui: &mut Ui) {
        let tc = Tc::for_ui(ui);
        let (name, path, size) = {
            let file_name_binding = self.picked_file_name.lock().ok();
            let file_path_binding = self.picked_file_path.lock().ok();
            let file_size_binding = self.picked_file_size.lock().ok();

            match (
                file_name_binding.as_ref().map(|f| f.as_ref().cloned()),
                file_path_binding.as_ref().map(|f| f.as_ref().cloned()),
                file_size_binding.as_ref().map(|f| f.as_ref().cloned()),
            ) {
                (Some(Some(name)), Some(Some(path)), Some(Some(size))) => (name, path, size),
                _ => return,
            }
        };

        let size_str = self.format_size(size);
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!("============{:?}", size).into());

        ui.add_space(16.0);
        let card = egui::Frame::new()
            .fill(tc.surface_low)
            .rounding(CornerRadius::same(10))
            .stroke(Stroke::new(1.0, tc.outline_var))
            .inner_margin(16.0);
        card.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // File icon box
                let (rect, _) = ui.allocate_exact_size(Vec2::new(44.0, 44.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, CornerRadius::same(8), tc.surface_high);
                ui.painter().text(
                    rect.center(), egui::Align2::CENTER_CENTER,
                    "📄", egui::FontId::proportional(22.0), tc.primary,
                );
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(&name).color(tc.on_surface).strong().size(15.0));
                    ui.label(RichText::new(format!("{size_str}  ·  {path}")).color(tc.outline).size(12.0));
                });
            });

            ui.add_space(12.0);
            ui.add(egui::Separator::default().spacing(0.0));
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                if ui.add(
                    Button::new(RichText::new("+ Add to Share").color(tc.on_primary).strong().size(14.0))
                        .fill(tc.primary)
                        .rounding(CornerRadius::same(6))
                        .min_size(Vec2::new(140.0, 34.0))
                ).on_hover_text("Add this file to the share queue").clicked() {
                    self.add_file_to_share(ui.ctx());
                }

                if self.is_accepting {
                    ui.add_space(8.0);
                    if ui.add(
                        Button::new(RichText::new("Stop").color(tc.error).size(14.0))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, tc.error))
                            .rounding(CornerRadius::same(6))
                    ).on_hover_text("Stop accepting connections").clicked() {
                        self.stop_accepting();
                    }
                }
            });

            if !self.magnet_input.is_empty() {
                ui.add_space(10.0);
                let url_box = egui::Frame::new()
                    .fill(tc.surface_lowest)
                    .rounding(CornerRadius::same(6))
                    .stroke(Stroke::new(1.0, tc.outline_var))
                    .inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 });
                url_box.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&self.magnet_input).color(tc.primary).monospace().size(12.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                Button::new(RichText::new("Copy").color(tc.on_primary).size(13.0))
                                    .fill(tc.primary).rounding(CornerRadius::same(6))
                            ).clicked() {
                                ui.ctx().copy_text(self.magnet_input.clone());
                            }
                        });
                    });
                });
            }
        });
    }
}

impl P2PTransfer {
    fn apply_theme(ctx: &egui::Context) {
        let dark = ctx.style().visuals.dark_mode;
        let tc = if dark { Tc::dark() } else { Tc::light() };
        let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill                              = tc.bg;
        v.window_fill                             = tc.surface;
        v.faint_bg_color                          = tc.surface_low;
        v.extreme_bg_color                        = tc.surface_lowest;
        v.code_bg_color                           = tc.surface_lowest;
        v.widgets.noninteractive.bg_fill          = tc.surface_low;
        v.widgets.noninteractive.weak_bg_fill     = tc.surface_low;
        v.widgets.noninteractive.bg_stroke        = Stroke::new(1.0, tc.outline_var);
        v.widgets.noninteractive.fg_stroke        = Stroke::new(1.0, tc.outline);
        v.widgets.inactive.bg_fill                = tc.surface;
        v.widgets.inactive.weak_bg_fill           = tc.surface;
        v.widgets.inactive.bg_stroke              = Stroke::new(1.0, tc.outline_var);
        v.widgets.hovered.bg_fill                 = tc.surface_high;
        v.widgets.hovered.bg_stroke               = Stroke::new(1.0, tc.outline);
        v.widgets.active.bg_fill                  = tc.surface_high;
        v.widgets.active.bg_stroke                = Stroke::new(1.0, tc.primary);
        v.selection.bg_fill                       = Color32::from_rgba_unmultiplied(
            tc.primary.r(), tc.primary.g(), tc.primary.b(), 60,
        );
        v.override_text_color                     = Some(tc.on_surface);
        ctx.set_visuals(v);
    }

    fn show_home_cards(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let tc = Tc::for_ui(ui);
        ui.add_space(24.0);

        ui.vertical_centered(|ui| {
            ui.label(RichText::new("P2P File Transfer").color(tc.on_surface).size(28.0).strong());
            ui.add_space(4.0);
            ui.label(RichText::new("Secure, direct peer-to-peer sharing — no cloud storage").color(tc.outline).size(15.0));
        });

        ui.add_space(32.0);

        // Two cards side by side using columns
        let mut pick_file = false;
        let mut open_receive = false;

        ui.columns(2, |cols| {
            // ── Send card ───────────────────────────────────────────
            let send_frame = egui::Frame::new()
                .fill(tc.surface_low)
                .rounding(CornerRadius::same(12))
                .stroke(Stroke::new(1.0, tc.outline_var))
                .inner_margin(egui::Margin::same(28));
            send_frame.show(&mut cols[0], |ui| {
                ui.set_min_height(200.0);
                ui.vertical_centered(|ui| {
                    let (ico_rect, _) = ui.allocate_exact_size(Vec2::splat(56.0), egui::Sense::hover());
                    ui.painter().circle_filled(ico_rect.center(), 28.0, Color32::from_rgba_unmultiplied(192, 193, 255, 30));
                    ui.painter().text(ico_rect.center(), egui::Align2::CENTER_CENTER, "⬆", egui::FontId::proportional(28.0), tc.primary);
                    ui.add_space(14.0);
                    ui.label(RichText::new("Send a File").color(tc.on_surface).size(22.0).strong());
                    ui.add_space(6.0);
                    ui.label(RichText::new("Choose a file and generate a share link").color(tc.on_surface_var).size(14.0));
                    ui.add_space(20.0);
                    if ui.add(
                        Button::new(RichText::new("Choose File").color(tc.on_primary).strong().size(15.0))
                            .fill(tc.primary)
                            .rounding(CornerRadius::same(8))
                            .min_size(Vec2::new(150.0, 38.0))
                    ).clicked() {
                        pick_file = true;
                    }
                });
            });

            // ── Receive card ─────────────────────────────────────────
            let recv_frame = egui::Frame::new()
                .fill(tc.surface_low)
                .rounding(CornerRadius::same(12))
                .stroke(Stroke::new(1.0, tc.outline_var))
                .inner_margin(egui::Margin::same(28));
            recv_frame.show(&mut cols[1], |ui| {
                ui.set_min_height(200.0);
                ui.vertical_centered(|ui| {
                    let (ico_rect, _) = ui.allocate_exact_size(Vec2::splat(56.0), egui::Sense::hover());
                    ui.painter().circle_filled(ico_rect.center(), 28.0, Color32::from_rgba_unmultiplied(78, 222, 163, 30));
                    ui.painter().text(ico_rect.center(), egui::Align2::CENTER_CENTER, "⬇", egui::FontId::proportional(28.0), tc.secondary);
                    ui.add_space(14.0);
                    ui.label(RichText::new("Receive a File").color(tc.on_surface).size(22.0).strong());
                    ui.add_space(6.0);
                    ui.label(RichText::new("Paste a share link to download directly").color(tc.on_surface_var).size(14.0));
                    ui.add_space(20.0);
                    if ui.add(
                        Button::new(RichText::new("Receive").color(Color32::from_rgb(0, 56, 36)).strong().size(15.0))
                            .fill(tc.secondary)
                            .rounding(CornerRadius::same(8))
                            .min_size(Vec2::new(150.0, 38.0))
                    ).clicked() {
                        open_receive = true;
                    }
                });
            });
        });

        // Apply actions after columns closure ends
        if pick_file {
            #[cfg(target_arch = "wasm32")]
            self.pick_file(ctx);
            #[cfg(not(target_arch = "wasm32"))]
            self.pick_file();
        }
        if open_receive {
            self.show_receive_dialog = true;
        }
    }

    fn show_receive_panel(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let tc = Tc::for_ui(ui);
        ui.add_space(16.0);

        let card = egui::Frame::new()
            .fill(tc.surface_low)
            .rounding(CornerRadius::same(10))
            .stroke(Stroke::new(1.0, tc.outline_var))
            .inner_margin(24.0);
        card.show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Header
            ui.horizontal(|ui| {
                ui.label(RichText::new("Receive Files").color(tc.on_surface).strong().size(20.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(
                        Button::new(RichText::new("✕ Cancel").color(tc.outline).size(13.0))
                            .fill(Color32::TRANSPARENT)
                    ).clicked() {
                        self.show_receive_dialog = false;
                        self.receive_hash_input.clear();
                        if let Ok(mut s) = self.receive_status.lock() { s.clear(); }
                    }
                });
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Paste a peer ID or share link to establish a connection.")
                .color(tc.on_surface_var).size(14.0));

            ui.add_space(16.0);

            // Save folder (desktop only)
            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.label(RichText::new("SAVE FOLDER").color(tc.outline).size(11.0));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let dir_text = if let Ok(d) = self.save_directory.lock() {
                        d.clone().unwrap_or_else(|| "No folder selected".to_string())
                    } else {
                        "No folder selected".to_string()
                    };
                    let dir_color = if dir_text == "No folder selected" { tc.error } else { tc.secondary };
                    ui.label(RichText::new(&dir_text).color(dir_color).size(13.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(
                            Button::new(RichText::new("Select Folder").color(tc.on_primary).size(13.0))
                                .fill(tc.primary).rounding(CornerRadius::same(6))
                        ).clicked() {
                            use rfd::FileDialog;
                            if let Some(folder) = FileDialog::new().pick_folder() {
                                if let Ok(mut save_dir) = self.save_directory.lock() {
                                    *save_dir = Some(folder.to_string_lossy().to_string());
                                }
                            }
                        }
                    });
                });
                ui.add_space(14.0);
            }

            // Link input
            ui.label(RichText::new("PEER IDENTIFIER").color(tc.outline).size(11.0));
            ui.add_space(4.0);
            let input_frame = egui::Frame::new()
                .fill(tc.surface)
                .rounding(CornerRadius::same(6))
                .stroke(Stroke::new(1.0, tc.outline_var))
                .inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 });
            input_frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::TextEdit::singleline(&mut self.receive_hash_input)
                    .hint_text("Paste share link or node ID")
                    .frame(false)
                    .font(egui::FontId::monospace(13.0))
                    .text_color(tc.on_surface)
                    .desired_width(f32::INFINITY));
            });

            ui.add_space(10.0);

            // Status
            if let Ok(status) = self.receive_status.lock() {
                if !status.is_empty() {
                    let status_color = if status.starts_with("Error") { tc.error } else { tc.secondary };
                    ui.label(RichText::new(status.as_str()).color(status_color).size(13.0));
                    ui.add_space(8.0);
                }
            }

            let is_receiving = self.is_receiving.lock().map(|r| *r).unwrap_or(false);
            #[cfg(not(target_arch = "wasm32"))]
            let has_save_dir = self.save_directory.lock().ok()
                .and_then(|d| d.as_ref().map(|_| true)).unwrap_or(false);
            #[cfg(target_arch = "wasm32")]
            let has_save_dir = true;

            ui.horizontal(|ui| {
                if !is_receiving {
                    let enabled = has_save_dir && !self.receive_hash_input.trim().is_empty();
                    if ui.add_enabled(enabled,
                        Button::new(RichText::new("Connect").color(tc.on_primary).strong().size(14.0))
                            .fill(tc.primary)
                            .rounding(CornerRadius::same(6))
                            .min_size(Vec2::new(120.0, 34.0))
                    ).on_disabled_hover_text(if !has_save_dir {
                        "Select a save folder first"
                    } else {
                        "Enter a share link or node ID"
                    }).clicked() {
                        match self.extract_node_id(&self.receive_hash_input) {
                            Ok(node_id) => self.start_receiving(ctx, node_id),
                            Err(err) => {
                                if let Ok(mut s) = self.receive_status.lock() { *s = err; }
                            }
                        }
                    }
                } else {
                    ui.spinner();
                    ui.add_space(4.0);
                    ui.label(RichText::new("Receiving…").color(tc.secondary).size(14.0));
                    ui.add_space(16.0);
                    if ui.add(
                        Button::new(RichText::new("Refresh").color(tc.on_primary).size(13.0))
                            .fill(tc.primary).rounding(CornerRadius::same(6))
                    ).on_hover_text("Check for new files from sender").clicked() {
                        if let Ok(node_id) = self.extract_node_id(&self.receive_hash_input) {
                            self.reconnect_for_files(ctx, node_id);
                        }
                    }
                    ui.add_space(8.0);
                    if ui.add(
                        Button::new(RichText::new("Stop").color(tc.error).size(13.0))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, tc.error))
                            .rounding(CornerRadius::same(6))
                    ).clicked() {
                        self.stop_receiving();
                    }
                }
            });
        });

        // Received files below
        self.show_received_files(ui);
    }
}

impl eframe::App for P2PTransfer {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Auto-open receive panel when URL hash contains a node ID
        #[cfg(target_arch = "wasm32")]
        if !self.show_receive_dialog && !self.is_accepting {
            if let Some(node_id) = self.parse_node_id_from_url() {
                self.show_receive_dialog = true;
                self.receive_hash_input = format!("{}", node_id);
            }
        }

        Self::apply_theme(ctx);
        let tc = if ctx.style().visuals.dark_mode { Tc::dark() } else { Tc::light() };

        // ── Header ────────────────────────────────────────────────────
        let header_frame = egui::Frame::new()
            .fill(tc.surface)
            .stroke(Stroke::new(1.0, tc.outline_var))
            .inner_margin(egui::Margin { left: 24, right: 24, top: 0, bottom: 0 });
        egui::TopBottomPanel::top("header")
            .exact_height(54.0)
            .frame(header_frame)
            .show(ctx, |ui| {
                ui.set_height(54.0);
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("Syncoxiders").color(tc.primary).strong().size(20.0));

                    // Mode label
                    let mode_label = if self.show_receive_dialog {
                        "RECEIVE"
                    } else if self.is_accepting {
                        "SHARING"
                    } else {
                        "SEND"
                    };
                    ui.add_space(12.0);
                    ui.label(RichText::new(mode_label).color(tc.outline).size(11.0));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        egui::widgets::global_theme_preference_buttons(ui);

                        // Quick-access buttons (desktop only has File>Quit — skip)
                        ui.add_space(12.0);

                        // Receive toggle
                        if !self.is_accepting {
                            let recv_active = self.show_receive_dialog;
                            let recv_color = if recv_active { tc.secondary } else { tc.outline };
                            if ui.add(
                                Button::new(RichText::new("Receive").color(recv_color).size(13.0))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::new(1.0, recv_color))
                                    .rounding(CornerRadius::same(6))
                            ).clicked() {
                                self.show_receive_dialog = !self.show_receive_dialog;
                            }
                            ui.add_space(8.0);
                        }

                        // Send / Choose File button
                        if !self.show_receive_dialog {
                            if ui.add(
                                Button::new(RichText::new("Choose File").color(tc.on_primary).strong().size(13.0))
                                    .fill(tc.primary)
                                    .rounding(CornerRadius::same(6))
                            ).clicked() {
                                #[cfg(target_arch = "wasm32")]
                                self.pick_file(ctx);
                                #[cfg(not(target_arch = "wasm32"))]
                                self.pick_file();
                            }
                        }

                        // Desktop File > Quit
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            ui.add_space(12.0);
                            ui.menu_button("File", |ui| {
                                if ui.button("Quit").clicked() {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            });
                        }
                    });
                });
            });

        // ── Terminal bar (bottom) ─────────────────────────────────────
        let terminal_frame = egui::Frame::new()
            .fill(tc.surface_lowest)
            .stroke(Stroke::new(1.0, tc.outline_var))
            .inner_margin(egui::Margin { left: 20, right: 20, top: 0, bottom: 0 });
        let terminal_height = if self.show_terminal_view { 200.0 } else { 44.0 };
        egui::TopBottomPanel::bottom("terminal_bar")
            .exact_height(terminal_height)
            .frame(terminal_frame)
            .show(ctx, |ui| {
                // ── Header row ────────────────────────────────────────
                ui.horizontal(|ui| {
                    ui.set_height(44.0);
                    let chevron = if self.show_terminal_view { "▼" } else { "▲" };
                    let toggle_label = format!("{} Terminal Output >_", chevron);
                    if ui.add(
                        Button::new(RichText::new(toggle_label).color(tc.secondary).monospace().size(12.0))
                            .fill(Color32::TRANSPARENT)
                    ).clicked() {
                        self.show_terminal_view = !self.show_terminal_view;
                    }
                    ui.add_space(12.0);

                    // Latest log entry (preview when collapsed)
                    if !self.show_terminal_view {
                        if let Ok(logs) = self.terminal_logs.lock() {
                            let msg = logs.last().map(|s| s.as_str()).unwrap_or("No logs yet…");
                            ui.label(RichText::new(msg).color(tc.outline).monospace().size(12.0));
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(
                            Button::new(RichText::new("Clear").color(tc.outline).monospace().size(12.0))
                                .fill(Color32::TRANSPARENT)
                        ).clicked() {
                            if let Ok(mut logs) = self.terminal_logs.lock() { logs.clear(); }
                        }
                    });
                });

                // ── Expanded log view ─────────────────────────────────
                if self.show_terminal_view {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if let Ok(logs) = self.terminal_logs.lock() {
                                if logs.is_empty() {
                                    ui.label(RichText::new("No logs yet…").color(tc.outline_var).monospace().size(12.0));
                                } else {
                                    for line in logs.iter() {
                                        ui.label(RichText::new(line).color(tc.on_surface_var).monospace().size(12.0));
                                    }
                                }
                            }
                        });
                }
            });

        // ── Central content ───────────────────────────────────────────
        let content_frame = egui::Frame::new()
            .fill(tc.bg)
            .inner_margin(egui::Margin { left: 24, right: 24, top: 16, bottom: 16 });
        egui::CentralPanel::default()
            .frame(content_frame)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    if self.show_receive_dialog {
                        self.show_receive_panel(ui, ctx);
                        return;
                    }

                    let has_file = self.picked_file_name.lock().ok()
                        .and_then(|g| g.clone()).is_some();
                    let has_shared = self.shared_files.lock().ok()
                        .map(|g| !g.is_empty()).unwrap_or(false);

                    if !has_file && !has_shared && !self.is_accepting {
                        self.show_home_cards(ui, ctx);
                    } else {
                        self.show_file_info(ui);
                        self.show_shared_files(ui, ctx);
                        self.show_connection_status(ui);
                        self.show_received_files(ui);
                    }
                });
            });
    }
}