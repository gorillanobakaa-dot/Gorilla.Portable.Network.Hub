//! The teacher's window: every screen of the hub as a real window, with
//! buttons, lists, tick boxes and menus.
//!
//! WHY. A teacher who used the terminal screens called them unusable, and the
//! owner's order followed (2026-09-25): "EVERYTHING HAS TO HAVE A GUI IN RUST.
//! FOR EVERYTHING... Proper buttons proper menus proper graphical user
//! interface". The people this is for have never typed a command and should
//! never have to know which key does what.
//!
//! HOW. This is a child module of tui.rs, so it drives the very same `App`
//! the terminal screens drive: the same hotspot, server, pickers, tick lists,
//! accept and refuse, pause, change of password, downloads and checkup. Where
//! a terminal key did something, the button here calls the same code, often
//! literally the same key handler, so a behaviour tested on the terminal is
//! the behaviour behind the button. Only the drawing is new. The class chat
//! is drawn natively from room.rs and chat.rs, in the same process, with the
//! private chats as windows inside this one, as mIRC's were.
//!
//! The terminal screens stay, as `hub screen`, and as what runs where no
//! window can be opened.

use super::*;
use eframe::egui::{self, Color32, RichText, Vec2};

const GREEN: Color32 = Color32::from_rgb(0x3d, 0x5a, 0x1e);
const GREEN_OK: Color32 = Color32::from_rgb(0x1c, 0x7a, 0x3a);
const RED: Color32 = Color32::from_rgb(0xb3, 0x26, 0x1e);
const AMBER: Color32 = Color32::from_rgb(0x9a, 0x5b, 0x00);
const BLUE: Color32 = Color32::from_rgb(0x28, 0x52, 0x7a);
const CALL_BG: Color32 = Color32::from_rgb(0xff, 0xf4, 0xd6);
const MINE_BG: Color32 = Color32::from_rgb(0xdd, 0xe9, 0xd2);
const HELP_BG: Color32 = Color32::from_rgb(0xfd, 0xec, 0xea);
const NICKS: [Color32; 6] = [
    Color32::from_rgb(0x2c, 0x5d, 0x9e),
    Color32::from_rgb(0x8a, 0x3b, 0x8f),
    Color32::from_rgb(0x0f, 0x6e, 0x6e),
    Color32::from_rgb(0x9c, 0x4a, 0x12),
    Color32::from_rgb(0x5b, 0x5b, 0xb0),
    Color32::from_rgb(0x7a, 0x63, 0x00),
];

/// Open the window. False when no window could be made at all, so the caller
/// can fall back to the terminal screens.
pub fn run() -> bool {
    IN_WINDOW.store(true, std::sync::atomic::Ordering::Relaxed);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Gorilla Portable Network Hub")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([760.0, 520.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let started = eframe::run_native(
        "Gorilla Portable Network Hub",
        options,
        Box::new(|cc| {
            style(&cc.egui_ctx);
            let app = App::new();
            // Looked at straight away, in the background, so the first
            // screen can say whether this computer is ready.
            app.refresh_services();
            std::thread::spawn(|| {
                let _ = net::wifi_card_summary();
            });
            Ok(Box::new(Gui::new(app)))
        }),
    );
    started.is_ok()
}


/// A short sound when a child writes or asks for help. The window may be
/// behind another one; the sound and the flashing taskbar button are how the
/// teacher learns of it.
fn beep(urgent: bool) {
    #[cfg(windows)]
    {
        extern "system" {
            fn MessageBeep(kind: u32) -> i32;
        }
        // SAFETY: a plain Win32 call.
        unsafe {
            MessageBeep(if urgent { 0x10 } else { 0x40 });
        }
    }
    #[cfg(not(windows))]
    {
        let _ = urgent;
        print!("\x07");
    }
}

fn style(ctx: &egui::Context) {
    // Fonts this computer already has, for text the built-in ones cannot
    // draw: children's names and messages in Dari and Pashto, and the
    // symbols the page uses (the raised hand, the tick).
    let mut fonts = egui::FontDefinitions::default();
    let candidates: &[(&str, &str)] = if cfg!(windows) {
        &[
            ("segoe", "C:\\Windows\\Fonts\\segoeui.ttf"),
            ("symbols", "C:\\Windows\\Fonts\\seguisym.ttf"),
            ("emoji", "C:\\Windows\\Fonts\\seguiemj.ttf"),
            ("arabic", "C:\\Windows\\Fonts\\tahoma.ttf"),
        ]
    } else {
        &[
            ("segoe", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
            ("symbols", "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf"),
            ("emoji", "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf"),
            ("arabic", "/usr/share/fonts/truetype/noto/NotoNaskhArabic-Regular.ttf"),
        ]
    };
    let mut loaded = Vec::new();
    for (name, path) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert((*name).to_string(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            loaded.push(*name);
        }
    }
    let prop = fonts.families.entry(egui::FontFamily::Proportional).or_default();
    if loaded.contains(&"segoe") {
        prop.insert(0, "segoe".into());
    }
    for extra in ["symbols", "emoji", "arabic"] {
        if loaded.contains(&extra) {
            prop.push(extra.into());
        }
    }
    ctx.set_fonts(fonts);

    ctx.set_visuals(egui::Visuals::light());
    ctx.style_mut(|s| {
        use egui::{FontFamily, FontId, TextStyle};
        s.text_styles = [
            (TextStyle::Heading, FontId::new(26.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(17.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(17.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(15.0, FontFamily::Monospace)),
            (TextStyle::Small, FontId::new(13.5, FontFamily::Proportional)),
        ]
        .into();
        s.spacing.button_padding = Vec2::new(14.0, 8.0);
        s.spacing.item_spacing = Vec2::new(10.0, 8.0);
        s.spacing.interact_size.y = 34.0;
        // Tick boxes a finger or a shaky mouse can hit.
        s.spacing.icon_width = 24.0;
        s.spacing.icon_width_inner = 14.0;
        s.spacing.icon_spacing = 8.0;
        s.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, Color32::from_gray(150));
        s.visuals.selection.bg_fill = GREEN;
        s.visuals.hyperlink_color = BLUE;
        s.visuals.panel_fill = Color32::from_rgb(0xf2, 0xf4, 0xef);
        s.visuals.window_fill = Color32::from_rgb(0xfb, 0xfc, 0xf9);
        s.visuals.extreme_bg_color = Color32::WHITE;
    });
}

fn nick_colour(n: &str) -> Color32 {
    let mut h: u32 = 0;
    for c in n.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    NICKS[(h as usize) % NICKS.len()]
}

/// A big coloured button, for the one thing a screen is for.
fn big(ui: &mut egui::Ui, text: &str, fill: Color32) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(20.0).strong().color(Color32::WHITE))
            .fill(fill)
            .min_size(Vec2::new(220.0, 48.0))
            .corner_radius(8.0),
    )
}

/// A plain button with a coloured edge, for everything else.
fn plain(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(GREEN)).stroke(egui::Stroke::new(1.5_f32, GREEN)).corner_radius(6.0))
}

fn card<R>(ui: &mut egui::Ui, fill: Color32, edge: Color32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.5_f32, edge))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| ui.vertical(add).inner)
        .inner
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).heading().strong());
    ui.add_space(6.0);
}

/// A private chat open as a window inside the main one.
struct ChatWin {
    key: String,
    open: bool,
    big: bool,
    draft: String,
}

/// A card saying somebody wrote, with Open and Later.
struct Toast {
    key: String,
    who: String,
    text: String,
    help: bool,
}

struct Gui {
    app: App,
    /// The line being typed in # main.
    draft: String,
    wins: Vec<ChatWin>,
    toasts: Vec<Toast>,
    /// Per private chat, the newest message already announced.
    announced: std::collections::HashMap<String, u64>,
    /// The person whose menu is open in the people list.
    menu_for: Option<String>,
    /// A second press is needed to remove a device.
    sure_remove: Option<String>,
    topic_draft: Option<String>,
    nick_draft: String,
    notice_draft: String,
    confirm_stop: bool,
    confirm_close: bool,
    close_ok: bool,
    show_help: bool,
    show_about: bool,
    /// The typed address on the receive screen.
    address: String,
    /// HUB_GUI_TOUR: walk every screen by itself and save a picture of each.
    tour: Option<Tour>,
    /// The newest # main line on screen, to scroll to a new one.
    last_line: u64,
    /// The private chats as they were when the window first looked, so only
    /// what arrives afterwards rings.
    announced_init: bool,
}

/// The window's own screenshot tour (HUB_GUI_TOUR=<folder>).
///
/// For checking every screen and for the release pictures, with a pretend
/// class filled in through the same functions the real phones reach. Needed
/// because a mouse driven from outside cannot be trusted on a screen another
/// program covers (2026-09-25: clicks went to the window in front).
struct Tour {
    dir: PathBuf,
    step: usize,
    frames: u32,
    asked: bool,
}

impl Gui {
    fn new(app: App) -> Gui {
        Gui {
            app,
            draft: String::new(),
            wins: Vec::new(),
            toasts: Vec::new(),
            announced: std::collections::HashMap::new(),
            menu_for: None,
            sure_remove: None,
            topic_draft: None,
            nick_draft: String::new(),
            notice_draft: crate::page::notice(),
            confirm_stop: false,
            confirm_close: false,
            close_ok: false,
            show_help: false,
            show_about: false,
            address: String::new(),
            last_line: 0,
            announced_init: false,
            tour: std::env::var_os("HUB_GUI_TOUR").map(|d| Tour { dir: PathBuf::from(d), step: 0, frames: 0, asked: false }),
        }
    }

