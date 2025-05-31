use eframe::egui;
use egui::{Button, Color32, Grid, Label, RichText, TextStyle, Ui, Vec2};
use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, JsValue};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct TorrentInfo{
    magnet_uri : Option<String>,
    download_progress: f32,
    peers_count: usize,
    is_download: bool,
    is_seeding: bool,
    download_complete: bool
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
    #[serde(skip)]
    torrent_info: std::sync::Arc<std::sync::Mutex<TorrentInfo>>,
    #[serde(skip)]
    magnet_input: String

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
            torrent_info: std::sync::Arc::new(std::sync::Mutex::new(TorrentInfo::default())),
            magnet_input: String::new(),
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
        use web_sys::{Event, HtmlInputElement};

        self.file_input_closure = None;

        let document = web_sys::window().unwrap().document().unwrap();

        let input = document.create_element("input").unwrap().dyn_into::<HtmlInputElement>().unwrap();
        input.set_attribute("type", "file").unwrap();

        let ctx_clone = ctx.clone();
        let shared_filename = self.picked_file_name.clone();
        let shared_filepath = self.picked_file_path.clone();
        let shared_filesize = self.picked_file_size.clone();

        let closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: Event| {
            let input = event.target().unwrap().dyn_into::<HtmlInputElement>().unwrap();

            if let Some(files) = input.files() {
                if let Some(file) = files.get(0) {
                    let name = file.name();
                    let size = file.size() as u64;
                    // In web, path is not fully accessible for security reasons, but we can use the name
                    let path = name.clone();

                    web_sys::console::log_1(&format!("Picked file: {}", name).into());

                    if let Some(window) = web_sys::window() {
                        if let Some(local_storage) = window.local_storage().ok().flatten() {
                            let _ = local_storage.set_item("picked", name.as_str());

                            // Update the shared states
                            if let Ok(mut filename) = shared_filename.lock() {
                                *filename = Some(name);
                            }
                            if let Ok(mut filepath) = shared_filepath.lock() {
                                *filepath = Some(path);
                            }
                            if let Ok(mut filesize) = shared_filesize.lock() {
                                *filesize = Some(size);
                            }
                        }
                    }

                    ctx_clone.request_repaint();
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

    // Added this method to fix the missing generate_magnet_uri error


    fn show_file_info(&mut self, ui: &mut Ui) {
        // Scope the mutex locks to extract data and drop guards early
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
                _ => {
                    // No file selected, display message and return
                    ui.add_space(15.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.label(RichText::new("📄").color(Color32::GRAY));
                        ui.label(RichText::new("No file selected").color(Color32::GRAY));
                    });
                    return;
                }
            }
        };

        // Generate magnet URI before entering the closure
        // if let Some(magnet_uri) = self.generate_magnet_uri(&path) {
        //     self.magnet_input = magnet_uri;
        // }

        ui.add_space(15.0);
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                // Header with icon
                ui.horizontal(|ui| {
                    ui.add(Label::new(RichText::new("📁").heading()));
                    ui.heading(RichText::new("Selected File").color(Color32::from_rgb(50, 150, 200)));
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                // File info in a more compact layout
                Grid::new("file_info_grid")
                    .num_columns(2)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        // File name
                        ui.strong("Name:");
                        ui.label(&name);
                        ui.end_row();

                        // File path
                        ui.strong("Path:");
                        ui.label(&path);
                        ui.end_row();

                        // File size
                        ui.strong("Size:");
                        #[cfg(target_arch = "wasm32")]
                        web_sys::console::log_1(&format!("============{:?}", size).into());
                        ui.label(self.format_size(size));
                        ui.end_row();
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                // Action buttons
                ui.horizontal(|ui| {
                    // Share button with icon
                    let share_btn = ui.add(
                        Button::new(RichText::new("🔗 Share").text_style(TextStyle::Button))
                    );
                    share_btn.clone().on_hover_text("Generate magnet URI for this file");

                    if share_btn.clicked() {
                        // if let Some(magnet_uri) = self.generate_magnet_uri(&path) {
                        //     self.magnet_input = magnet_uri;
                        // }
                    }
                });

                // Display generated magnet URI if available
                if !self.magnet_input.is_empty() {
                    ui.add_space(10.0);
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Magnet URI:").strong());
                            ui.add_space(5.0);
                            ui.label(&self.magnet_input);
                            ui.add_space(5.0);

                            if ui.button("📋 Copy").clicked() {
                                ui.ctx().copy_text(self.magnet_input.clone());
                            }
                        });
                    });
                }
            });
        });
    }
}

impl eframe::App for P2PTransfer {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame = egui::containers::Frame::new()
            .fill(ctx.style().visuals.window_fill)
            .inner_margin(20.0)
            .stroke(ctx.style().visuals.widgets.noninteractive.bg_stroke);

        egui::TopBottomPanel::top("top_panel")
            .frame(frame.clone())
            .show(ctx, |ui| {
                ui.add_space(4.0);
                egui::menu::bar(ui, |ui| {
                    ui.heading(RichText::new("Syncoxiders").strong());
                    ui.add_space(16.0);

                    let is_web = cfg!(target_arch = "wasm32");
                    if !is_web {
                        ui.menu_button("File", |ui| {
                            if ui.button("Quit").clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                        ui.add_space(16.0);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        egui::widgets::global_theme_preference_buttons(ui);
                    });
                });
                ui.add_space(4.0);
            });

        egui::CentralPanel::default()
            .frame(frame)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.heading(RichText::new("P2P File Sharing").size(24.0));
                    ui.add_space(5.0);
                    ui.label("Easily share files with a secure peer-to-peer connection");
                    ui.add_space(20.0);
                });

                ui.horizontal_centered(|ui| {
                    let btn = ui.add_sized(
                        Vec2::new(200.0, 40.0),
                        egui::Button::new(RichText::new("Choose File").size(16.0))
                            .fill(Color32::from_rgb(50, 150, 200))
                    );

                    if btn.clicked() {
                        #[cfg(target_arch = "wasm32")]
                        self.pick_file(ctx);
                        #[cfg(not(target_arch = "wasm32"))]
                        self.pick_file();
                    }
                });

                self.show_file_info(ui);

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("Powered by ");
                        ui.hyperlink_to("Syncoxiders", "https://github.com/emilk/eframe_template");
                        ui.label(" • ");
                        ui.hyperlink_to(
                            "Source code",
                            "https://github.com/emilk/eframe_template/blob/main/",
                        );
                    });
                    egui::warn_if_debug_build(ui);
                });
            });

        // egui::CentralPanel::default()
        // .frame(frame)
        // .show(ctx, |ui| {
        //     ui.label("Iroh node working");
        // });

    }
}