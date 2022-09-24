use eframe::egui;
use fidget::render::RenderConfig;

mod highlight;

fn main() {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Fidget",
        options,
        Box::new(|_cc| Box::new(MyApp::default())),
    );
}

struct MyApp {
    label_height: Option<f32>,
    textures: Vec<egui::TextureHandle>,

    engine: fidget::bind::Engine,

    script: String,
    out: Result<fidget::bind::ScriptContext, String>,
}

////////////////////////////////////////////////////////////////////////////////

impl Default for MyApp {
    fn default() -> Self {
        let engine = fidget::bind::Engine::new();

        Self {
            textures: vec![],
            engine,
            script: "draw(circle(0, 0, 0.5))".to_owned(),
            out: Err("".to_string()),
            label_height: None,
        }
    }
}

impl MyApp {
    fn solarized(&mut self, ctx: &egui::Context) {
        let mut theme = egui::Visuals::dark();

        let f = |c: Option<syntect::highlighting::Color>| {
            c.map(|c| egui::Color32::from_rgb(c.r, c.g, c.b)).unwrap()
        };
        let sol = crate::highlight::get_theme().settings;

        theme.extreme_bg_color = f(sol.background);
        theme.widgets.noninteractive.bg_fill = f(sol.gutter);
        theme.widgets.hovered.bg_stroke =
            egui::Stroke::new(1.0, f(sol.selection_border));
        theme.selection.bg_fill = f(sol.selection);
        theme.selection.stroke =
            egui::Stroke::new(1.0, f(sol.selection_border));

        ctx.set_visuals(theme);
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.solarized(ctx);

        egui::SidePanel::left("root")
            .default_width(400.0)
            .frame(
                egui::Frame::none()
                    .fill(ctx.style().visuals.widgets.noninteractive.bg_fill)
                    .inner_margin(egui::style::Margin {
                        top: 7.0,
                        bottom: 5.0,
                        left: 5.0,
                        right: 5.0,
                    }),
            )
            .show(ctx, |ui| {
                let mut size = ui.available_size();
                if let Some(h) = self.label_height {
                    const PAD: f32 = 8.0;
                    let style = ui.style_mut();
                    style.spacing.item_spacing.y += PAD;
                    size.y -= h + 2.0 * PAD;
                }
                let r =
                    crate::highlight::code_view_ui(ui, &mut self.script, size);
                if r.changed() || self.label_height.is_none() {
                    self.out = self
                        .engine
                        .run(&self.script)
                        .map_err(|e| format!("{:?}", e));
                }

                let new_height = if let Err(e) = &self.out {
                    let label = ui.label(e);
                    label.rect.height()
                } else {
                    0.0
                };
                if Some(new_height) != self.label_height {
                    self.label_height = Some(new_height);
                    ctx.request_repaint();
                }
            });

        let rect = ctx.available_rect();
        let size = rect.max - rect.min;
        let max_size = size.x.max(size.y);
        let image_size = (max_size * ctx.pixels_per_point()) as usize;
        let tile_size = 64;
        let image_size = (image_size + tile_size - 1) / tile_size * tile_size;

        // Render shapes into self.textures
        let render_start = std::time::Instant::now();
        if let Ok(script_ctx) = &self.out {
            for (i, s) in script_ctx.shapes.iter().enumerate() {
                let tape = script_ctx
                    .context
                    .get_tape(s.shape, fidget::asm::dynasm::REGISTER_LIMIT);
                let image = fidget::render::render::<
                    fidget::asm::dynasm::JitEvalFamily,
                >(
                    tape,
                    &RenderConfig {
                        image_size,
                        tile_size,
                        subtile_size: tile_size / 8,
                        threads: 8,
                        interval_subdiv: 3,
                    },
                );
                let pixels = image
                    .into_iter()
                    .map(|p| {
                        egui::Color32::from_rgba_unmultiplied(
                            s.color_rgb[0],
                            s.color_rgb[1],
                            s.color_rgb[2],
                            p.as_alpha(),
                        )
                    })
                    .collect::<Vec<_>>();

                let image = egui::ImageData::Color(egui::ColorImage {
                    size: [image_size; 2],
                    pixels,
                });

                match self.textures.get_mut(i) {
                    Some(t) => {
                        if t.size() == [image_size; 2] {
                            t.set(image, egui::TextureFilter::Linear)
                        } else {
                            *t = ctx.load_texture(
                                "tex",
                                image,
                                egui::TextureFilter::Linear,
                            )
                        }
                    }
                    None => {
                        let texture = ctx.load_texture(
                            "tex",
                            image,
                            egui::TextureFilter::Linear,
                        );
                        self.textures.push(texture);
                    }
                }
            }
        }
        let dt = render_start.elapsed();
        ctx.set_visuals(egui::Visuals::dark());
        egui::Window::new("debug").show(ctx, |ui| {
            ui.label(format!("Image size: {0}x{0}", image_size));
            ui.label(format!("Render time: {:.2?}", dt));
        });

        let uv = if size.x > size.y {
            let r = (1.0 - (size.y / size.x)) / 2.0;
            egui::Rect {
                min: egui::Pos2::new(0.0, r),
                max: egui::Pos2::new(1.0, 1.0 - r),
            }
        } else {
            let r = (1.0 - (size.x / size.y)) / 2.0;
            egui::Rect {
                min: egui::Pos2::new(r, 0.0),
                max: egui::Pos2::new(1.0 - r, 1.0),
            }
        };

        let r = egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                let pos = ui.next_widget_position();
                let size = ui.available_size();
                let painter = ui.painter_at(egui::Rect {
                    min: pos,
                    max: pos + size,
                });
                for t in &self.textures {
                    let mut mesh = egui::Mesh::with_texture(t.id());
                    mesh.add_rect_with_uv(
                        egui::Rect {
                            min: pos,
                            max: pos + size,
                        },
                        uv,
                        egui::Color32::WHITE,
                    );
                    painter.add(mesh);
                }
                // Return events from the canvas in the inner response
                ui.interact(
                    egui::Rect {
                        min: pos,
                        max: pos + size,
                    },
                    egui::Id::new("canvas"),
                    egui::Sense::click_and_drag(),
                )
            });
        // TODO: handle r.inner
        if r.inner.hovered() {
            // TODO: handle ctx.input().scroll_delta
        }
    }
}