    fn lesson_running(&self) -> bool {
        matches!(
            self.app.screen,
            Screen::Sending
                | Screen::Class
                | Screen::NewPassword
                | Screen::JoinCode
                | Screen::Waiting
                | Screen::Messages
                | Screen::Thread { .. }
                | Screen::Tick { pre: false }
        ) || matches!(&self.app.screen, Screen::Note(_)) && self.app.started.is_some()
    }

    /// New private messages and HELP: a card, a sound, and the taskbar button
    /// flashing when the window is behind something else.
    fn announce(&mut self, ctx: &egui::Context) {
        let convs = crate::chat::conversations(false);
        let room = crate::room::snapshot();
        let mut urgent = None;
        for c in &convs {
            let newest = c.last_id;
            let before = self.announced.get(&c.key).copied();
            self.announced.insert(c.key.clone(), newest);
            let before = match before {
                Some(b) => b,
                // Already there when the window first looked: counted, not
                // rung. A conversation that starts later rings like any other.
                None if !self.announced_init => continue,
                None => 0,
            };
            if newest <= before || c.unread == 0 {
                continue;
            }
            let showing = self.wins.iter().any(|w| w.key == c.key && w.open);
            if showing {
                continue;
            }
            let who = room.people.iter().find(|p| p.key == c.key).map(|p| p.nick.clone()).unwrap_or_else(|| c.label.clone());
            self.toasts.retain(|t| t.key != c.key);
            self.toasts.insert(0, Toast { key: c.key.clone(), who, text: c.last_text.clone(), help: c.needs_talk });
            self.toasts.truncate(3);
            urgent = Some(urgent.unwrap_or(false) || c.needs_talk);
        }
        self.announced_init = true;
        if let Some(help) = urgent {
            beep(help);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(if help {
                egui::UserAttentionType::Critical
            } else {
                egui::UserAttentionType::Informational
            }));
        }
    }

    fn open_chat(&mut self, key: &str) {
        self.toasts.retain(|t| t.key != key);
        crate::chat::adult_opened(key, false);
        match self.wins.iter_mut().find(|w| w.key == key) {
            Some(w) => w.open = true,
            None => self.wins.push(ChatWin { key: key.to_string(), open: true, big: false, draft: String::new() }),
        }
        if !matches!(self.app.screen, Screen::Sending) && self.lesson_running() {
            self.app.screen = Screen::Sending;
        }
    }
}

impl eframe::App for Gui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        if let Some(path) = std::env::var_os("HUB_GUI_DEBUG") {
            let line = ctx.input(|i| {
                (!i.events.is_empty()).then(|| {
                    format!(
                        "ppp {} rect {:?} pointer {:?} events {:?}
",
                        i.pixels_per_point,
                        i.screen_rect,
                        i.pointer.latest_pos(),
                        i.events.iter().map(|e| format!("{e:?}").chars().take(80).collect::<String>()).collect::<Vec<_>>()
                    )
                })
            });
            if let Some(l) = line {
                use std::io::Write as _;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    let _ = f.write_all(l.as_bytes());
                }
            }
        }
        self.app.per_frame();
        // A download finishing moves the screen on by itself, as it does on
        // the terminal: its handler looks at the result whatever the key.
        if matches!(self.app.screen, Screen::Receiving) {
            self.app.receiving_key(Key::None);
        }
        if self.app.started.is_some() {
            self.announce(ctx);
        }

        // Closing the window while a class is connected: ask first.
        if ctx.input(|i| i.viewport().close_requested()) && self.app.hotspot.is_some() && !self.close_ok {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }

        self.run_tour(ctx);
        self.menu_bar(ctx);
        self.status_bar(ctx);
        let screen = self.app.screen.clone();
        match screen {
            Screen::Home => self.home(ctx),
            Screen::Pick => self.pick(ctx),
            Screen::Send => self.send(ctx),
            Screen::Tick { pre: true } => self.tick_screen(ctx, true),
            Screen::Checkup => self.checkup(ctx),
            Screen::FixOffer => self.fixoffer(ctx),
            Screen::Receive => self.receive(ctx),
            Screen::ReceiveFiles => self.receive_files(ctx),
            Screen::Receiving => self.receiving(ctx),
            Screen::Note(text) => self.note(ctx, &text),
            _ => self.lesson(ctx),
        }
        self.dialogs(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.app.shutdown();
    }
}

// ---------------------------------------------------------------- the frame around every screen

impl Gui {
    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                let busy = self.app.started.is_some();
                ui.menu_button("Hub", |ui| {
                    // During a lesson the way back to the start is to stop it,
                    // so that is what is offered, never a greyed-out entry.
                    if self.lesson_running() {
                        if ui.button(RichText::new("Stop the lesson").color(RED)).clicked() {
                            self.confirm_stop = true;
                            ui.close();
                        }
                    } else if ui.button("Start screen").clicked() {
                        self.app.screen = Screen::Home;
                        self.app.row = 0;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Close the hub").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        ui.close();
                    }
                });
                ui.menu_button("Lesson", |ui| {
                    for (i, what) in [
                        "Hand out files to the class over wifi",
                        "Send files down a cable to one other computer",
                        "Get files from another computer",
                    ]
                    .iter()
                    .enumerate()
                    {
                        if ui.add_enabled(!busy, egui::Button::new(*what)).clicked() {
                            self.app.screen = Screen::Home;
                            self.app.row = i;
                            self.app.home_key(Key::Enter);
                            ui.close();
                        }
                    }
                    if busy {
                        ui.separator();
                        ui.label(RichText::new("Stop the lesson first to start something else.").small());
                    }
                });
                ui.menu_button("Computer", |ui| {
                    if ui.add_enabled(!busy, egui::Button::new("Fix problems with this computer")).clicked() {
                        self.app.screen = Screen::Checkup;
                        self.app.row = 0;
                        ui.close();
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("How to use the hub").clicked() {
                        self.show_help = true;
                        ui.close();
                    }
                    if ui.button("About").clicked() {
                        self.show_about = true;
                        ui.close();
                    }
                });
            });
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Gorilla Portable Network Hub {}{}", env!("CARGO_PKG_VERSION"), crate::built()))
                        .small()
                        .weak(),
                );
                if self.lesson_running() {
                    ui.separator();
                    let live = serve::transfers();
                    let getting = live.iter().filter(|t| !t.finished).count();
                    ui.label(
                        RichText::new(format!(
                            "{} on the network   {} moving files   {} sent",
                            self.app.joined.len(),
                            getting,
                            human(serve::total_sent())
                        ))
                        .small(),
                    );
                    if let Some(ch) = net::hotspot_channel() {
                        ui.separator();
                        ui.label(RichText::new(format!("Broadcasting on {}", net::describe_channel(ch))).small());
                    }
                }
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.show_help {
            let mut open = true;
            egui::Window::new("How to use the hub").open(&mut open).default_size([640.0, 520.0]).show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(GUIDE);
                });
            });
            self.show_help = open;
        }
        if self.show_about {
            let mut open = true;
            egui::Window::new("About").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label(RichText::new("Gorilla Portable Network Hub").strong());
                ui.label(format!("Version {}{}", env!("CARGO_PKG_VERSION"), crate::built()));
                ui.label("Hands files to a class over wifi or a cable, with no internet and no router.");
            });
            self.show_about = open;
        }
        if self.confirm_close {
            egui::Window::new(RichText::new("Close the hub?").size(18.0).strong()).collapsible(false).default_width(420.0).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label("The class is still connected. Closing switches the wifi network off, and the phones lose the class page and the chat.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if big(ui, "Close and stop the lesson", RED).clicked() {
                        self.close_ok = true;
                        self.confirm_close = false;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if plain(ui, "Keep teaching").clicked() {
                        self.confirm_close = false;
                    }
                });
            });
        }
        if self.confirm_stop {
            egui::Window::new(RichText::new("Stop handing out?").size(18.0).strong()).collapsible(false).default_width(420.0).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label("The wifi network goes off and the phones lose the class page and the chat.");
                ui.label("Work already accepted stays in your received folder.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if big(ui, "Stop the lesson", RED).clicked() {
                        self.confirm_stop = false;
                        self.wins.clear();
                        self.toasts.clear();
                        self.announced.clear();
                        self.app.screen = Screen::Sending;
                        self.app.sending_key(Key::Char('q'));
                    }
                    if plain(ui, "Keep teaching").clicked() {
                        self.confirm_stop = false;
                    }
                });
            });
        }
    }

    fn note(&mut self, ctx: &egui::Context, text: &str) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(720.0);
                card(ui, Color32::WHITE, Color32::from_gray(200), |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(ui.available_height() - 90.0).show(ui, |ui| {
                        for (i, para) in text.split("\n\n").enumerate() {
                            if i == 0 {
                                ui.label(RichText::new(para).size(20.0).strong());
                            } else {
                                ui.label(para);
                            }
                            ui.add_space(6.0);
                        }
                    });
                    ui.add_space(10.0);
                    if big(ui, "OK", GREEN).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.app.screen = std::mem::replace(&mut self.app.back, Screen::Home);
                        self.app.row = 0;
                    }
                });
            });
        });
    }
}

// ---------------------------------------------------------------- the start screen

