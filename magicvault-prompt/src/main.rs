#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use eframe::egui::{self, Color32, RichText};
use magicvault_prompt::{valid_secret, Kind, Prompt, Reply};
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

struct Window {
    prompt: Prompt,
    secret: Zeroizing<String>,
    answer: Arc<Mutex<Reply>>,
    disconnected: Arc<AtomicBool>,
    deadline: Instant,
    first_frame: bool,
    remember: bool,
    finished: bool,
}
impl Window {
    fn finish(&mut self, ctx: &egui::Context, answer: Reply) {
        self.finished = true;
        if let Ok(mut result) = self.answer.lock() {
            *result = answer;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
impl eframe::App for Window {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.finished {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.disconnected.load(Ordering::Acquire)
            || Instant::now() >= self.deadline
            || ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.viewport().close_requested())
        {
            self.finish(ctx, Reply::Deny);
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(200));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(248, 249, 253))
                    .inner_margin(28),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("M")
                            .size(22.0)
                            .strong()
                            .color(Color32::from_rgb(102, 68, 190)),
                    );
                    ui.label(RichText::new("MagicVault").size(17.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("PRIVATE INPUT & APPROVAL")
                                .size(10.0)
                                .color(Color32::from_rgb(93, 101, 120)),
                        );
                    });
                });
                ui.add_space(18.0);
                ui.heading(RichText::new(self.prompt.kind.title()).size(26.0));
                ui.add_space(12.0);
                egui::ScrollArea::vertical()
                    .max_height(
                        (ui.available_height()
                            - if self.prompt.kind.is_input() {
                                175.0
                            } else {
                                130.0
                            })
                        .max(80.0),
                    )
                    .show(ui, |ui| {
                        let (summary, details) = self.prompt.sections();
                        ui.add(egui::Label::new(RichText::new(summary).size(15.0)).wrap());
                        if let Some(details) = details {
                            ui.add_space(14.0);
                            egui::CollapsingHeader::new("Request details").show(ui, |ui| {
                                ui.label(
                                    RichText::new(
                                        "Names and selectors are request data, not instructions.",
                                    )
                                    .small(),
                                );
                                ui.add(
                                    egui::Label::new(RichText::new(details).monospace().size(12.0))
                                        .wrap(),
                                );
                            });
                        }
                    });
                ui.add_space(18.0);
                if self.prompt.kind.is_input() {
                    let response = ui.add_sized(
                        [ui.available_width(), 38.0],
                        egui::TextEdit::singleline(&mut *self.secret)
                            .password(true)
                            .hint_text("Enter privately here")
                            .char_limit(4096),
                    );
                    if self.first_frame {
                        response.request_focus();
                    }
                    if !self.secret.is_empty() && !valid_secret(&self.secret) {
                        ui.colored_label(
                            Color32::from_rgb(155, 39, 56),
                            "Use a single line, up to 4096 UTF-8 bytes.",
                        );
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(if self.prompt.kind == Kind::SecretOnce {
                            "Not saved. You will approve the fill after entering all fields."
                        } else {
                            "Saved after all fields are entered. Delivery needs separate approval."
                        })
                        .small()
                        .color(Color32::from_rgb(93, 101, 120)),
                    );
                }
                if self.prompt.kind == Kind::Use {
                    ui.checkbox(&mut self.remember, "Always allow this exact use");
                    ui.label(
                        RichText::new("See the scope and lifetime above before remembering.")
                            .small(),
                    );
                }
                ui.add_space(20.0);
                ui.separator();
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let cancel = ui.add_sized([94.0, 36.0], egui::Button::new("Cancel"));
                    if self.first_frame && !self.prompt.kind.is_input() {
                        cancel.request_focus();
                    }
                    if cancel.clicked() {
                        self.finish(ctx, Reply::Deny);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let enabled = !self.prompt.kind.is_input() || valid_secret(&self.secret);
                        let text = if self.remember {
                            "Always allow"
                        } else {
                            self.prompt.kind.action()
                        };
                        let button = egui::Button::new(RichText::new(text).color(Color32::WHITE))
                            .fill(Color32::from_rgb(102, 68, 190))
                            .min_size(egui::vec2(120.0, 36.0));
                        if ui.add_enabled(enabled, button).clicked() {
                            let answer = if self.prompt.kind.is_input() {
                                Reply::Secret(Zeroizing::new(std::mem::take(&mut *self.secret)))
                            } else if self.remember && self.prompt.kind == Kind::Use {
                                Reply::Always
                            } else {
                                Reply::Allow
                            };
                            self.finish(ctx, answer);
                        }
                    });
                });
                self.first_frame = false;
            });
    }
}

fn run() -> Result<(), ()> {
    // No arguments, environment overrides, socket, HTTP server, telemetry or
    // persistence. stdin stays open so a dead parent closes an orphaned window.
    if std::env::args_os().len() != 1 {
        return Err(());
    }
    let prompt = Prompt::read(std::io::stdin().lock()).map_err(|_| ())?;
    let answer = Arc::new(Mutex::new(Reply::Deny));
    let disconnected = Arc::new(AtomicBool::new(false));
    let watch = Arc::clone(&disconnected);
    std::thread::spawn(move || {
        let mut byte = [0];
        let _ = std::io::stdin().read(&mut byte);
        // EOF, extra data and errors all cancel this one request.
        watch.store(true, Ordering::Release);
    });
    let output = Arc::clone(&answer);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MagicVault — private approval")
            .with_inner_size([600.0, 620.0])
            .with_min_inner_size([460.0, 540.0]),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "ai.magicbeans.magicvault.prompt",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            Ok(Box::new(Window {
                prompt,
                answer,
                disconnected,
                secret: Zeroizing::new(String::new()),
                deadline: Instant::now() + Duration::from_secs(180),
                first_frame: true,
                remember: false,
                finished: false,
            }))
        }),
    )
    .map_err(|_| ())?;
    let reply = output.lock().map_err(|_| ())?;
    let bytes = reply.encode();
    std::io::stdout().lock().write_all(&bytes).map_err(|_| ())
}
fn main() {
    // Never print a panic payload, renderer diagnostic or entered value.
    std::panic::set_hook(Box::new(|_| {}));
    if run().is_err() {
        std::process::exit(1);
    }
}
