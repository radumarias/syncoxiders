use eframe::egui;
use egui::{Color32, RichText, Ui, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct P2PTransfer {
    #[serde(skip)]
    value: f32,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    file_input_closure: Option<wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>>,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    picked_file_name: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    picked_file_path: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    #[cfg(target_arch = "wasm32")]
    #[serde(skip)]
    picked_file_size: std::sync::Arc<std::sync::Mutex<Option<u64>>>,
}

impl Default for P2PTransfer {
    fn default() -> Self {
        Self {
            value: 0.0,
            #[cfg(target_arch = "wasm32")]
            file_input_closure: None,
            #[cfg(target_arch = "wasm32")]
            picked_file_name: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(target_arch = "wasm32")]
            picked_file_path: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(target_arch = "wasm32")]
            picked_file_size: std::sync::Arc::new(std::sync::Mutex::new(None)),
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

    fn show_file_info(&mut self, ui: &mut Ui) {
        let has_file = match (
            self.picked_file_name.lock().ok().as_ref().map(|f| f.as_ref().cloned()),
            self.picked_file_path.lock().ok().as_ref().map(|f| f.as_ref().cloned()),
            self.picked_file_size.lock().ok().as_ref().map(|f| f.as_ref().cloned()),
        ) {
            (Some(Some(name)), Some(Some(path)), Some(Some(size))) => {
                ui.add_space(10.0);
                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical(|ui| {
                        ui.heading(RichText::new("Selected File").color(Color32::from_rgb(50, 150, 200)));
                        ui.add_space(5.0);

                        // File name
                        ui.horizontal(|ui| {
                            ui.strong("Name:");
                            ui.label(name);
                        });

                        // File path
                        ui.horizontal(|ui| {
                            ui.strong("Path:");
                            ui.label(path);
                        });

                        // File size
                        ui.horizontal(|ui| {
                            ui.strong("Size:");
                            ui.label(self.format_size(size));
                        });
                    });
                });
                true
            },
            _ => false,
        };

        if !has_file {
            ui.add_space(10.0);
            ui.label(RichText::new("No file selected").color(Color32::GRAY));
        }
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
    }
}