impl Gui {
    fn home(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("Gorilla Portable Network Hub").size(30.0).strong());
                ui.label("Hand lessons to a class over wifi or a cable. No internet and no router needed.");
                ui.add_space(12.0);
                match self.app.services_off() {
                    Some(off) if !off.is_empty() => {
                        card(ui, HELP_BG, RED, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new("This computer is not ready yet").size(20.0).strong().color(RED));
                            ui.label(format!("Parts of Windows the hub needs are switched off: {}.", off.join(", ")));
                            ui.label("Fix them first. It takes a few seconds; Windows will ask permission: say Yes.");
                            ui.add_space(6.0);
                            if big(ui, "Fix this computer now", RED).clicked() {
                                self.app.screen = Screen::Checkup;
                                self.app.row = 0;
                            }
                        });
                    }
                    Some(_) => {
                        ui.label(RichText::new("\u{2714} This computer is ready: everything the hub needs is switched on.").color(GREEN_OK));
                    }
                    None if cfg!(windows) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Checking whether this computer is ready...");
                        });
                    }
                    None => {}
                }
                ui.add_space(14.0);
                let choices: [(&str, &str, &str, Color32); 4] = [
                    ("\u{1f4f6}", "Hand out files to the class", "Makes a wifi network from this laptop. Phones and laptops join it, get the files you choose, talk in the class chat and send work back.", GREEN),
                    ("\u{1f50c}", "Send files down a cable", "For one other computer joined to this one by a network cable. The fastest way to move a lot at once.", BLUE),
                    ("\u{1f4e5}", "Get files from another computer", "Takes files from another computer running this hub, on the same wifi or over a cable.", BLUE),
                    ("\u{1f6e0}", "Fix problems with this computer", "Checks the parts of Windows the hub needs, the firewall and the wifi card, and switches back on what is off.", AMBER),
                ];
                let w = ((ui.available_width() - 20.0) / 2.0).max(300.0);
                egui::Grid::new("home").num_columns(2).spacing([20.0, 20.0]).show(ui, |ui| {
                    for (i, (icon, title, what, colour)) in choices.iter().enumerate() {
                        let resp = card(ui, Color32::WHITE, *colour, |ui| {
                            ui.set_width(w - 30.0);
                            ui.set_min_height(130.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(*icon).size(30.0));
                                ui.label(RichText::new(*title).size(21.0).strong().color(*colour));
                            });
                            ui.label(*what);
                            ui.add_space(4.0);
                            big(ui, "Open", *colour)
                        });
                        if resp.clicked() {
                            self.app.screen = Screen::Home;
                            self.app.row = i;
                            self.app.home_key(Key::Enter);
                        }
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
                ui.add_space(14.0);
                ui.label(
                    RichText::new(
                        "On a new computer, run \"Fix problems with this computer\" once, even if everything seems fine. \
                         Many laptops have had parts of Windows switched off to \"speed them up\", and nothing looks wrong \
                         until the wifi network or the cable fails in front of the class.",
                    )
                    .weak(),
                );
            });
        });
    }
}

// ---------------------------------------------------------------- choosing a folder

impl Gui {
    fn pick(&mut self, ctx: &egui::Context) {
        let receive = self.app.picking_receive;
        egui::TopBottomPanel::top("pick_top").show(ctx, |ui| {
            ui.add_space(6.0);
            heading(
                ui,
                if receive { "Choose where work from the class goes" } else { "Choose what to hand out" },
            );
            ui.label(if receive {
                "Open the folder you want, then press USE THIS FOLDER."
            } else {
                "Open the folder with the lesson in it and press the green button to hand all of it out. \
                 Or tick only the files and folders you want."
            });
            ui.add_space(6.0);
            // Where we are, each part a button to go back to it.
            ui.horizontal_wrapped(|ui| {
                if plain(ui, "\u{2b06} Up one folder").clicked() {
                    if let Some(p) = self.app.pick_dir.parent().map(|p| p.to_path_buf()) {
                        self.app.pick_at(p);
                    }
                }
                ui.separator();
                let mut acc = PathBuf::new();
                let here = self.app.pick_dir.clone();
                let mut go = None;
                for c in here.components() {
                    acc.push(c.as_os_str());
                    let name = c.as_os_str().to_string_lossy().trim_end_matches('\\').to_string();
                    if name.is_empty() {
                        continue;
                    }
                    if ui.link(name).clicked() {
                        go = Some(acc.clone());
                    }
                    ui.label("\u{203a}");
                }
                if let Some(p) = go {
                    self.app.pick_at(p);
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Places:").weak());
                for (name, path) in places() {
                    if ui.small_button(name).clicked() {
                        self.app.pick_at(path);
                    }
                }
            });
            ui.add_space(4.0);
        });
        egui::TopBottomPanel::bottom("pick_bottom").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let n = self.app.picked.len();
                let label = if receive {
                    "USE THIS FOLDER".to_string()
                } else if n == 0 {
                    "HAND OUT EVERYTHING IN THIS FOLDER".to_string()
                } else {
                    format!("HAND OUT THE {n} TICKED")
                };
                if big(ui, &label, GREEN).clicked() {
                    self.app.row = 0;
                    self.app.pick_key(Key::Enter);
                }
                if !receive && n > 0 && plain(ui, "Clear every tick").clicked() {
                    self.app.pick_key(Key::Char('c'));
                }
                if !receive && self.app.quick && plain(ui, "\u{2699} Wifi name, password and more").clicked() {
                    self.app.pick_key(Key::Char('s'));
                }
                if plain(ui, "Back").clicked() {
                    self.app.pick_key(Key::Esc);
                }
            });
            if !receive && self.app.picked.iter().any(|p| p.is_dir()) {
                ui.label(RichText::new("A ticked folder means everything inside it, however deep.").weak());
            }
            ui.add_space(6.0);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.app.pick_unreadable {
                ui.label(RichText::new("This folder cannot be read. Go up and choose another.").color(RED));
                return;
            }
            let kids = self.app.pick_kids.clone();
            let files = if receive { Vec::new() } else { self.app.pick_files.clone() };
            if kids.is_empty() && files.is_empty() {
                ui.label(RichText::new("This folder is empty.").weak());
            }
            let dir = self.app.pick_dir.clone();
            let mut go: Option<PathBuf> = None;
            let mut toggle: Option<PathBuf> = None;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 36.0, kids.len() + files.len(), |ui, range| {
                for i in range {
                    ui.horizontal(|ui| {
                        if i < kids.len() {
                            let p = dir.join(&kids[i]);
                            if !receive {
                                let mut on = self.app.pick_is_ticked(&p);
                                if ui.checkbox(&mut on, "").clicked() {
                                    toggle = Some(p.clone());
                                }
                            }
                            if ui.add(egui::Button::new(RichText::new(format!("\u{1f4c1}  {}", kids[i])).strong()).frame(false)).clicked() {
                                go = Some(p);
                            }
                        } else {
                            let (name, size) = &files[i - kids.len()];
                            let p = dir.join(name);
                            let mut on = self.app.pick_is_ticked(&p);
                            if ui.checkbox(&mut on, format!("\u{1f4c4}  {name}")).clicked() {
                                toggle = Some(p);
                            }
                            ui.label(RichText::new(human(*size)).weak());
                        }
                    });
                }
            });
            if let Some(p) = toggle {
                self.app.pick_toggle(p);
            }
            if let Some(p) = go {
                self.app.pick_at(p);
            }
        });
    }
}

/// The folders a person usually means, one click away.
fn places() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from) {
        for (n, sub) in [("Desktop", "Desktop"), ("Documents", "Documents"), ("Downloads", "Downloads"), ("Videos", "Videos"), ("Pictures", "Pictures")] {
            let p = home.join(sub);
            if p.is_dir() {
                out.push((n.to_string(), p));
            }
        }
        out.push(("Home".to_string(), home));
    }
    if cfg!(windows) {
        // Drives, including the USB stick a lesson usually lives on.
        for l in b'C'..=b'Z' {
            let p = PathBuf::from(format!("{}:\\", l as char));
            if p.is_dir() {
                out.push((format!("{}:", l as char), p));
            }
        }
    } else {
        for base in ["/media", "/run/media", "/mnt"] {
            if let Ok(rd) = std::fs::read_dir(base) {
                for e in rd.flatten() {
                    out.push((e.file_name().to_string_lossy().to_string(), e.path()));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- the settings

impl Gui {
    fn send(&mut self, ctx: &egui::Context) {
        let cable = self.app.cable;
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                heading(ui, if cable { "Sending down a cable" } else { "Handing out files: the settings" });
                ui.label(if cable {
                    "Join the two computers with a network cable, choose what to send, and press START."
                } else {
                    "Everything here has a sensible setting already. Change only what you need, then press START."
                });
                ui.add_space(10.0);
                egui::Grid::new("send").num_columns(2).spacing([18.0, 14.0]).min_col_width(220.0).show(ui, |ui| {
                    ui.label(RichText::new(if cable { "What to send" } else { "Folder to hand out" }).strong());
                    ui.horizontal(|ui| {
                        ui.label(self.app.what_to_send());
                        if plain(ui, "Choose...").clicked() {
                            self.app.open_picker();
                        }
                    });
                    ui.end_row();

                    if !cable {
                        ui.label(RichText::new("Wifi network to make").strong());
                        let mut ssid = self.app.ssid.clone();
                        let r = ui.add(egui::TextEdit::singleline(&mut ssid).desired_width(320.0).hint_text("Gorilla Hub"));
                        if r.changed() {
                            self.app.ssid = ssid.trim().to_string();
                        }
                        if r.lost_focus() && !self.app.password_typed {
                            // The same name keeps the same password, so phones
                            // that joined before connect at once.
                            if let Some(saved) = net::saved_hotspot_password(&self.app.ssid) {
                                self.app.password = saved;
                            }
                        }
                        ui.end_row();

                        ui.label(RichText::new("Password for it").strong());
                        ui.horizontal(|ui| {
                            let mut pw = self.app.password.clone();
                            if ui.add(egui::TextEdit::singleline(&mut pw).desired_width(220.0).font(egui::TextStyle::Monospace)).changed() {
                                self.app.password = pw.trim().to_string();
                                self.app.password_typed = true;
                            }
                            if plain(ui, "New password").clicked() {
                                self.app.password = net::suggest_password();
                                self.app.password_typed = true;
                            }
                        });
                        ui.end_row();
                        ui.label("");
                        ui.label(RichText::new("At least 8 letters or numbers. Short and easy to copy from the board.").small().weak());
                        ui.end_row();

                        if cfg!(windows) {
                            ui.label(RichText::new("Wifi band").strong());
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.app.band5, false, "2.4 GHz (every phone sees it)");
                                ui.radio_value(&mut self.app.band5, true, "5 GHz (faster; some phones will not see it)");
                            });
                        } else {
                            ui.label(RichText::new("Wifi channel").strong());
                            let mut ch = self.app.channel.clone();
                            if ui.add(egui::TextEdit::singleline(&mut ch).desired_width(80.0).hint_text("auto")).changed() {
                                self.app.channel = ch.trim().to_string();
                            }
                        }
                        ui.end_row();
                    } else {
                        ui.label(RichText::new("Set up the other computer").strong());
                        ui.vertical(|ui| {
                            ui.checkbox(&mut self.app.anyway, "Even when this computer is on another network (can break that network)");
                        });
                        ui.end_row();
                    }

                    ui.label(RichText::new("Connections to serve at once").strong());
                    ui.add(egui::DragValue::new(&mut self.app.helpers).range(1..=512));
                    ui.end_row();

                    ui.label(RichText::new("Work from the class goes to").strong());
                    ui.horizontal(|ui| {
                        ui.label(self.app.receive_dir.display().to_string());
                        if plain(ui, "Choose...").clicked() {
                            // The same route the terminal's last row takes.
                            let fields = self.app.send_fields().len();
                            self.app.row = fields - 1;
                            self.app.send_key(Key::Enter);
                        }
                    });
                    ui.end_row();
                });
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    if big(ui, "START", GREEN).clicked() {
                        let n = self.app.send_fields().len() + 1;
                        self.app.row = n - 1;
                        self.app.send_key(Key::Enter);
                    }
                    if plain(ui, "Back to the start").clicked() {
                        self.app.send_key(Key::Esc);
                    }
                });
            });
        });
    }
}

// ---------------------------------------------------------------- what the class can see

impl Gui {
    /// The tick list: before starting (`pre`), and during the lesson as the
    /// Files tab, where a tick publishes a file at once.
    fn tick_body(&mut self, ui: &mut egui::Ui, pre: bool) {
        ui.label(if pre {
            "Everything ticked is what the class will see. Untick anything that should stay private."
        } else {
            "Tick a file and it appears on the phones at once; untick it and it goes. A file put in the folder \
             during the lesson appears here unticked."
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if !self.app.tick_dir.is_empty() && plain(ui, "\u{2b06} Up one folder").clicked() {
                self.app.tick_up();
            }
            ui.label(RichText::new(if self.app.tick_dir.is_empty() { "All files".to_string() } else { format!("In {}/", self.app.tick_dir) }).strong());
            ui.separator();
            if plain(ui, "Tick everything here").clicked() {
                self.app.tick_key(Key::Char('a'), pre);
            }
            if plain(ui, "Untick everything here").clicked() {
                self.app.tick_key(Key::Char('n'), pre);
            }
        });
        if let Some(what) = self.app.tick_confirm.clone() {
            let (files, bytes, _) = self.app.tick_confirm_size(&what);
            card(ui, CALL_BG, AMBER, |ui| {
                ui.label(RichText::new(format!("That is {} files, {}. Tick them all?", count(files), human(bytes))).strong());
                ui.horizontal(|ui| {
                    if big(ui, "Yes, tick them all", AMBER).clicked() {
                        // The terminal asks for the same key twice; the
                        // second press is this button.
                        self.app.tick_confirm = Some(what.clone());
                        if what.starts_with('\u{0}') {
                            self.app.tick_key(Key::Char('a'), pre);
                        } else {
                            self.app.tick_key(Key::Char(' '), pre);
                        }
                    }
                    if plain(ui, "No").clicked() {
                        self.app.tick_confirm = None;
                    }
                });
            });
        }
        if !self.app.tick_said.is_empty() {
            ui.label(RichText::new(self.app.tick_said.clone()).color(GREEN_OK));
        }
        ui.add_space(4.0);
        let entries = self.app.tick_entries();
        let fixed = self.app.tick_fixed();
        let mut act: Option<(usize, Key)> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 70.0).show_rows(ui, 34.0, entries.len(), |ui, range| {
            for i in range {
                ui.horizontal(|ui| match &entries[i] {
                    TickEntry::Folder { name, files, ticked, bytes } => {
                        let mut on = *ticked == *files && *files > 0;
                        let mixed = *ticked > 0 && *ticked < *files;
                        if ui.add(egui::Checkbox::new(&mut on, "").indeterminate(mixed)).clicked() {
                            act = Some((i, Key::Char(' ')));
                        }
                        if ui.add(egui::Button::new(RichText::new(format!("\u{1f4c1}  {name}/")).strong()).frame(false)).clicked() {
                            act = Some((i, Key::Right));
                        }
                        ui.label(RichText::new(format!("{} of {} ticked, {}", count(*ticked), count(*files), human(*bytes))).weak());
                    }
                    TickEntry::File(ix) => {
                        let (name, size, ticked) = self.app.tick[*ix].clone();
                        let short = name.rsplit('/').next().unwrap_or(&name).to_string();
                        let mut on = ticked;
                        if ui.checkbox(&mut on, format!("\u{1f4c4}  {short}")).clicked() {
                            act = Some((i, Key::Char(' ')));
                        }
                        ui.label(RichText::new(human(size)).weak());
                    }
                });
            }
        });
        if let Some((i, k)) = act {
            self.app.row = fixed + i;
            self.app.tick_key(k, pre);
        }
    }

    fn tick_screen(&mut self, ctx: &egui::Context, pre: bool) {
        egui::TopBottomPanel::bottom("tick_bottom").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let n = self.app.tick.iter().filter(|t| t.2).count();
                if big(ui, &format!("START HANDING OUT ({} files)", count(n)), GREEN).clicked() {
                    self.app.row = 0;
                    self.app.tick_key(Key::Enter, pre);
                }
                if plain(ui, "Back").clicked() {
                    self.app.tick_key(Key::Esc, pre);
                }
            });
            ui.add_space(6.0);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            heading(ui, "Check what the class will see");
            self.tick_body(ui, pre);
        });
    }
}

// ---------------------------------------------------------------- the lesson

/// Which part of the lesson is showing, from the screen the program is on.
#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Chat,
    Who,
    Work,
    Files,
    Join,
}

impl Gui {
    fn tab(&self) -> Tab {
        match self.app.screen {
            Screen::Class | Screen::NewPassword => Tab::Who,
            Screen::Waiting => Tab::Work,
            Screen::Tick { pre: false } => Tab::Files,
            Screen::JoinCode => Tab::Join,
            _ => Tab::Chat,
        }
    }

    fn go(&mut self, t: Tab) {
        if self.tab() == Tab::Files && t != Tab::Files {
            self.app.apply_ticks();
        }
        match t {
            Tab::Chat => self.app.screen = Screen::Sending,
            Tab::Who => self.app.screen = Screen::Class,
            Tab::Work => self.app.screen = Screen::Waiting,
            Tab::Files => self.app.open_tick(false),
            Tab::Join => self.app.screen = Screen::JoinCode,
        }
        self.app.row = 0;
    }

    fn lesson(&mut self, ctx: &egui::Context) {
        let help = crate::chat::help_waiting();
        // The alarm, above everything, on every tab.
        if help > 0 {
            egui::TopBottomPanel::top("alarm").frame(egui::Frame::new().fill(RED).inner_margin(egui::Margin::same(10))).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let who: Vec<String> = crate::chat::conversations(false)
                        .iter()
                        .filter(|c| c.needs_talk)
                        .map(|c| c.key.clone())
                        .collect();
                    let room = crate::room::snapshot();
                    let names: Vec<String> = who
                        .iter()
                        .map(|k| room.people.iter().find(|p| &p.key == k).map(|p| p.nick.clone()).unwrap_or_else(|| k.clone()))
                        .collect();
                    ui.label(
                        RichText::new(format!("\u{270b} {} ASKED FOR HELP: {}", if help == 1 { "A CHILD" } else { "CHILDREN" }, names.join(", ")))
                            .size(20.0)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    if let Some(k) = who.first() {
                        if ui.add(egui::Button::new(RichText::new("Open").strong()).fill(Color32::WHITE)).clicked() {
                            let k = k.clone();
                            self.open_chat(&k);
                        }
                    }
                });
            });
        }

        let tab = self.tab();
        egui::SidePanel::left("lesson_side").resizable(false).exact_width(270.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(6.0);
                if let Some(h) = &self.app.hotspot {
                    card(ui, Color32::WHITE, GREEN, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("WRITE ON THE BOARD").small().strong().color(GREEN));
                        ui.label("1. Join the wifi network");
                        ui.label(RichText::new(h.ssid.clone()).size(24.0).strong());
                        ui.label("2. Type the password");
                        ui.label(RichText::new(self.app.password.clone()).size(24.0).strong().monospace());
                        ui.label("3. The class page opens by itself and asks for a name.");
                    });
                } else {
                    card(ui, Color32::WHITE, GREEN, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(if self.app.cable { "THE OTHER COMPUTER TYPES" } else { "THE CLASS TYPES" }).small().strong().color(GREEN));
                        for a in self.app.addresses_to_give().iter().take(2) {
                            let url = if serve::on_port_80() { format!("http://{a}") } else { format!("http://{a}:{}", port()) };
                            ui.label(RichText::new(url).size(18.0).strong());
                        }
                        if !self.app.cable_note.is_empty() {
                            ui.label(RichText::new(self.app.cable_note.clone()).small());
                        }
                    });
                }
                if let Some(problem) = net::hotspot_problem() {
                    card(ui, HELP_BG, RED, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(problem).color(RED));
                    });
                }
                ui.add_space(8.0);
                let unread = crate::chat::unread(false);
                let pending = serve::pending_count();
                let on = crate::room::online();
                for (t, label) in [
                    (Tab::Chat, format!("\u{1f4ac}  Class chat{}", if unread > 0 { format!("  ({unread} new)") } else { String::new() })),
                    (Tab::Who, format!("\u{1f465}  Who is connected ({})", self.app.joined.len().max(on))),
                    (Tab::Work, format!("\u{1f4e5}  Work handed in{}", if pending > 0 { format!("  ({pending})") } else { String::new() })),
                    (Tab::Files, "\u{1f4c2}  Files the class can see".to_string()),
                    (Tab::Join, "\u{1f4f7}  Join code for cameras".to_string()),
                ] {
                    let sel = tab == t;
                    let text = if sel { RichText::new(label).strong().color(Color32::WHITE) } else { RichText::new(label) };
                    let b = egui::Button::new(text).fill(if sel { GREEN } else { Color32::TRANSPARENT }).min_size(Vec2::new(ui.available_width(), 40.0));
                    if ui.add(b).clicked() {
                        self.go(t);
                    }
                }
                ui.add_space(8.0);
                if plain(ui, "\u{1f4c2}  Open the received folder").clicked() {
                    open_with_system(&self.app.receive_dir);
                }
                ui.add_space(8.0);
                ui.label(RichText::new("A notice at the top of every phone's page").small().strong());
                ui.add(egui::TextEdit::singleline(&mut self.notice_draft).hint_text("for example: Open lesson 2").desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    if plain(ui, "Show it").clicked() {
                        crate::page::set_notice(&self.notice_draft);
                    }
                    if !crate::page::notice().is_empty() && plain(ui, "Take it down").clicked() {
                        crate::page::set_notice("");
                        self.notice_draft.clear();
                    }
                });
                ui.add_space(16.0);
                if big(ui, "STOP HANDING OUT", RED).clicked() {
                    self.confirm_stop = true;
                }
            });
        });

        match tab {
            Tab::Chat => self.chat(ctx),
            Tab::Who => self.who(ctx),
            Tab::Work => self.work(ctx),
            Tab::Files => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    heading(ui, "Files the class can see");
                    self.tick_body(ui, false);
                });
            }
            Tab::Join => self.join(ctx),
        }
        self.chat_windows(ctx);
        self.toast_cards(ctx);
    }

    // ------------------------------------------------------------ the chat

    fn chat(&mut self, ctx: &egui::Context) {
        let room = crate::room::snapshot();
        let convs = crate::chat::conversations(false);
        let op = crate::room::op_nick();

        // The people, right, as mIRC's nick list.
        egui::SidePanel::right("people").resizable(false).exact_width(240.0).show(ctx, |ui| {
            let online: Vec<&crate::room::PersonView> = room.people.iter().filter(|p| p.online).collect();
            ui.add_space(6.0);
            if let Some((_, at)) = &room.check {
                let got = online.iter().filter(|p| !p.answered.is_empty()).count();
                ui.label(RichText::new(format!("{got} of {} answered", online.len())).size(20.0).strong());
                ui.label(RichText::new(format!("Comms check at {at}")).small().weak());
            } else {
                ui.label(RichText::new(format!("{} here now", online.len())).size(20.0).strong());
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("\u{25cf}").color(GREEN_OK));
                ui.label(RichText::new("@").strong().color(nick_colour(&op)));
                if self.nick_draft.is_empty() {
                    self.nick_draft = op.clone();
                }
                let r = ui
                    .add(egui::TextEdit::singleline(&mut self.nick_draft).desired_width(110.0))
                    .on_hover_text("Your name in the class chat. Type a new one and press Enter.");
                if r.lost_focus() && self.nick_draft != op && !crate::room::set_op_nick(&self.nick_draft) {
                    self.nick_draft = op.clone();
                }
                ui.label(RichText::new("you").small().weak());
            });
            let mut people = room.people.clone();
            let help_of = |k: &str| convs.iter().any(|c| c.key == k && c.needs_talk);
            people.sort_by_key(|p| {
                let rank = if help_of(&p.key) {
                    0
                } else if !p.online {
                    3
                } else if room.check.is_some() && p.answered.is_empty() {
                    1
                } else {
                    2
                };
                (rank, p.nick.to_lowercase())
            });
            egui::ScrollArea::vertical().show(ui, |ui| {
                for p in &people {
                    let help = help_of(&p.key);
                    let unread = convs.iter().find(|c| c.key == p.key).map(|c| c.unread).unwrap_or(0);
                    let (dot, state) = if help {
                        (RED, "HELP".to_string())
                    } else if !p.online {
                        (Color32::GRAY, "gone".to_string())
                    } else if unread > 0 {
                        (GREEN_OK, format!("\u{2709} {unread}"))
                    } else if !p.answered.is_empty() {
                        (GREEN_OK, format!("\u{2714} {}", p.answered))
                    } else if room.check.is_some() {
                        (AMBER, "waiting".to_string())
                    } else {
                        (GREEN_OK, String::new())
                    };
                    let resp = ui
                        .horizontal(|ui| {
                            ui.label(RichText::new("\u{25cf}").color(dot));
                            let name = RichText::new(p.nick.clone()).strong().color(if p.online { nick_colour(&p.nick) } else { Color32::GRAY });
                            let r = ui.add(egui::Button::new(name).frame(false));
                            if p.muted {
                                ui.label(RichText::new("muted").small().weak());
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(state).small());
                            });
                            r
                        })
                        .inner;
                    if resp.clicked() {
                        self.menu_for = if self.menu_for.as_deref() == Some(p.key.as_str()) { None } else { Some(p.key.clone()) };
                        self.sure_remove = None;
                    }
                    if self.menu_for.as_deref() == Some(p.key.as_str()) {
                        card(ui, Color32::WHITE, Color32::from_gray(200), |ui| {
                            ui.set_width(ui.available_width());
                            if ui.button(format!("\u{1f4ac} Private chat with {}", p.nick)).clicked() {
                                let k = p.key.clone();
                                self.menu_for = None;
                                self.open_chat(&k);
                            }
                            if ui.button(if p.muted { "Let them write in # main again" } else { "Mute in # main" }).clicked() {
                                if crate::room::set_muted(&p.key, !p.muted) {
                                    note_record(&self.app, &format!("@{op}  {} {} in # main", if p.muted { "unmuted" } else { "muted" }, p.nick));
                                }
                                self.menu_for = None;
                            }
                            let sure = self.sure_remove.as_deref() == Some(p.key.as_str());
                            let label = if sure { format!("Press again to remove {}", p.nick) } else { "Remove this device".to_string() };
                            if ui.add(egui::Button::new(RichText::new(label).color(RED))).clicked() {
                                if sure {
                                    if let Some((ip, name)) = crate::room::remove(&p.key) {
                                        let label = serve::block_device(&ip);
                                        note_record(&self.app, &format!("{label}  removed from the lesson by @{op} ({name}); paused, the teacher can let them back"));
                                    }
                                    self.menu_for = None;
                                    self.sure_remove = None;
                                } else {
                                    self.sure_remove = Some(p.key.clone());
                                }
                            }
                        });
                    }
                }
                if people.is_empty() {
                    ui.label(RichText::new("Nobody yet. When a phone joins the wifi and types a name, it appears here.").weak());
                }
            });
        });

        // The rooms and private chats, left of the conversation.
        egui::SidePanel::left("rooms").resizable(false).exact_width(200.0).show(ctx, |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("ROOMS").small().weak());
            let _ = ui.add(egui::Button::new(RichText::new("# main").strong().color(Color32::WHITE)).fill(GREEN).min_size(Vec2::new(ui.available_width(), 36.0)));
            ui.add_space(8.0);
            ui.label(RichText::new("PRIVATE CHATS").small().weak());
            let mut order = convs.clone();
            order.sort_by_key(|c| (!c.needs_talk, c.unread == 0));
            for c in &order {
                let nick = room.people.iter().find(|p| p.key == c.key).map(|p| p.nick.clone()).unwrap_or_else(|| c.label.clone());
                let text = if c.needs_talk {
                    RichText::new(format!("\u{270b} HELP {nick}")).strong().color(RED)
                } else if c.unread > 0 {
                    RichText::new(format!("{nick}  ({})", c.unread)).strong()
                } else {
                    RichText::new(nick)
                };
                let fill = if c.needs_talk { HELP_BG } else { Color32::TRANSPARENT };
                if ui.add(egui::Button::new(text).fill(fill).min_size(Vec2::new(ui.available_width(), 34.0))).clicked() {
                    let k = c.key.clone();
                    self.open_chat(&k);
                }
            }
            if convs.is_empty() {
                ui.label(RichText::new("Click a name on the right to talk to one person.").small().weak());
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // The operator's tools, above the room.
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("# main").size(20.0).strong().monospace());
                match &mut self.topic_draft {
                    Some(t) => {
                        let r = ui.add(egui::TextEdit::singleline(t).desired_width(320.0).hint_text("Today's lesson or task"));
                        if plain(ui, "Set").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            crate::room::set_topic(t);
                            self.topic_draft = None;
                        }
                    }
                    None => {
                        ui.label(RichText::new(if room.topic.is_empty() { "No topic set".to_string() } else { room.topic.clone() }).weak());
                        if ui.small_button("Set topic").clicked() {
                            self.topic_draft = Some(room.topic.clone());
                        }
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                let online: Vec<&crate::room::PersonView> = room.people.iter().filter(|p| p.online).collect();
                let left = online.iter().filter(|p| p.answered.is_empty()).count();
                let label = if room.check.is_some() && left > 0 { format!("\u{1f4e1} CHECK AGAIN ({left})") } else { "\u{1f4e1} COMMS CHECK".to_string() };
                if big(ui, &label, GREEN).on_hover_text("Every phone gets a big I READ YOU button. The list on the right counts the answers.").clicked() {
                    let l = crate::room::start_check();
                    note_record(&self.app, &format!("@{op}  {}", l.text));
                }
                let quiet = room.quiet;
                if ui
                    .add(egui::Button::new(RichText::new(if quiet { "\u{1f50a} Open the room" } else { "\u{1f507} Quiet the room" }).strong()).min_size(Vec2::new(170.0, 48.0)))
                    .on_hover_text("While the room is quiet only you can write in # main. HELP and private chats still work.")
                    .clicked()
                {
                    crate::room::set_quiet(!quiet);
                    note_record(&self.app, &format!("@{op}  {} the room", if quiet { "opened" } else { "quieted" }));
                }
            });
            ui.separator();
            // The room itself.
            let bottom = 56.0;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .max_height(ui.available_height() - bottom)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    if room.lines.is_empty() {
                        ui.label(RichText::new("Nothing yet. Say good morning, or call a comms check.").weak());
                    }
                    for l in &room.lines {
                        line_row(ui, &l.at, &l.nick, l.key == "op", l.kind, &l.text, l.key == "op", false);
                    }
                    let newest = room.lines.last().map(|l| l.id).unwrap_or(0);
                    if newest != self.last_line {
                        self.last_line = newest;
                        ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.draft)
                        .hint_text("Write to everybody")
                        .desired_width(ui.available_width() - 120.0)
                        .font(egui::TextStyle::Body),
                );
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let send = ui.add(
                    egui::Button::new(RichText::new("SEND").size(18.0).strong().color(Color32::WHITE))
                        .fill(GREEN)
                        .min_size(Vec2::new(100.0, 40.0)),
                );
                if send.clicked() || enter {
                    let text = std::mem::take(&mut self.draft);
                    let text = text.trim();
                    if let Some(t) = text.strip_prefix("/topic ") {
                        crate::room::set_topic(t);
                    } else if let Some(n) = text.strip_prefix("/nick ") {
                        crate::room::set_op_nick(n);
                        self.nick_draft.clear();
                    } else if !text.is_empty() {
                        if let (_, Some(l)) = crate::room::say("op", &op, text, "", true) {
                            append(&self.app, "messages.txt", &format!("#main  @{op}: {}", l.text));
                        }
                    }
                    r.request_focus();
                }
            });
        });
    }

    fn chat_windows(&mut self, ctx: &egui::Context) {
        let room = crate::room::snapshot();
        let convs = crate::chat::conversations(false);
        let area = ctx.available_rect();
        let whole = ctx.screen_rect();
        let mut i = 0;
        for w in self.wins.iter_mut() {
            if !w.open {
                continue;
            }
            let nick = room.people.iter().find(|p| p.key == w.key).map(|p| p.nick.clone()).unwrap_or_else(|| {
                convs.iter().find(|c| c.key == w.key).map(|c| c.label.clone()).unwrap_or_else(|| w.key.clone())
            });
            let help = convs.iter().any(|c| c.key == w.key && c.needs_talk);
            crate::chat::adult_opened(&w.key, false);
            let title = if help {
                RichText::new(format!("\u{270b} HELP  {nick}")).size(17.0).strong().color(RED)
            } else {
                RichText::new(format!("{nick}  (private)")).size(17.0).strong()
            };
            let mut open = true;
            let mut win = egui::Window::new(title)
                .id(egui::Id::new(("chatwin", w.key.clone())))
                .open(&mut open)
                .collapsible(true)
                .resizable(true)
                .default_size([340.0, 400.0])
                // Side by side from the right, over the lists, as mIRC laid
                // out query windows; each can be dragged anywhere after.
                .default_pos([(whole.right() - 20.0 - 355.0 * (i as f32 + 1.0)).max(whole.left() + 10.0), whole.top() + 110.0]);
            if w.big {
                win = win.fixed_rect(area.shrink(20.0));
            }
            win.show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Only you and this person see this chat.").small().weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(if w.big { "\u{2750} Make small" } else { "\u{25a1} Make big" }).clicked() {
                            w.big = !w.big;
                        }
                    });
                });
                ui.separator();
                let t = crate::chat::thread(&w.key, false);
                egui::ScrollArea::vertical()
                    .id_salt(("thread", w.key.clone()))
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .max_height(ui.available_height() - 50.0)
                    .show(ui, |ui| {
                        if t.is_empty() {
                            ui.label(RichText::new("Say hello.").weak());
                        }
                        for m in &t {
                            let (who, kind, text) = if m.kind == crate::chat::Kind::NeedToTalk {
                                (nick.clone(), crate::room::LineKind::Text, "\u{270b} HELP".to_string())
                            } else {
                                (if m.from_child { nick.clone() } else { crate::room::op_nick() }, crate::room::LineKind::Text, crate::chat::describe(m))
                            };
                            line_row(ui, &m.at, &who, !m.from_child, kind, &text, !m.from_child, true);
                        }
                    });
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut w.draft).hint_text(format!("Write to {nick}")).desired_width(ui.available_width() - 90.0));
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.add(egui::Button::new(RichText::new("SEND").strong().color(Color32::WHITE)).fill(GREEN)).clicked() || enter {
                        let text = std::mem::take(&mut w.draft);
                        if let Some(m) = crate::chat::from_adult(&w.key, &text, crate::chat::Kind::Text, false) {
                            append(&self.app, "messages.txt", &format!("teacher -> {}: {}", m.label, m.text));
                        }
                        r.request_focus();
                    }
                });
            });
            if !open {
                w.open = false;
            }
            i += 1;
        }
    }

    fn toast_cards(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }
        let mut open: Option<String> = None;
        let mut later: Option<String> = None;
        egui::Area::new(egui::Id::new("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, [-255.0, -80.0]).show(ctx, |ui| {
            for t in &self.toasts {
                card(ui, Color32::WHITE, if t.help { RED } else { GREEN }, |ui| {
                    ui.set_width(340.0);
                    if t.help {
                        ui.label(RichText::new(format!("\u{270b} {} asked for HELP", t.who)).size(18.0).strong().color(RED));
                    } else {
                        ui.label(RichText::new(format!("{} wrote to you", t.who)).strong());
                        ui.label(t.text.chars().take(160).collect::<String>());
                    }
                    ui.horizontal(|ui| {
                        if big(ui, "Open", GREEN_OK).clicked() {
                            open = Some(t.key.clone());
                        }
                        if plain(ui, "Later").clicked() {
                            later = Some(t.key.clone());
                        }
                    });
                });
                ui.add_space(6.0);
            }
        });
        if let Some(k) = open {
            self.open_chat(&k);
        }
        if let Some(k) = later {
            self.toasts.retain(|t| t.key != k);
        }
    }

    // ------------------------------------------------------------ who is connected

    fn who(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            heading(ui, "Who is connected");
            ui.label("Pause a device that misbehaves: it keeps the wifi but loses the class page, the files and the chat, until you let it back.");
            ui.add_space(6.0);
            let rows = self.app.class_rows();
            if rows.is_empty() {
                ui.label(RichText::new("Nobody is on the network yet.").weak());
            }
            let mut act: Option<(usize, Key)> = None;
            egui::ScrollArea::vertical().max_height(ui.available_height() - 140.0).show(ui, |ui| {
                egui::Grid::new("who").striped(true).num_columns(3).spacing([20.0, 10.0]).show(ui, |ui| {
                    for (i, r) in rows.iter().enumerate() {
                        ui.label(RichText::new(r.label.clone()).strong());
                        ui.label(RichText::new(r.state.clone()).color(if r.blocked { RED } else { Color32::DARK_GRAY }));
                        let label = if r.blocked { "Let back in" } else { "Pause" };
                        if plain(ui, label).clicked() {
                            act = Some((i, Key::Char(' ')));
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some((i, k)) = act {
                self.app.row = i;
                self.app.class_key(k);
            }
            ui.add_space(10.0);
            ui.separator();
            ui.label(RichText::new("Somebody keeps coming back under another name?").strong());
            ui.label("Change the wifi password. Everybody is knocked off, and only those you give the new password to can come back.");
            if matches!(self.app.screen, Screen::NewPassword) {
                ui.horizontal(|ui| {
                    ui.label("New password:");
                    let mut pw = self.app.new_password.clone();
                    if ui.add(egui::TextEdit::singleline(&mut pw).font(egui::TextStyle::Monospace).desired_width(200.0)).changed() {
                        self.app.new_password = pw.chars().take(63).collect();
                    }
                    if big(ui, "Change it now", RED).clicked() {
                        self.app.newpassword_key(Key::Enter);
                    }
                    if plain(ui, "Cancel").clicked() {
                        self.app.newpassword_key(Key::Esc);
                    }
                });
            } else if plain(ui, "Change the wifi password...").clicked() {
                self.app.class_key(Key::Char('p'));
            }
        });
    }

    // ------------------------------------------------------------ work handed in

    fn work(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            heading(ui, "Work handed in");
            let root = PathBuf::from(shellexpand(&self.app.folder));
            ui.label(format!(
                "Work a child sends waits here. Nothing lands in your folder until you accept it. Accepted work goes to {}",
                crate::page::handed_in_dir(&root).display()
            ));
            ui.add_space(6.0);
            let items = serve::pending();
            if items.is_empty() {
                ui.label(RichText::new("Nothing is waiting.").size(18.0).weak());
                return;
            }
            if big(ui, &format!("Accept all {} pieces", items.len()), GREEN).clicked() {
                self.app.row = 0;
                self.app.waiting_key(Key::Char('e'));
                return;
            }
            let mut act: Option<(usize, Key)> = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, p) in items.iter().enumerate() {
                    card(ui, Color32::WHITE, Color32::from_gray(200), |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(p.original.clone()).size(18.0).strong());
                            ui.label(RichText::new(format!("{}   from {}   at {}", human(p.bytes), p.from, p.at)).weak());
                        });
                        ui.horizontal_wrapped(|ui| {
                            if ui.add(egui::Button::new(RichText::new("Accept").strong().color(Color32::WHITE)).fill(GREEN_OK)).clicked() {
                                act = Some((i, Key::Char('a')));
                            }
                            if ui.button("Look at it first").clicked() {
                                act = Some((i, Key::Char('o')));
                            }
                            if ui.button("Accept everything from this person").clicked() {
                                act = Some((i, Key::Char('p')));
                            }
                            if ui.add(egui::Button::new(RichText::new("Refuse").color(RED))).clicked() {
                                act = Some((i, Key::Char('r')));
                            }
                        });
                    });
                    ui.add_space(6.0);
                }
            });
            if let Some((i, k)) = act {
                self.app.row = i;
                self.app.waiting_key(k);
            }
        });
    }

    // ------------------------------------------------------------ the join code

    fn join(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            heading(ui, "Join code, for phones with a working camera");
            ui.label("Phones without a camera join from their wifi list with the name and password on the left. This is only a shortcut.");
            ui.add_space(10.0);
            let join = self.app.hotspot.as_ref().and_then(|h| crate::qr::wifi_join(&h.ssid, &self.app.password));
            let page = self.app.page_url().and_then(|u| crate::qr::encode(u.as_bytes()));
            ui.horizontal_wrapped(|ui| {
                if let Some(c) = &join {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("1. Scan to join the wifi").size(20.0).strong());
                        draw_code(ui, c, 300.0);
                    });
                    ui.add_space(40.0);
                }
                if let Some(c) = &page {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(if join.is_some() { "2. Then scan to open the page" } else { "Scan to open the page" }).size(20.0).strong());
                        draw_code(ui, c, 300.0);
                        if let Some(u) = self.app.page_url() {
                            ui.label(RichText::new(u).size(18.0));
                        }
                    });
                }
            });
        });
    }
}

/// One line of the room or a private chat: time, name, words, in columns
/// like an IRC client, the words wrapping in their own column.
fn line_row(ui: &mut egui::Ui, at: &str, who: &str, op: bool, kind: crate::room::LineKind, text: &str, mine: bool, compact: bool) {
    use crate::room::LineKind;
    let fill = match kind {
        LineKind::Call => CALL_BG,
        _ if mine => MINE_BG,
        _ => Color32::TRANSPARENT,
    };
    let name_w = if compact { 95.0 } else { 120.0 };
    egui::Frame::new().fill(fill).corner_radius(4.0).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.add_sized([44.0, 20.0], egui::Label::new(RichText::new(at).monospace().weak()));
            ui.allocate_ui_with_layout(Vec2::new(name_w, 20.0), egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.set_width(name_w);
                if !who.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(format!("{}{who}", if op { "@" } else { "" })).strong().monospace().color(nick_colour(who)))
                            .truncate(),
                    );
                }
            });
            let t = RichText::new(text);
            let t = match kind {
                LineKind::Event => t.italics().weak(),
                LineKind::Call => t.strong(),
                LineKind::Answer => t.color(GREEN_OK).strong(),
                LineKind::Text if text.starts_with('\u{270b}') => t.color(RED).strong(),
                LineKind::Text => t,
            };
            ui.add(egui::Label::new(t).wrap());
        });
    });
}

fn draw_code(ui: &mut egui::Ui, code: &crate::qr::Code, px: f32) {
    let quiet = 2usize;
    let n = code.size + quiet * 2;
    let cell = (px / n as f32).floor().max(2.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(cell * n as f32), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Color32::WHITE);
    for r in 0..code.size {
        for c in 0..code.size {
            if code.dark(r, c) {
                let min = rect.min + Vec2::new((c + quiet) as f32 * cell, (r + quiet) as f32 * cell);
                p.rect_filled(egui::Rect::from_min_size(min, Vec2::splat(cell)), 0.0, Color32::BLACK);
            }
        }
    }
}

/// A line in the files beside the received work, as serve.rs writes them.
fn append(app: &App, file: &str, line: &str) {
    let dir = crate::page::handed_in_dir(&PathBuf::from(shellexpand(&app.folder)));
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(file)) {
            use std::io::Write as _;
            let _ = writeln!(f, "{}  {line}", crate::net::timestamp());
        }
    }
}

fn note_record(app: &App, line: &str) {
    append(app, "sign-ins.txt", line);
}

// ---------------------------------------------------------------- getting files

impl Gui {
    fn receive(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            heading(ui, "Get files from another computer");
            ui.label("The other computer must be running the hub and handing files out, on the same wifi or down a cable.");
            ui.add_space(8.0);
            let found = self.app.found.lock().unwrap_or_else(|e| e.into_inner()).clone();
            match &found {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Looking for computers handing files out...");
                    });
                }
                Some(list) if list.is_empty() => {
                    ui.label(RichText::new("None found on this network.").strong());
                }
                Some(list) => {
                    ui.label(RichText::new("Found:").strong());
                    let mut pick = None;
                    for (i, (ip, n, who)) in list.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let name = if who.is_empty() { ip.to_string() } else { format!("{who}  ({ip})") };
                            ui.label(RichText::new(name).size(18.0).strong());
                            ui.label(format!("{} files", count(*n)));
                            if big(ui, "Open", GREEN).clicked() {
                                pick = Some(i);
                            }
                        });
                    }
                    if let Some(i) = pick {
                        self.app.row = i;
                        self.app.receive_key(Key::Enter);
                    }
                }
            }
            ui.add_space(8.0);
            if plain(ui, "Look again").clicked() {
                self.app.start_looking();
            }
            ui.add_space(14.0);
            ui.label(RichText::new("Or type the address the other computer shows:").strong());
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.address).hint_text("for example 192.168.137.1").desired_width(260.0));
                if big(ui, "Open", GREEN).clicked() && !self.address.trim().is_empty() {
                    self.app.typed_address = self.address.trim().to_string();
                    self.app.open_typed();
                }
            });
            ui.add_space(14.0);
            if plain(ui, "Back to the start").clicked() {
                self.app.receive_key(Key::Esc);
            }
        });
    }

    fn receive_files(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("rf_top").show(ctx, |ui| {
            heading(ui, "Files on the other computer");
            egui::Grid::new("rf").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                ui.label(RichText::new("Save into").strong());
                ui.add(egui::TextEdit::singleline(&mut self.app.save_into).desired_width(420.0));
                ui.end_row();
                ui.label(RichText::new("Files at the same time").strong());
                ui.add(egui::DragValue::new(&mut self.app.files_at_once).range(1..=16));
                ui.end_row();
                ui.label(RichText::new("Connections for each file").strong());
                ui.add(egui::DragValue::new(&mut self.app.at_once).range(1..=32));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if big(ui, &format!("GET EVERYTHING ({} files)", count(self.app.files.len())), GREEN).clicked() {
                    self.app.files_key(Key::Char('a'));
                }
                if plain(ui, "Back").clicked() {
                    self.app.files_key(Key::Esc);
                }
            });
            ui.add_space(6.0);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            let files = self.app.files.clone();
            let mut get = None;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 34.0, files.len(), |ui, range| {
                for i in range {
                    ui.horizontal(|ui| {
                        if ui.button("Get").clicked() {
                            get = Some(i);
                        }
                        ui.label(files[i].name.clone());
                        ui.label(RichText::new(human(files[i].size)).weak());
                    });
                }
            });
            if let Some(i) = get {
                self.app.row = i + 3;
                self.app.files_key(Key::Enter);
            }
        });
    }

    fn receiving(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let batch = self.app.batch.lock().unwrap_or_else(|e| e.into_inner()).clone();
            heading(ui, if batch.is_some() { "Getting a folder" } else { "Getting a file" });
            if let Some(b) = &batch {
                let pct = if b.total > 0 { b.done as f32 / b.total as f32 } else { 0.0 };
                ui.label(format!("File {} of {}", b.done.min(b.total), b.total));
                ui.add(egui::ProgressBar::new(pct).show_percentage().desired_height(28.0));
                for name in b.in_flight.iter().take(6) {
                    ui.label(RichText::new(name.clone()).weak());
                }
                let secs = self.app.since.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0).max(0.001);
                ui.label(format!("{} so far, {:.1} MB/s", human(b.bytes), b.bytes as f64 / secs / 1e6));
                if !b.failed.is_empty() {
                    ui.label(RichText::new(format!("{} did not arrive", b.failed.len())).color(RED));
                }
            } else {
                let (done, total) = crate::fetch::progress();
                ui.label(self.app.downloading.clone().unwrap_or_default());
                let pct = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(pct).show_percentage().desired_height(28.0));
                ui.label(format!("{} of {}", human(done), human(total)));
            }
            ui.add_space(12.0);
            if big(ui, "Stop", RED).clicked() {
                self.app.receiving_key(Key::Esc);
            }
        });
    }

    // ------------------------------------------------------------ fixing the computer

    fn checkup(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            heading(ui, "Fix problems with this computer");
            ui.label("Each button checks one thing and puts it right. Windows may ask permission: say Yes.");
            ui.add_space(10.0);
            if let Some(off) = self.app.services_off() {
                if off.is_empty() {
                    ui.label(RichText::new("\u{2714} Everything the hub needs from Windows is switched on.").color(GREEN_OK));
                } else {
                    ui.label(RichText::new(format!("Switched off here: {}", off.join(", "))).color(RED).strong());
                }
            }
            ui.add_space(8.0);
            let items = self.app.checkup_items();
            let mut run = None;
            for (i, it) in items.iter().enumerate() {
                card(ui, Color32::WHITE, GREEN, |ui| {
                    ui.set_width(ui.available_width().min(700.0));
                    ui.label(RichText::new(*it).size(18.0).strong());
                    if big(ui, "Check and fix", GREEN).clicked() {
                        run = Some(i);
                    }
                });
                ui.add_space(8.0);
            }
            if let Some(i) = run {
                self.app.row = i;
                self.app.checkup_key(Key::Enter);
            }
            if plain(ui, "Back to the start").clicked() {
                self.app.checkup_key(Key::Esc);
            }
        });
    }

    fn fixoffer(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            heading(ui, "This computer is not ready yet");
            ui.label("Parts of Windows the hub needs are switched off here:");
            for name in self.app.services_off().unwrap_or_default() {
                ui.label(RichText::new(format!("   {name}")).strong());
            }
            ui.add_space(6.0);
            ui.label("Without them the wifi network or the cable will not work, and Windows will not say why. Nothing is broken; this is common on laptops that have been \"sped up\".");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if big(ui, "Switch them on", GREEN).clicked() {
                    self.app.fixoffer_key(Key::Enter);
                }
                if plain(ui, "Carry on without").clicked() {
                    self.app.fixoffer_key(Key::Esc);
                }
            });
        });
    }
}

/// The guide behind Help, in the words the terminal's help used, for the
/// window's buttons instead of its keys.
const GUIDE: &str = "HANDING OUT FILES

1. Press \"Hand out files to the class\" and open the folder with the lesson in it.
2. Press the green button. The wifi network starts and everything in the folder is handed out. To hand out only some files, tick them first.
3. Write the network name and the password on the board. They are on the left of the lesson screen.
4. Children join that network from their phone's wifi list and type the password. The class page opens by itself and asks for their name.

DURING THE LESSON

Class chat: everybody writes in # main and everybody reads it. You are @Teacher.
- COMMS CHECK: every phone gets a big I READ YOU button, and the list on the right counts the answers. Press it again to call only those who have not answered.
- Quiet the room: only you can write in # main. HELP and private chats still work.
- Click a name on the right to open a private chat, mute that person in # main, or remove their device.
- A child who taps HELP turns red at the top of the window, with a sound. Open it, answer them, and find a safe, private moment to talk.
- Private chats open as windows. Drag them, make them big, fold them away, close them; the chat stays in the list on the left.

Who is connected: pause a device that misbehaves, or change the wifi password to knock everybody off.
Work handed in: accept or refuse what the class sends. Nothing lands in your folder until you accept it.
Files the class can see: tick or untick files while the class watches.
Join code: codes to scan, for phones whose camera works.
The notice: one line shown at the top of every phone's page.

STOP HANDING OUT switches the wifi network off. Closing the window asks first.

IF SOMETHING GOES WRONG

A phone cannot see the network: stand closer; walls and metal block wifi. Keep the band on 2.4 GHz.
The page does not open by itself: the phone can type the address shown on the left.
The first time on a computer: use Computer, Fix problems with this computer.

Everything said in the class chat is written to messages.txt, and every sign-in, comms check and removed device to sign-ins.txt, in the received folder.";

// ---------------------------------------------------------------- the tour

/// Each step's picture name; tour_setup says what happens before it.
const TOUR: &[&str] = &[
    "01-start",
    "02-choose-folder",
    "03-settings",
    "04-check-files",
    "05-lesson-chat",
    "06-lesson-help-and-card",
    "07-lesson-two-private-windows",
    "08-lesson-private-made-big",
    "09-who-is-connected",
    "10-work-handed-in",
    "11-files-the-class-sees",
    "12-join-code",
    "13-stop-question",
    "14-stopped",
    "15-get-files",
    "16-fix-this-computer",
    "17-not-ready-offer",
];

impl Gui {
    fn run_tour(&mut self, ctx: &egui::Context) {
        let Some(t) = &mut self.tour else { return };
        ctx.request_repaint();
        if t.asked {
            let image = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(img) = image {
                let _ = std::fs::create_dir_all(&t.dir);
                let _ = save_bmp(&t.dir.join(format!("{}.bmp", TOUR[t.step])), &img);
                t.asked = false;
                t.step += 1;
                t.frames = 0;
            }
            return;
        }
        if t.step >= TOUR.len() {
            self.tour = None;
            self.close_ok = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if t.frames == 0 {
            let step = t.step;
            self.tour_setup(step);
        }
        let t = self.tour.as_mut().expect("tour");
        t.frames += 1;
        // Long enough for layout to settle and the chat to be read in.
        if t.frames > 12 {
            t.asked = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }

    fn tour_setup(&mut self, step: usize) {
        let lesson = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("Documents")
            .join("Year 7 science");
        let _ = std::fs::create_dir_all(&lesson);
        for (n, text) in [
            ("Fractions worksheet.txt", "1/2 + 1/4 = ?"),
            ("Times tables.txt", "2 x 2 = 4"),
            ("Reading - The Lion.txt", "Once upon a time"),
        ] {
            let p = lesson.join(n);
            if !p.exists() {
                let _ = std::fs::write(p, text);
            }
        }
        let kids = [
            ("tag:amina01", "Amina", "192.168.137.21"),
            ("tag:kofi0002", "Kofi", "192.168.137.22"),
            ("tag:joseph03", "Joseph", "192.168.137.23"),
            ("tag:fatima04", "Fatima", "192.168.137.24"),
            ("tag:tendai05", "Tendai", "192.168.137.25"),
        ];
        match TOUR[step] {
            "02-choose-folder" => {
                self.app.folder = lesson.to_string_lossy().into_owned();
                self.app.cable = false;
                self.app.quick = true;
                self.app.open_picker();
            }
            "03-settings" => {
                self.app.quick = false;
                self.app.ssid = "Gorilla Hub".into();
                self.app.password = "kx7m4pq9".into();
                self.app.screen = Screen::Send;
            }
            "04-check-files" => self.app.open_tick(true),
            "05-lesson-chat" => {
                // A cable lesson: the same lesson screen, without switching
                // this laptop's wifi into a hotspot for a picture.
                self.app.cable = true;
                self.app.ssid.clear();
                self.app.password.clear();
                self.app.chosen = None;
                self.app.open_tick(true);
                self.app.apply_ticks();
                self.app.begin_sending();
                crate::room::set_topic("Lesson 3: fractions. The worksheet is in # files.");
                for (k, n, ip) in kids {
                    crate::room::here(k, n, ip);
                }
                crate::room::say("op", "Teacher", "Good morning everybody. Comms check coming.", "", true);
                crate::room::start_check();
                for (k, n, _) in &kids[..4] {
                    crate::room::answer(k, n);
                }
                crate::room::say("tag:kofi0002", "Kofi", "good morning teacher", "a", false);
                crate::room::say("tag:fatima04", "Fatima", "morning! I read you 5/5", "b", false);
                crate::room::say("op", "Teacher", "Open # files and start question 1.", "", true);
                crate::room::say("tag:amina01", "Amina", "got it", "c", false);
            }
            "06-lesson-help-and-card" => {
                crate::chat::from_child("tag:joseph03", "Joseph [an Android phone]", "", crate::chat::Kind::NeedToTalk, false, "h1");
                crate::chat::from_child(
                    "tag:kofi0002",
                    "Kofi [an Android phone]",
                    "the worksheet does not open on my phone",
                    crate::chat::Kind::Text,
                    false,
                    "k1",
                );
            }
            "07-lesson-two-private-windows" => {
                self.open_chat("tag:joseph03");
                crate::chat::from_adult("tag:joseph03", "Joseph, I am coming to your desk.", crate::chat::Kind::Text, false);
                self.open_chat("tag:kofi0002");
                crate::chat::from_adult("tag:kofi0002", "Try the READ button instead of GET IT.", crate::chat::Kind::Text, false);
            }
            "08-lesson-private-made-big" => {
                if let Some(w) = self.wins.first_mut() {
                    w.big = true;
                }
            }
            "09-who-is-connected" => {
                for w in self.wins.iter_mut() {
                    w.big = false;
                    w.open = false;
                }
                self.app.joined = kids
                    .iter()
                    .map(|(_, _, ip)| net::Joined { ip: ip.parse().expect("ip"), name: None })
                    .collect();
                for (_, n, ip) in kids {
                    serve::set_claimed_name(ip, n);
                }
                serve::block_device("192.168.137.25");
                self.go(Tab::Who);
            }
            "10-work-handed-in" => {
                serve::note_pending("192.168.137.21", "Amina fractions.jpg", "tour-1.jpg", 812_000);
                serve::note_pending("192.168.137.22", "question 1.txt", "tour-2.txt", 1_200);
                serve::note_pending("192.168.137.24", "my drawing.png", "tour-3.png", 2_400_000);
                self.go(Tab::Work);
            }
            "11-files-the-class-sees" => self.go(Tab::Files),
            "12-join-code" => self.go(Tab::Join),
            "13-stop-question" => {
                serve::unblock_device("192.168.137.25");
                self.go(Tab::Chat);
                self.confirm_stop = true;
            }
            "14-stopped" => {
                self.confirm_stop = false;
                self.wins.clear();
                self.toasts.clear();
                self.app.sending_key(Key::Char('q'));
            }
            "15-get-files" => {
                self.app.screen = Screen::Home;
                self.app.row = 2;
                self.app.home_key(Key::Enter);
            }
            "16-fix-this-computer" => self.app.screen = Screen::Checkup,
            "17-not-ready-offer" => self.app.screen = Screen::FixOffer,
            _ => {}
        }
    }
}

/// A picture as a plain 32-bit BMP: no image library needed for a test tool.
fn save_bmp(path: &Path, img: &egui::ColorImage) -> std::io::Result<()> {
    let (w, h) = (img.size[0] as u32, img.size[1] as u32);
    let data = w * h * 4;
    let mut out = Vec::with_capacity(54 + data as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + data).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(-(h as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&data.to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for p in &img.pixels {
        out.extend_from_slice(&[p.b(), p.g(), p.r(), 255]);
    }
    std::fs::write(path, out)
}
