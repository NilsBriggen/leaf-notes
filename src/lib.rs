pub mod ai;
pub mod math;
pub mod model;
pub mod rich;
pub mod hw;
mod export;
mod audio;
pub(crate) mod canvas;
mod chrome;
mod settings;
pub mod sync;
pub mod storage;
pub mod index;
pub mod vault;
mod locks;
mod welcome;
mod transcribe;
mod updates;
#[cfg(target_os = "android")]
mod droid;
mod safety;
pub mod headless;
pub mod i18n;
mod i18n_table;
pub(crate) mod state;
pub(crate) use i18n::tr;
use state::*;

/// Frames the window has drawn in a `--screenshot` run (none means the system isn't giving it any).
pub static SHOT_FRAMES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Files given on the command line, opened onto the page once the window is up.
pub static OPEN_AT_START: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
/// Android: something is pending, so the wake-up thread repaints the app regularly.
#[cfg(target_os = "android")]
static WAKE_FAST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// Android: lifecycle flag so the wake-up thread exits cleanly when the activity stops.
#[cfg(target_os = "android")]
static ANDROID_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
mod keys;

use eframe::egui::{self, Color32, CornerRadius, Key, Modifiers, Pos2, Stroke as EStroke, Vec2};
use model::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

#[cfg(target_os = "android")]
pub static ANDROID_DATA: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

const SETTINGS_FILE: &str = "ai-settings.json";
const PREFS_FILE: &str = "prefs.json";

pub(crate) fn c3(c: [u8; 3]) -> Color32 { Color32::from_rgb(c[0], c[1], c[2]) }
pub(crate) fn c4(c: [u8; 4]) -> Color32 { Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]) }
/// The person's accent colour (teal unless changed in Settings).
static ACCENT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x0F766E);
pub(crate) fn accent_base() -> Color32 { let v = ACCENT.load(std::sync::atomic::Ordering::Relaxed); Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8) }

/// All tooltips go through `tip`: one without text would show as an empty bubble, so it's skipped.
pub(crate) trait Tip { fn tip(self, text: impl Into<egui::WidgetText>) -> Self; }
impl Tip for egui::Response {
    fn tip(self, text: impl Into<egui::WidgetText>) -> Self {
        let text = text.into();
        let translated = tr(text.text()).to_string();
        if translated.trim().is_empty() { self } else if translated != text.text() { self.on_hover_text(translated) } else { self.on_hover_text(text) }
    }
}

pub(crate) fn accent(ui: &egui::Ui) -> Color32 { if ui.visuals().dark_mode { mix(accent_base(), Color32::WHITE, 0.45) } else { accent_base() } }
fn mix(a: Color32, b: Color32, t: f32) -> Color32 { a.lerp_to_gamma(b, t) }

/// Every window looks like Settings: a rounded card with a serif title.
#[allow(dead_code)]
pub(crate) fn window_frame(ctx: &egui::Context) -> egui::Frame { egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16)).corner_radius(16) }
#[allow(dead_code)]
pub(crate) fn window_title(t: impl Into<String>) -> egui::RichText { egui::RichText::new(t.into()).font(rich::serif(19.0)) }

pub const ACCENTS: [(&str, [u8; 3]); 6] = [("Teal", [15, 118, 110]), ("Indigo", [67, 56, 202]), ("Coral", [214, 82, 62]), ("Forest", [21, 128, 61]), ("Plum", [147, 51, 134]), ("Slate", [51, 65, 85])];

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum ZoomReq { By(f32), To(f32), FitWidth }

/// Which kind of input started the current gesture.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Input { Mouse, Finger, Pen }

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tab { Home, Insert, Draw, Ai, Study, View }

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Tool { Type, Pen, Highlighter, Eraser, Lasso, Shape(Shape) }

/// A formatting command from the ribbon, applied by the text note it targets.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Fmt { Wrap(&'static str), Prefix(&'static str), Heading(u8) }

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Rewrite { Replace, Append }

/// How a chosen file goes onto the page.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Ins { Picture, Printout, Attach }

/// Settings that follow the person between sessions.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub(crate) struct Prefs {
    pub dark: bool,
    pub zoom: f32,
    pub show_nav: bool,
    pub show_ai: bool,
    pub ribbon_open: bool,
    pub pens: Vec<Pen>,
    pub pen: usize,
    pub stabilizer: f32,
    pub pressure_curve: f32,
    pub prompts: String,
    pub auto_backup: bool,
    pub last_backup: u64,
    pub snap_shapes: bool,
    pub snap_delay: f32,
    pub speed: f32,
    pub lock_after_min: u32,
    pub last_id: u64,
    pub views: HashMap<u64, [f32; 5]>,
    pub skip_version: String,
    pub last_page: HashMap<u64, u64>,
    pub recent_colors: Vec<[u8; 4]>,
    pub hl_color: [u8; 4],
    pub erase_partial: bool,
    pub eraser_size: f32,
    pub hl_width: f32,
    pub ink_ai: bool,
    pub last: (usize, usize, usize),
    pub follow_system: bool,
    pub accent: [u8; 3],
    pub note_size: f32,
    pub line: f32,
    pub serif: bool,
    pub new_paper: Paper,
    pub draw_mouse: bool,
    pub draw_finger: bool,
    pub pressure: bool,
    pub straighten_hl: bool,
    pub scratch_out: bool,
    pub click_to_type: bool,
    pub auto_lists: bool,
    pub note_width: f32,
    pub favorites: Vec<u64>,
    pub recent: Vec<u64>,
    pub mic_ok: bool,
    pub pen_detect: bool,
    pub lang: String,
    pub seen_version: String,
    pub high_contrast: bool,
    pub check_updates: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            dark: false, zoom: 1.0, show_nav: true, show_ai: false, ribbon_open: true, pens: default_pens(), pen: 0,
            stabilizer: 0.35, pressure_curve: 1.0, recent_colors: vec![], last_page: HashMap::new(), snap_shapes: true,
            snap_delay: 0.55, prompts: String::new(), auto_backup: true, last_backup: 0, speed: 1.0, lock_after_min: 10,
            skip_version: String::new(), last_id: 0, views: HashMap::new(), hl_color: [253, 224, 71, 120],
            erase_partial: true, eraser_size: 8.0, hl_width: 16.0, ink_ai: false, last: (0, 0, 0),
            follow_system: false, accent: [15, 118, 110], note_size: 15.0, line: 1.55, serif: true, new_paper: Paper::Plain,
            draw_mouse: false, draw_finger: false, pressure: true, straighten_hl: true, scratch_out: true,
            click_to_type: true, auto_lists: true, note_width: 480.0, favorites: vec![], recent: vec![], mic_ok: false,
            pen_detect: false, lang: String::new(), seen_version: String::new(), high_contrast: false, check_updates: true,
        }
    }
}

/// A Google sign-in waiting for the browser: the address it opened, the result (refresh token, email), and where a pasted address goes.
pub(crate) type GoogleWait = (String, Receiver<Result<(String, String), String>>, std::sync::mpsc::Sender<String>);

/// The deck made from a page's notes (found again by this name to add more cards).
fn deck_name(title: &str) -> String { tf!("{title} — cards", title = title) }

/// What copied ink puts on the clipboard (readable if pasted somewhere else), so Ctrl+V can paste it back.
const INK_MARK: &str = "(Leaf Notes drawing)";
fn is_ink_mark(t: &str) -> bool { let t = t.trim(); t == INK_MARK || t == tr(INK_MARK) }

/// `Page` has no `PartialEq`; its JSON is stable (no unordered maps inside).
pub(crate) fn same_page(a: &Page, b: &Page) -> bool { serde_json::to_vec(a).ok() == serde_json::to_vec(b).ok() }

pub struct NotesApp {
    store: Store,
    st: St,
    nb: usize,
    sec: usize,
    pg: usize,
    screenshot: Option<PathBuf>,
    frames: u32,
    shot_requested: bool,
    shot_done: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(target_os = "android")]
    android: Option<android_activity::AndroidApp>,
    #[cfg(target_os = "android")]
    keyboard_shown: bool,
}

/// Use the platform's own UI font (egui's as fallback), a bold face for **bold**, and math-capable fallbacks.
fn setup_fonts(ctx: &egui::Context) {
    const REGULAR: &[&str] = &["C:\\Windows\\Fonts\\segoeui.ttf", "/system/fonts/Roboto-Regular.ttf", "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/NotoSans-Regular.ttf", "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", "/usr/share/fonts/google-noto/NotoSans-Regular.ttf"];
    const BOLD: &[&str] = &["C:\\Windows\\Fonts\\segoeuib.ttf", "/system/fonts/Roboto-Bold.ttf", "/usr/share/fonts/noto/NotoSans-Bold.ttf",
        "/usr/share/fonts/TTF/NotoSans-Bold.ttf", "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf", "/usr/share/fonts/google-noto/NotoSans-Bold.ttf",
        "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"];
    const MATH: &[&str] = &["/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "C:\\Windows\\Fonts\\seguisym.ttf",
        "/system/fonts/NotoSansSymbols-Regular-Subsetted.ttf"];
    const MATH2: &[&str] = &["/usr/share/fonts/noto/NotoSansMath-Regular.ttf", "/usr/share/fonts/TTF/NotoSansMath-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansMath-Regular.ttf", "C:\\Windows\\Fonts\\cambria.ttc", "/system/fonts/NotoSansMath-Regular.otf"];
    let read = |paths: &[&str]| paths.iter().find_map(|p| std::fs::read(p).ok());
    let mut defs = egui::FontDefinitions::default();
    let mut prop = defs.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    let add = |defs: &mut egui::FontDefinitions, name: &str, bytes: Vec<u8>| { defs.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(bytes))); };
    if let Some(b) = read(REGULAR) { add(&mut defs, "ui", b); prop.insert(0, "ui".into()); }
    for (i, list) in [MATH, MATH2].iter().enumerate() {
        if let Some(b) = read(list) { let n = format!("math{i}"); add(&mut defs, &n, b); prop.push(n); }
    }
    if let Some(b) = read(BOLD) {
        add(&mut defs, "bold", b);
        let mut fam = vec!["bold".to_string()];
        fam.extend(prop.iter().cloned());
        defs.families.insert(egui::FontFamily::Name("bold".into()), fam);
        rich::HAS_BOLD.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    const SERIF: &[&str] = &["C:\\Windows\\Fonts\\georgia.ttf", "/system/fonts/NotoSerif-Regular.ttf", "/usr/share/fonts/noto/NotoSerif-Medium.ttf",
        "/usr/share/fonts/noto/NotoSerif-Regular.ttf", "/usr/share/fonts/truetype/noto/NotoSerif-Regular.ttf", "/usr/share/fonts/google-noto/NotoSerif-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSerif.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf"];
    if let Some(b) = read(SERIF) {
        add(&mut defs, "serif", b);
        let mut fam = vec!["serif".to_string()];
        fam.extend(prop.iter().cloned());
        defs.families.insert(egui::FontFamily::Name("serif".into()), fam);
        rich::HAS_SERIF.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    defs.families.insert(egui::FontFamily::Proportional, prop);
    ctx.set_fonts(defs);
}

/// A dialog's width, never wider than the screen.
pub(crate) fn fit_w(ctx: &egui::Context, w: f32) -> f32 { w.min(ctx.content_rect().width() - 72.0).max(160.0) }

/// Copy a picture into the app, upright and at most 2400 px on its long side.
fn store_picture(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<u64> {
    use image::ImageDecoder;
    let fixed = (|| -> Option<image::DynamicImage> {
        let mut dec = image::ImageReader::open(src).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
        let turn = dec.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
        let mut img = image::DynamicImage::from_decoder(dec).ok()?;
        let big = img.width().max(img.height()) > 2400;
        if !big && turn == image::metadata::Orientation::NoTransforms { return None; }
        img.apply_orientation(turn);
        if big { img = img.resize(2400, 2400, image::imageops::FilterType::Triangle); }
        Some(img)
    })();
    match fixed {
        Some(img) if matches!(dest.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(), Some("jpg" | "jpeg")) =>
            image::DynamicImage::ImageRgb8(img.to_rgb8()).save(dest).map_err(std::io::Error::other)?,
        Some(img) => img.save(dest).map_err(std::io::Error::other)?,
        None => { std::fs::copy(src, dest)?; }
    }
    std::fs::metadata(dest).map(|m| m.len())
}

fn where_saved(path: &std::path::Path) -> String {
    if cfg!(target_os = "android") { "Downloads/Leaf Notes".into() } else { path.display().to_string() }
}

fn shared(path: &std::path::Path) {
    #[cfg(target_os = "android")]
    droid::share(path);
    let _ = path;
}

pub(crate) fn card(ui: &egui::Ui) -> egui::Frame {
    let v = ui.visuals();
    egui::Frame::new().fill(v.extreme_bg_color).corner_radius(14).stroke(v.widgets.noninteractive.bg_stroke)
        .shadow(egui::Shadow { offset: [0, 2], blur: 14, spread: 0, color: Color32::from_black_alpha(if v.dark_mode { 70 } else { 14 }) })
}

pub(crate) fn apply_prefs(ctx: &egui::Context, p: &Prefs) {
    i18n::set(&p.lang);
    use std::sync::atomic::Ordering::Relaxed;
    ACCENT.store(u32::from_be_bytes([0, p.accent[0], p.accent[1], p.accent[2]]), Relaxed);
    rich::LINE_X100.store((p.line.clamp(1.2, 2.0) * 100.0) as u32, Relaxed);
    rich::USE_SERIF.store(p.serif, Relaxed);
    HIGH_CONTRAST.store(p.high_contrast, Relaxed);
    apply_theme(ctx, p.dark);
}

pub(crate) fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    if dark {
        v.panel_fill = Color32::from_rgb(17, 20, 21);
        v.window_fill = Color32::from_rgb(29, 34, 35);
        v.extreme_bg_color = Color32::from_rgb(26, 30, 31);
        v.faint_bg_color = Color32::from_rgb(34, 40, 41);
        v.code_bg_color = Color32::from_rgb(34, 40, 41);
        let a = mix(accent_base(), Color32::WHITE, 0.45);
        v.selection.bg_fill = mix(accent_base(), Color32::from_rgb(17, 20, 21), 0.55);
        v.selection.stroke = EStroke::new(1.0, a);
        v.hyperlink_color = a;
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(44, 55, 56);
        v.widgets.hovered.bg_stroke = EStroke::new(1.0, Color32::from_rgb(88, 104, 106));
        v.widgets.inactive.bg_fill = Color32::from_rgb(42, 50, 52);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 48, 50);
        v.widgets.noninteractive.bg_stroke = EStroke::new(1.0, Color32::from_rgb(62, 72, 74));
    } else {
        v.panel_fill = Color32::from_rgb(236, 240, 238);
        v.window_fill = Color32::WHITE;
        v.extreme_bg_color = Color32::WHITE;
        v.faint_bg_color = Color32::from_rgb(245, 248, 247);
        v.code_bg_color = Color32::from_rgb(242, 245, 244);
        v.selection.bg_fill = mix(accent_base(), Color32::WHITE, 0.82);
        v.selection.stroke = EStroke::new(1.0, accent_base());
        v.hyperlink_color = accent_base();
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(226, 234, 231);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(238, 242, 240);
        v.widgets.noninteractive.bg_stroke = EStroke::new(1.0, Color32::from_rgb(225, 231, 228));
        v.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(52, 64, 62);
        v.widgets.inactive.fg_stroke.color = Color32::from_rgb(40, 52, 50);
    }
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(8);
    }
    v.widgets.inactive.bg_stroke = if dark { EStroke::new(1.0, Color32::from_rgb(62, 72, 74)) } else { EStroke::NONE };
    v.text_edit_bg_color = Some(if dark { Color32::from_rgb(38, 45, 46) } else { Color32::from_rgb(240, 244, 242) });
    v.window_corner_radius = CornerRadius::same(16);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = egui::Shadow { offset: [0, 10], blur: 32, spread: 0, color: Color32::from_black_alpha(if dark { 130 } else { 34 }) };
    v.popup_shadow = egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(if dark { 110 } else { 28 }) };
    v.window_stroke = EStroke::new(1.0, if dark { Color32::from_rgb(44, 52, 53) } else { Color32::from_rgb(225, 231, 228) });
    v.window_fill = v.extreme_bg_color;
    if HIGH_CONTRAST.load(std::sync::atomic::Ordering::Relaxed) {
        let (fg, weak, line) = if dark { (Color32::WHITE, Color32::from_gray(215), Color32::from_gray(160)) } else { (Color32::BLACK, Color32::from_gray(45), Color32::from_gray(85)) };
        v.override_text_color = Some(fg);
        v.weak_text_color = Some(weak);
        v.widgets.noninteractive.bg_stroke = EStroke::new(1.5, line);
        v.widgets.inactive.bg_stroke = EStroke::new(1.0, line);
        v.window_stroke = EStroke::new(1.5, line);
        v.selection.stroke = EStroke::new(2.0, v.selection.stroke.color);
    }
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals(v);
}

fn instance_lock() -> Result<Option<std::fs::File>, ()> {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(f) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(".lock")) else { return Ok(None) };
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(std::fs::TryLockError::WouldBlock) => Err(()),
        Err(_) => Ok(None),
    }
}

/// Converted handwriting waiting for "replace", "keep both" or "cancel".
pub(crate) struct InkReview { pub math: bool, pub page: u64, pub strokes: Vec<u64>, pub at: [f32; 2], pub width: f32, pub text: String }

/// Ready-made pages: (name, paper, content in Markdown).
pub(crate) const TEMPLATES: [(&str, Paper, &str); 3] = [
    ("Lecture notes", Paper::Cornell, "## Key terms\n- \n\n## Notes\n\n\n## Questions\n- [ ] \n\n## Summary\n"),
    ("Problem set", Paper::Grid, "## Problems\n- [ ] 1.\n- [ ] 2.\n- [ ] 3.\n\n## Working\n"),
    ("Weekly plan", Paper::Plain, "## Monday\n- [ ] \n## Tuesday\n- [ ] \n## Wednesday\n- [ ] \n## Thursday\n- [ ] \n## Friday\n- [ ] \n## Weekend\n- [ ] \n"),
];

fn rec_marker(id: u64) -> PathBuf { data_dir().join("files").join(format!("{id}.rec.part")) }

pub(crate) static HIGH_CONTRAST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Width threshold below which Nav and AI panes act as full-width drawers covering the canvas.
pub(crate) const DRAWER_COVER_W: f32 = 600.0;

impl NotesApp {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>) -> Self {
        setup_fonts(&cc.egui_ctx);
        let persist = screenshot.is_none();
        let instance = if persist { instance_lock() } else { Ok(None) };
        let (mut store, load_msg, store_ok) = if !persist { (sample(), None, false) } else {
            match storage::load(&data_dir()) {
                Ok(storage::Loaded::Fresh) => (sample(), None, true),
                Ok(storage::Loaded::Store(s, damaged)) if damaged.is_empty() => (s, None, true),
                Ok(storage::Loaded::Store(s, damaged)) => (s, Some(tf!("⚠ {n} page(s) couldn't be read and are shown empty: {list}. Older versions are in ⋯ ▸ Page history.", n = damaged.len(), list = damaged.join(", "))), true),
                Err(e) => (sample(), Some(tf!("⚠ Your notes couldn't be read ({e}). Nothing will be saved until this is fixed, so your files stay untouched.", e = e)), false),
            }
        };
        model::rekey_study(&mut store);
        #[cfg(target_os = "android")]
        if persist { let _ = std::fs::remove_dir_all(data_dir().join("exports")); }
        let (load_msg, store_ok) = if instance.is_err() {
            (Some(tr("⚠ Leaf Notes is already open in another window. This window shows your notes but doesn't save them.").to_string()), false)
        } else {
            (load_msg, store_ok)
        };
        let mut ai: ai::AiSettings = if persist { load(SETTINGS_FILE).unwrap_or_default() } else { ai::AiSettings::default() };
        let fresh_install = persist && !data_dir().join(PREFS_FILE).exists() && !data_dir().join(storage::LIBRARY).exists();
        let p: Prefs = if persist { load(PREFS_FILE).unwrap_or_default() } else { Prefs::default() };
        let mut sync: sync::SyncSettings = if persist { load(sync::SYNC_FILE).unwrap_or_default() } else { Default::default() };
        if persist {
            keys::load("ai-key", &mut ai.api_key);
            keys::load("webdav-password", &mut sync.password);
            keys::load("google-token", &mut sync.google_token);
        }
        apply_prefs(&cc.egui_ctx, &p);
        cc.egui_ctx.all_styles_mut(|s| {
            s.spacing.item_spacing = Vec2::new(8.0, 6.0);
            s.spacing.button_padding = Vec2::new(10.0, 5.0);
            s.spacing.menu_margin = egui::Margin::same(8);
            s.spacing.interact_size.y = 26.0;
            s.spacing.window_margin = egui::Margin::same(16);
        });
        if (p.zoom - 1.0).abs() > 0.01 { cc.egui_ctx.set_zoom_factor(p.zoom.clamp(0.6, 2.5)); }
        let (nb, sec, pg) = p.last;
        let st = St {
            ai,
            p,
            tab: Tab::Home,
            modals: ModalStack::default(),
            canvas: CanvasState::default(),
            nav: NavState::default(),
            jobs: JobsState::default(),
            audio: AudioState::default(),
            sync_st: SyncState::new(sync),
            locks: LockState::new(),
            cache: CacheState::default(),
            focus: None,
            undo: vec![],
            redo: vec![],
            dirty: false,
            since_save: 0.0,
            prev_t: 0.0,
            status: String::new(),
            status_age: 0.0,
            save_worker: (persist && store_ok).then(storage::SaveWorker::start),
            index: index::Index::load(persist && store_ok),
            full_save: true,
            full_at: 0.0,
            instance: instance.ok().flatten(),
            bars: [0.0; 4],
            bars_at: -10.0,
            picking_since: -1e9,
            kb_idle_since: None,
            opened_args: false,
            theme_dirty: false,
            persist: persist && store_ok,
        };
        let mut app = NotesApp {
            store, st, nb, sec, pg, screenshot, frames: 0, shot_requested: false, shot_done: Default::default(),
            #[cfg(target_os = "android")] android: None,
            #[cfg(target_os = "android")] keyboard_shown: false,
        };
        app.clamp();
        app.st.dirty = app.st.persist;
        if let Some(m) = load_msg { app.st.say(m); app.st.status_age = -50.0; }
        app.st.audio.speed = app.st.p.speed.clamp(0.5, 2.0);
        if let Some((a, b, c)) = app.locate(app.st.p.last_id) { (app.nb, app.sec, app.pg) = (a, b, c); }
        for (id, v) in app.st.p.views.clone() {
            app.st.canvas.views.insert(id, egui::Rect::from_min_size(Pos2::new(v[0], v[1]), Vec2::new(v[2], v[3])));
            app.st.canvas.scales.insert(id, v[4].clamp(0.3, 4.0));
        }
        if app.st.persist {
            app.recover_recordings();
            app.greet(fresh_install);
            welcome::install_crash_hook();
            if let Some(path) = welcome::crash_reports().into_iter().next() {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                app.st.modals.push(ActiveModal::Crash { path, text });
            }
        }
        if app.st.persist && app.st.p.check_updates {
            app.st.jobs.update_rx = Some(updates::check(welcome::VERSION, app.st.p.skip_version.clone()));
        }
        if !persist {
            app.st.canvas.last_text = app.page().blocks.get(1).map(|b| b.id);
            app.st.jobs.ai_log = vec![
                (false, "Summarize this page".into()),
                (true, "**Fourier series** in three points:\n- Periodic signals are sums of sines and cosines\n- Coefficients: $a_n = \\frac{2}{T}\\int_0^T f(t)\\cos(n\\omega t)\\,dt$\n- A square wave uses only odd harmonics".into()),
            ];
        }
        app
    }

    #[cfg(target_os = "android")]
    pub fn with_android(mut self, app: android_activity::AndroidApp) -> Self { self.android = Some(app); self }

    fn clamp(&mut self) {
        let s = &mut self.store;
        if s.notebooks.is_empty() { s.notebooks.push(Notebook { id: new_id(), name: "My Notebook".into(), color: PALETTE[0], sections: vec![] }); }
        self.nb = self.nb.min(s.notebooks.len() - 1);
        let nb = &mut s.notebooks[self.nb];
        if nb.sections.is_empty() { nb.sections.push(Section { id: new_id(), name: "Quick Notes".into(), color: PALETTE[1], pages: vec![], lock: None, locked_files: vec![] }); }
        self.sec = self.sec.min(nb.sections.len() - 1);
        let sec = &mut nb.sections[self.sec];
        if sec.pages.is_empty() { sec.pages.push(Page::new("Untitled page")); }
        self.pg = self.pg.min(sec.pages.len() - 1);
    }

    fn page(&mut self) -> &mut Page { self.clamp(); &mut self.store.notebooks[self.nb].sections[self.sec].pages[self.pg] }
    fn section(&mut self) -> &mut Section { self.clamp(); &mut self.store.notebooks[self.nb].sections[self.sec] }

    fn find_page(&mut self, id: u64) -> Option<&mut Page> {
        self.store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).flat_map(|s| s.pages.iter_mut()).find(|p| p.id == id)
    }

    fn go_to(&mut self, nb: usize, sec: usize, pg: usize) {
        if self.st.dirty { self.st.full_save = true; }
        let id_at = |s: &Self, a: usize, b: usize, c: usize| s.store.notebooks.get(a).and_then(|n| n.sections.get(b)).and_then(|x| x.pages.get(c)).map(|p| p.id);
        if let (Some(from), Some(to)) = (id_at(self, self.nb, self.sec, self.pg), id_at(self, nb, sec, pg)) {
            if from != to && !self.st.nav.navigating {
                self.st.nav.nav_back.push(from);
                if self.st.nav.nav_back.len() > 50 { self.st.nav.nav_back.remove(0); }
                self.st.nav.nav_fwd.clear();
            }
        }
        if self.st.nav.narrow { self.st.p.show_nav = false; }
        (self.nb, self.sec, self.pg) = (nb, sec, pg);
        self.clamp();
        self.st.nav.reveal = true;
        if let Some((sid, id)) = self.store.notebooks.get(nb).and_then(|n| n.sections.get(sec)).and_then(|s| s.pages.get(pg).map(|p| (s.id, p.id))) {
            self.st.p.last_page.insert(sid, id);
            self.st.p.recent.retain(|r| *r != id);
            self.st.p.recent.insert(0, id);
            self.st.p.recent.truncate(15);
        }
        self.st.canvas.popup = None;
        if self.st.jobs.ai_log.last().is_some_and(|(_, t)| !t.starts_with("———")) {
            let title = self.store.notebooks.get(nb).and_then(|n| n.sections.get(sec)).and_then(|s| s.pages.get(pg)).map(|p| p.title.clone()).unwrap_or_default();
            self.st.jobs.ai_log.push((true, format!("——— {} ———", if title.trim().is_empty() { tr("Untitled page").to_string() } else { title })));
        }
        // Keep audio player playing across page switches (controlled via the global mini-player bar).
        self.st.audio.fade_after = None;
        self.st.clear_selection();
        self.st.canvas.editing = None;
        self.st.canvas.last_text = None;
        self.st.cache.derived_dirty = true;
        self.st.dirty = true;
    }

    pub(crate) fn go_back(&mut self, back: bool) {
        let here = self.page().id;
        loop {
            let popped = if back { self.st.nav.nav_back.pop() } else { self.st.nav.nav_fwd.pop() };
            let Some(id) = popped else { return };
            let Some((a, b, c)) = self.locate(id) else { continue };
            if back { self.st.nav.nav_fwd.push(here) } else { self.st.nav.nav_back.push(here) }
            self.st.nav.navigating = true;
            self.go_to(a, b, c);
            self.st.nav.navigating = false;
            return;
        }
    }

    pub(crate) fn open_section(&mut self, nb: usize, sec: usize) {
        let pg = self.store.notebooks.get(nb).and_then(|n| n.sections.get(sec)).and_then(|s| {
            let id = self.st.p.last_page.get(&s.id)?;
            s.pages.iter().position(|p| p.id == *id)
        }).unwrap_or(0);
        self.go_to(nb, sec, pg);
    }

    fn locate(&self, id: u64) -> Option<(usize, usize, usize)> {
        self.store.notebooks.iter().enumerate().find_map(|(a, n)| n.sections.iter().enumerate().find_map(|(b, s)| s.pages.iter().position(|p| p.id == id).map(|c| (a, b, c))))
    }

    fn open_title(&mut self, title: &str) {
        if title.trim().is_empty() { return; }
        let t = title.trim().to_lowercase();
        let hit = self.store.notebooks.iter().flat_map(|n| n.sections.iter()).flat_map(|s| s.pages.iter()).find(|p| p.title.trim().to_lowercase() == t).map(|p| p.id);
        match hit.and_then(|id| self.locate(id)) {
            Some((a, b, c)) => self.go_to(a, b, c),
            None if !self.section_open_for_writing() => {}
            None => {
                let p = Page { paper: self.st.p.new_paper, ..Page::new(title.trim()) };
                let sec = self.section();
                sec.pages.push(p);
                let at = sec.pages.len() - 1;
                let (nb, s) = (self.nb, self.sec);
                self.go_to(nb, s, at);
                self.st.say(tf!("Created the page “{trim}”.", trim = title.trim()));
            }
        }
    }

    fn rename(&mut self, id: u64, name: String) {
        let old = self.store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).find(|p| p.id == id).map(|p| p.title.clone());
        if let Some(old) = old.filter(|o| !o.trim().is_empty() && o.trim() != name.trim()) {
            let mut n = 0;
            for p in self.store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).flat_map(|s| s.pages.iter_mut()) {
                for b in &mut p.blocks {
                    match &mut b.kind {
                        Kind::Text { text, .. } => n += relink(text, &old, name.trim()),
                        Kind::Checklist { items } => for (_, t) in items { n += relink(t, &old, name.trim()); },
                        _ => {}
                    }
                }
            }
            if n > 0 { self.st.say(tf!("Updated {n} link(s) to this page.", n = n)); }
        }
        for n in &mut self.store.notebooks {
            if n.id == id { n.name = name; self.st.cache.derived_dirty = true; return; }
            for s in &mut n.sections {
                if s.id == id { s.name = name; self.st.cache.derived_dirty = true; return; }
                for p in &mut s.pages {
                    if p.id == id {
                        p.title = name.clone();
                        if let Some((pid, t)) = &mut self.st.audio.player_page {
                            if *pid == id { *t = name; }
                        }
                        self.st.cache.derived_dirty = true;
                        return;
                    }
                }
            }
        }
    }

    fn delete(&mut self, id: u64) {
        let now = now_secs();
        let mut binned = vec![];
        if let Some(i) = self.store.notebooks.iter().position(|n| n.id == id) {
            let mut n = self.store.notebooks.remove(i);
            for s in &mut n.sections { self.close_for_bin(s); }
            binned.push(Trashed { label: tf!("Notebook “{name}”", name = n.name), deleted: now, item: TrashItem::Notebook(n), at: Some(i) });
        }
        let mut found = None;
        for (a, n) in self.store.notebooks.iter().enumerate() {
            if let Some(i) = n.sections.iter().position(|s| s.id == id) { found = Some((a, i, None)); }
            for (b, s) in n.sections.iter().enumerate() {
                if let Some(i) = s.pages.iter().position(|p| p.id == id && p.kind != PageKind::Locked) { found = Some((a, b, Some(i))); }
            }
        }
        match found {
            Some((a, i, None)) => {
                let mut s = self.store.notebooks[a].sections.remove(i);
                self.close_for_bin(&mut s);
                let n = &self.store.notebooks[a];
                binned.push(Trashed { label: tf!("Section “{name}” from {nb}", name = s.name, nb = n.name), deleted: now, item: TrashItem::Section { notebook: n.id, section: s }, at: Some(i) });
            }
            Some((a, b, Some(i))) => {
                let p = self.store.notebooks[a].sections[b].pages.remove(i);
                let (n, s) = (&self.store.notebooks[a], &self.store.notebooks[a].sections[b]);
                let (nid, nname) = (n.id, n.name.clone());
                if s.lock.is_none() {
                    binned.push(Trashed { label: tf!("Page “{title}” from {nb} › {sec}", title = p.title, nb = nname, sec = s.name), deleted: now, item: TrashItem::Page { notebook: nid, section: s.id, page: p }, at: Some(i) });
                } else {
                    let from = s.clone_meta();
                    match self.sealed_page_for_bin(&from, p) {
                        Ok(section) => binned.push(Trashed { label: tf!("A page from {nb} › {name} (locked)", nb = nname, name = from.name), deleted: now, item: TrashItem::Section { notebook: nid, section }, at: None }),
                        Err(p) => { self.store.notebooks[a].sections[b].pages.insert(i, p); self.st.say("Couldn't delete this page."); return; }
                    }
                }
            }
            None => {}
        }
        if !binned.is_empty() { self.st.nav.undo_delete = Some(now_f()); }
        self.store.trash.extend(binned);
        self.st.dirty = true;
        self.st.cache.derived_dirty = true;
        self.st.canvas.selected.clear();
        self.st.say("Moved to Recently deleted — restore it from the notebook menu.");
        self.clamp();
    }

    fn restore(&mut self, i: usize) {
        let t = self.store.trash.remove(i);
        let (nb, sec) = (self.nb, self.sec);
        fn put<T>(v: &mut Vec<T>, at: Option<usize>, x: T) { let i = at.unwrap_or(usize::MAX).min(v.len()); v.insert(i, x); }
        match t.item {
            TrashItem::Notebook(n) => put(&mut self.store.notebooks, t.at, n),
            TrashItem::Section { notebook, section } => {
                match self.store.notebooks.iter_mut().find(|n| n.id == notebook) { Some(n) => put(&mut n.sections, t.at, section), None => self.store.notebooks[nb].sections.push(section) }
            }
            TrashItem::Page { notebook, section, page } => {
                let here_ok = !self.store.notebooks[nb].sections[sec].sealed();
                let s = self.store.notebooks.iter_mut().filter(|n| n.id == notebook).flat_map(|n| n.sections.iter_mut()).find(|s| s.id == section && !s.sealed());
                match s {
                    Some(s) => put(&mut s.pages, t.at, page),
                    None if here_ok => self.store.notebooks[nb].sections[sec].pages.push(page),
                    None => {
                        self.store.trash.insert(i, Trashed { label: t.label, deleted: t.deleted, item: TrashItem::Page { notebook, section, page }, at: t.at });
                        self.st.say("Unlock this section first.");
                        return;
                    }
                }
            }
        }
        sync::dedupe(&mut self.store);
        self.st.dirty = true;
        self.st.cache.derived_dirty = true;
        self.st.say(tf!("Restored {label}", label = t.label));
    }

    pub(crate) fn purge(&mut self, gone: Vec<Trashed>) {
        if self.st.persist {
            let pages = gone.iter().flat_map(|t| match &t.item {
                TrashItem::Page { page, .. } => vec![page.id],
                TrashItem::Section { section, .. } => section.pages.iter().map(|p| p.id).collect(),
                TrashItem::Notebook(n) => n.sections.iter().flat_map(|s| &s.pages).map(|p| p.id).collect(),
            });
            for id in pages { let _ = std::fs::remove_file(data_dir().join("history").join(format!("{id}.jsonl"))); }
            let rec_file = self.st.audio.recorder.as_ref().and_then(|(r, ..)| r.path.file_name().and_then(|n| n.to_str()).map(String::from));
            let store_snap = self.sealed().unwrap_or_else(|| self.store.clone());
            let dir = data_dir();
            std::thread::spawn(move || {
                storage::gc_local_files(&store_snap, &dir, rec_file.as_deref(), 60);
            });
        }
        self.st.dirty = true;
        self.st.cache.derived_dirty = true;
    }

    fn duplicate_page(&mut self, nb: usize, sec: usize, pg: usize) {
        if self.store.notebooks[nb].sections[sec].sealed() || self.store.notebooks[nb].sections[sec].pages[pg].kind == PageKind::Locked {
            return;
        }
        let mut p = self.store.notebooks[nb].sections[sec].pages[pg].clone();
        p.id = new_id();
        p.title = tf!("{name} (copy)", name = p.title);
        for b in &mut p.blocks { b.id = new_id(); }
        for s in &mut p.strokes { s.id = new_id(); }
        for c in &mut p.cards { c.id = new_id(); }
        for r in &mut p.recordings { r.id = new_id(); }
        self.store.notebooks[nb].sections[sec].pages.insert(pg + 1, p);
        self.go_to(nb, sec, pg + 1);
    }

    fn save_now(&mut self) {
        self.st.sync_st.saves += 1;
        self.st.dirty = false;
        self.st.since_save = 0.0;
        if !self.st.persist { return; }
        self.st.p.last = (self.nb, self.sec, self.pg);
        if let Some(id) = self.store.notebooks.get(self.nb).and_then(|n| n.sections.get(self.sec)).and_then(|s| s.pages.get(self.pg)).map(|p| p.id) { self.st.p.last_id = id; }
        let keep: Vec<u64> = std::iter::once(self.st.p.last_id).chain(self.st.p.recent.iter().copied()).take(20).collect();
        self.st.p.views = keep.iter().filter_map(|id| {
            let v = self.st.canvas.views.get(id)?;
            Some((*id, [v.min.x, v.min.y, v.width(), v.height(), self.st.canvas.scales.get(id).copied().unwrap_or(1.0)]))
        }).collect();
        let sealed = self.sealed();
        let mut prefs = self.st.p.clone();
        if let Some(c) = &self.st.canvas.capture { (prefs.show_nav, prefs.show_ai, prefs.ribbon_open) = c.keep; }
        let full = std::mem::take(&mut self.st.full_save) || now_f() - self.st.full_at > 20.0;
        if full { self.st.full_at = now_f(); }
        let front = (!full).then(|| self.page().id);
        let backup_name = (self.st.p.auto_backup && now_secs().saturating_sub(self.st.p.last_backup) > 86_400)
            .then(|| chrome::day_name(now_secs()));
        let store_snap = sealed.unwrap_or_else(|| self.store.clone());
        let job = storage::SaveJob {
            dir: data_dir(),
            store: store_snap,
            front,
            ai: self.st.ai.clone(),
            prefs,
            sync: self.st.sync_st.settings.clone(),
            backup_name,
        };
        if let Some(worker) = &self.st.save_worker {
            worker.submit(job);
        }
        self.st.sync_st.pending = true;
    }

    fn undo(&mut self, redo: bool) -> bool {
        let here = self.page().id;
        loop {
            let stack = if redo { &mut self.st.redo } else { &mut self.st.undo };
            let popped = stack.iter().rposition(|(id, _)| *id == here).map(|i| stack.remove(i));
            let Some((id, mut old)) = popped else { self.st.say(if redo { "Nothing to redo." } else { "Nothing to undo." }); return false };
            let Some(p) = self.find_page(id) else { return false };
            if same_page(p, &old) { continue; }
            old.title = p.title.clone();
            old.level = p.level;
            let cur = std::mem::replace(p, old);
            if redo { self.st.undo.push((id, cur)) } else { self.st.redo.push((id, cur)) }
            self.st.dirty = true;
            self.st.cache.derived_dirty = true;
            self.st.canvas.selected.clear();
            return true;
        }
    }

    // ---------- AI & Background Jobs ----------
    fn start_ai_job(&mut self, kind: AiJobKind, system: &str, prompt: String, png: Option<Vec<u8>>, chat_idx: Option<usize>) {
        if !self.st.ai.ready() {
            self.st.modals.open_settings(3);
            self.st.say("Add your AI key first — it stays on this device.");
            return;
        }
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_bg = cancel.clone();
        let (s, system) = (self.st.ai.clone(), system.to_string());
        std::thread::spawn(move || {
            let r = ai::ask(&s, &system, &prompt, png.as_deref(), &cancel_bg).map_err(|e| e.message);
            let _ = tx.send(AiEvent::Done(r));
        });
        self.st.jobs.ai_jobs.push(AiJob { kind, rx, cancel, chat_idx });
    }

    fn chat(&mut self, prompt: String) {
        if !self.st.ai.ready() {
            self.st.modals.open_settings(3);
            self.st.say("Add your AI key first — it stays on this device.");
            return;
        }
        let mut page = self.page().plain_text();
        const MAX_CHARS: usize = 60_000;
        if let Some((byte_idx, _)) = page.char_indices().nth(MAX_CHARS) {
            page.truncate(byte_idx);
            page.push_str("\n[… the rest of the page was left out]");
        }
        let strokes: Vec<&Stroke> = self.page().strokes.iter().filter(|s| !s.highlighter).collect();
        let png = if strokes.is_empty() { None } else { rasterize(&strokes) };
        let since = self.st.jobs.ai_log.iter().rposition(|(_, t)| t.starts_with("———")).map_or(0, |i| i + 1);
        // Build structured multi-turn message history instead of flattening into a single user prompt.
        let mut turns: Vec<(bool, String)> = self.st.jobs.ai_log[since..]
            .iter()
            .filter(|(_, t)| !t.is_empty() && !t.starts_with('⚠'))
            .rev()
            .take(8)
            .cloned()
            .collect();
        turns.reverse();
        let current_user = format!("--- Current note page ---\n{page}\n\n--- Request ---\n{prompt}");
        turns.push((false, current_user));

        self.st.jobs.ai_log.push((false, prompt));
        let chat_idx = self.st.jobs.ai_log.len();
        self.st.jobs.ai_log.push((true, String::new()));
        self.st.p.show_ai = true;

        let picture = if png.is_some() { " The attached picture shows the page's handwritten notes." } else { "" };
        let system = format!(
            "You are a helpful assistant inside a note-taking app. Reply in concise Markdown. Use $...$ for inline math and put display math on its own line as $$...$$. Write action items as '- [ ] item'.{picture}{}",
            crate::i18n::reply_hint()
        );
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_bg = cancel.clone();
        let s = self.st.ai.clone();
        std::thread::spawn(move || {
            let mut streamed = false;
            let mut r = ai::stream_chat(&s, &system, &turns, png.as_deref(), &cancel_bg, |delta| {
                streamed = true;
                let _ = tx.send(AiEvent::Delta(delta.to_string()));
            });
            // Only retry without vision image if the server explicitly rejected the payload (HTTP 400/415/422) before streaming.
            if let Err(ref e) = r {
                if !streamed && png.is_some() && e.is_vision_rejected() && !cancel_bg.load(std::sync::atomic::Ordering::Relaxed) {
                    r = ai::stream_chat(&s, &system, &turns, None, &cancel_bg, |delta| {
                        let _ = tx.send(AiEvent::Delta(delta.to_string()));
                    });
                }
            }
            let _ = tx.send(AiEvent::Done(r.map_err(|e| e.message)));
        });
        self.st.jobs.ai_jobs.push(AiJob { kind: AiJobKind::Chat, rx, cancel, chat_idx: Some(chat_idx) });
    }

    fn rewrite(&mut self, mode: Rewrite, instruction: &str) {
        let target = self.st.canvas.last_text;
        let page = self.page();
        let Some(b) = target.and_then(|id| page.blocks.iter().find(|b| b.id == id)) else { self.st.say("Click into a text note first."); return; };
        let Kind::Text { text, .. } = &b.kind else { return };
        let (pid, bid, text) = (page.id, b.id, text.clone());
        self.st.say("AI is writing…");
        self.start_ai_job(
            AiJobKind::Rewrite { page: pid, block: bid, mode },
            "You edit notes. Keep the author's Markdown formatting and $math$. Reply with only the resulting text.",
            format!("{instruction}\n\n---\n{text}"),
            None,
            None,
        );
    }

    fn equation_from_words(&mut self) {
        let desc = std::mem::take(&mut self.st.jobs.eq_prompt);
        if desc.trim().is_empty() { self.st.say("Describe the equation first, e.g. “quadratic formula”."); return; }
        let pid = self.page().id;
        self.st.say("Writing the equation…");
        self.start_ai_job(
            AiJobKind::Equation { page: pid },
            "You write LaTeX math.",
            format!("Write the LaTeX for: {desc}\nReply with only the LaTeX expression, no $ signs, no code fences."),
            None,
            None,
        );
    }

    fn ink_convert(&mut self, math: bool) {
        let sel = self.st.canvas.selected.clone();
        if sel.is_empty() && self.page().strokes.iter().any(|s| !s.highlighter) {
            self.st.canvas.tool = Tool::Lasso;
            self.st.say("Circle the handwriting to convert with the lasso, then press the button again.");
            return;
        }
        let use_ai = self.st.p.ink_ai && self.st.ai.ready();
        let page = self.page();
        let strokes: Vec<&Stroke> = page.strokes.iter().filter(|s| !s.highlighter && (sel.is_empty() || sel.contains(&s.id))).collect();
        if strokes.is_empty() {
            self.st.say("Write something with the pen first (or lasso the ink to convert).");
            return;
        }
        let ids = strokes.iter().map(|s| s.id).collect();
        let at = strokes.iter().flat_map(|s| s.points.iter()).fold([f32::MAX, f32::MAX], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
        let width = (strokes.iter().flat_map(|s| s.points.iter()).fold(f32::MIN, |m, p| m.max(p[0])) - at[0] + 40.0).clamp(220.0, 900.0);
        let pid = page.id;
        if !use_ai {
            let owned: Vec<Stroke> = strokes.iter().map(|s| (*s).clone()).collect();
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let t = std::panic::catch_unwind(|| if math { hw::recognize_math(&owned) } else { hw::recognize_text(&owned) }).unwrap_or_default();
                let _ = tx.send(if t.trim().is_empty() { Err(tr("Couldn't read that handwriting — try writing a little larger and more separated.").into()) } else { Ok(t) });
            });
            self.st.jobs.bg_jobs.push(BgJob { kind: BgJobKind::OfflineInk { math, page: pid, strokes: ids, at, width }, rx });
            self.st.say(if math { "Reading your handwriting as math (offline)…" } else { "Reading your handwriting (offline)…" });
            return;
        }
        let Some(png) = rasterize(&strokes) else { self.st.say("Write something with the pen first (or lasso the ink to convert)."); return; };
        let (system, prompt) = if math {
            ("You convert handwritten math to LaTeX.", "Convert the handwritten math in this image to LaTeX. Reply with only the LaTeX expression, no $ signs, no code fences, no commentary.")
        } else {
            ("You transcribe handwriting.", "Transcribe the handwriting in this image. Reply with only the transcribed text, keeping line breaks. Use Markdown lists if the writing is a list. No commentary.")
        };
        self.st.say(if math { "Reading your handwriting as math…" } else { "Reading your handwriting…" });
        self.start_ai_job(AiJobKind::Ink { math, page: pid, strokes: ids, at, width }, system, prompt.into(), Some(png), None);
    }

    fn test_ai(&mut self) {
        self.st.jobs.test_result = None;
        self.start_ai_job(AiJobKind::Test, "Reply with the single word OK.", "Say OK".into(), None, None);
    }

    fn poll_ai(&mut self, ctx: &egui::Context) {
        let mut done = vec![];
        let mut jobs = std::mem::take(&mut self.st.jobs.ai_jobs);
        jobs.retain_mut(|job| {
            loop {
                match job.rx.try_recv() {
                    Ok(AiEvent::Delta(chunk)) => {
                        if let Some(idx) = job.chat_idx {
                            if let Some((_, text)) = self.st.jobs.ai_log.get_mut(idx) {
                                text.push_str(&chunk);
                            }
                        }
                    }
                    Ok(AiEvent::Done(r)) => {
                        done.push((std::mem::replace(&mut job.kind, AiJobKind::Chat), job.chat_idx, r));
                        return false;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => return true,
                    Err(_) => {
                        done.push((std::mem::replace(&mut job.kind, AiJobKind::Chat), job.chat_idx, Err(tr("That stopped unexpectedly — please try again.").into())));
                        return false;
                    }
                }
            }
        });
        self.st.jobs.ai_jobs = jobs;
        if self.st.jobs.ai_busy() { ctx.request_repaint_after(std::time::Duration::from_millis(80)); }
        for (kind, chat_idx, r) in done {
            match (kind, r) {
                (AiJobKind::Test, r) => self.st.jobs.test_result = Some(r),
                (AiJobKind::Cards { title }, Ok(t)) => {
                    let cards: Vec<(String, String)> = t.lines().filter_map(|l| l.trim().trim_start_matches(['-', '*', ' ']).split_once("::").map(|(q, a)| (q.trim().to_string(), a.trim().to_string())))
                        .filter(|(q, a)| !q.is_empty() && !a.is_empty()).collect();
                    if cards.is_empty() { self.st.say("⚠ The AI didn't return any flashcards — try again."); continue; }
                    let n = cards.len();
                    self.new_deck(&deck_name(&title), cards);
                    self.st.say(tf!("Made a deck of {n} cards — edit anything you like, then press Study.", n = n));
                }
                (AiJobKind::Chat, Err(e)) => {
                    if let Some(idx) = chat_idx {
                        if let Some((_, text)) = self.st.jobs.ai_log.get_mut(idx) {
                            if text.is_empty() { *text = format!("⚠ {e}"); } else { text.push_str(&format!("\n\n⚠ {e}")); }
                        }
                    } else {
                        self.st.jobs.ai_log.push((true, format!("⚠ {e}")));
                    }
                    self.st.say(e);
                }
                (_, Err(e)) => self.st.say(format!("⚠ {e}")),
                (AiJobKind::Chat, Ok(t)) => {
                    if let Some(idx) = chat_idx {
                        if let Some((_, text)) = self.st.jobs.ai_log.get_mut(idx) {
                            if text.is_empty() { *text = t; }
                        }
                    } else {
                        self.st.jobs.ai_log.push((true, t));
                    }
                }
                (AiJobKind::Ink { math, page, strokes, at, width }, Ok(t)) => {
                    let t = ai::clean(&t);
                    if t.trim().is_empty() { self.st.say("⚠ Nothing could be read there, so the handwriting stays as it is."); continue; }
                    self.st.modals.push(ActiveModal::InkReview(InkReview { math, page, strokes, at, width, text: t }));
                }
                (AiJobKind::Rewrite { page, block: bid, mode }, Ok(t)) => {
                    let Some(p) = self.find_page(page) else { continue };
                    let snap = p.clone();
                    if let Some(Kind::Text { text, .. }) = p.blocks.iter_mut().find(|b| b.id == bid).map(|b| &mut b.kind) {
                        match mode { Rewrite::Replace => *text = t.trim().to_string(), Rewrite::Append => { text.push_str("\n\n"); text.push_str(t.trim()); } }
                    }
                    self.st.snap(&snap);
                    self.st.say("Done. Undo restores the previous text.");
                }
                (AiJobKind::Equation { page }, Ok(t)) => {
                    if self.page().id == page { self.insert(Kind::Math { src: ai::clean(&t) }); self.st.canvas.editing = None; self.st.say("Equation added."); }
                    else { self.st.say("The equation is ready, but you moved to another page — ask again there."); }
                }
            }
        }
    }

    fn poll_bg(&mut self, ctx: &egui::Context) {
        let mut done = vec![];
        self.st.jobs.bg_jobs.retain_mut(|job| match job.rx.try_recv() {
            Ok(r) => { done.push((std::mem::replace(&mut job.kind, BgJobKind::Note), r)); false }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => { done.push((std::mem::replace(&mut job.kind, BgJobKind::Note), Err(tr("That stopped unexpectedly — please try again.").into()))); false }
        });
        if self.st.jobs.bg_busy() { ctx.request_repaint_after(std::time::Duration::from_millis(150)); }
        for (kind, r) in done {
            match (kind, r) {
                (BgJobKind::Note, Ok(m)) => self.st.say(m),
                (_, Err(e)) => self.st.say(format!("⚠ {e}")),
                (BgJobKind::OfflineInk { math, page, strokes, at, width }, Ok(t)) => {
                    let t = ai::clean(&t);
                    if t.trim().is_empty() { self.st.say("⚠ Nothing could be read there, so the handwriting stays as it is."); continue; }
                    self.st.modals.push(ActiveModal::InkReview(InkReview { math, page, strokes, at, width, text: t }));
                }
                (BgJobKind::Printout { page }, Ok(names)) => {
                    let Some(p) = self.find_page(page) else { continue };
                    let snap = p.clone();
                    self.st.snap(&snap);
                    let mut extra = String::new();
                    for name in names.lines() {
                        if let Some(n) = name.strip_prefix('…') { extra = tf!(" (first 60 of {n} pages)", n = n); continue; }
                        let (w, h) = image::image_dimensions(files_path(name)).unwrap_or((800, 1000));
                        let width = 640.0;
                        let y = {
                            let heights = std::mem::take(&mut self.st.canvas.heights);
                            let y = self.find_page(page).map_or(8.0, |p| p.blocks.iter().map(|b| b.pos[1] + heights.get(&b.id).copied().unwrap_or(40.0) + 18.0).fold(8.0, f32::max));
                            self.st.canvas.heights = heights;
                            y
                        };
                        let b = block(24.0, y, width, Kind::Image { file: name.to_string() });
                        self.st.canvas.heights.insert(b.id, width * h as f32 / w.max(1) as f32);
                        if let Some(p) = self.find_page(page) { p.blocks.push(b); }
                    }
                    self.st.say(tf!("Printout added{extra} — pick a pen to write on it.", extra = extra));
                }
            }
        }
    }

    // ---------- inserting ----------
    fn next_free_y(&mut self) -> f32 {
        let heights = std::mem::take(&mut self.st.canvas.heights);
        let y = self.page().blocks.iter().map(|b| b.pos[1] + heights.get(&b.id).copied().unwrap_or(40.0) + 18.0).fold(8.0, f32::max);
        self.st.canvas.heights = heights;
        y
    }

    fn insert(&mut self, kind: Kind) {
        if self.page().kind == PageKind::Locked { return; }
        let y = self.next_free_y();
        let w = match kind { Kind::Table { .. } | Kind::Checklist { .. } | Kind::Link { .. } | Kind::File { .. } => 380.0, _ => self.st.p.note_width };
        let pid = self.page().id;
        let w = self.st.canvas.views.get(&pid).map_or(w, |v| w.min(v.right() - 24.0 - 12.0).max(160.0));
        let b = block(24.0, y, w, kind);
        let id = b.id;
        let page = self.page();
        let snap = page.clone();
        page.blocks.push(b);
        self.st.snap(&snap);
        self.st.nav.focus_id = Some(egui::Id::unique(("ed", id)));
        self.st.canvas.editing = Some(id);
        self.st.canvas.tool = Tool::Type;
        if let Some(v) = self.st.canvas.views.get_mut(&pid) {
            if y < v.top() || y + 60.0 > v.bottom() || v.left() > 24.0 {
                *v = v.translate(Vec2::new(-v.left(), (y - 80.0).max(-40.0) - v.top()));
            }
        }
    }

    fn insert_markdown(&mut self, md: &str) {
        let snap = self.page().clone();
        self.st.snap(&snap);
        let near = std::mem::take(&mut self.st.jobs.insert_near).then_some(self.st.canvas.last_text).flatten()
            .and_then(|id| { let h = self.st.canvas.heights.get(&id).copied().unwrap_or(40.0); self.page().blocks.iter().find(|b| b.id == id).map(|b| b.pos[1] + h + 16.0) });
        let mut y_at = near;
        for k in blocks_from_markdown(md) {
            let y = match y_at { Some(y) => y, None => self.next_free_y() };
            let lines = k.summary().len() as f32 / 60.0 + md.lines().count().min(12) as f32;
            let w = if matches!(k, Kind::Checklist { .. } | Kind::Table { .. }) { 380.0 } else { self.st.p.note_width };
            let b = block(24.0, y, w, k);
            self.st.canvas.heights.insert(b.id, 24.0 * lines.max(1.0));
            if let Some(y) = &mut y_at { *y += 24.0 * lines.max(1.0) + 16.0; }
            self.page().blocks.push(b);
        }
        self.st.canvas.tool = Tool::Type;
        self.st.say("Inserted into the page.");
    }

    fn insert_file(&mut self, path: &std::path::Path, how: Ins) {
        self.insert_file_named(path, None, how);
    }

    fn insert_file_named(&mut self, path: &std::path::Path, display_name: Option<&str>, how: Ins) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let ext = if (1..=8).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) { ext } else { "bin".into() };
        if how == Ins::Printout && matches!(ext.as_str(), "md" | "markdown" | "txt") {
            match std::fs::read_to_string(path) {
                Ok(t) => { self.insert_markdown(&t); self.st.say("Printed the text onto the page."); }
                Err(e) => self.st.say(tf!("⚠ Could not read {path}: {e}", path = path.display(), e = e)),
            }
            return;
        }
        let is_image = how == Ins::Picture || matches!(ext.as_str(), "png" | "jpg" | "jpeg");
        let stored = format!("{}.{ext}", new_id());
        let dir = data_dir().join("files");
        let part = dir.join(format!(".{stored}"));
        let copied = std::fs::create_dir_all(&dir)
            .and_then(|_| if is_image { store_picture(path, &part) } else { std::fs::copy(path, &part) })
            .and_then(|n| {
                storage::set_private_perms(&part);
                std::fs::rename(&part, dir.join(&stored)).map(|_| n)
            });
        if copied.is_err() { let _ = std::fs::remove_file(&part); }
        match copied {
            Err(e) => self.st.say(tf!("⚠ Could not add {path}: {e}", path = path.display(), e = e)),
            Ok(size) => {
                let name = display_name
                    .filter(|s| !s.trim().is_empty())
                    .map(String::from)
                    .unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                self.insert(if is_image { Kind::Image { file: stored.clone() } } else { Kind::File { file: stored.clone(), name, size } });
                self.st.canvas.editing = None;
                if how == Ins::Printout && ext == "pdf" { self.start_printout(dir.join(&stored)); }
            }
        }
    }

    fn start_printout(&mut self, pdf: PathBuf) {
        const MAX_PAGES: usize = 60;
        let pid = self.page().id;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let r = print_pdf(&pdf, MAX_PAGES);
            let _ = tx.send(r);
        });
        self.st.jobs.bg_jobs.push(BgJob { kind: BgJobKind::Printout { page: pid }, rx });
        self.st.say("Printing the PDF onto the page…");
    }

    fn pick_file(&mut self, how: Ins) {
        #[cfg(not(target_os = "android"))]
        {
            let mut d = rfd::FileDialog::new();
            match how {
                Ins::Picture => d = d.add_filter(tr("Pictures"), &["png", "jpg", "jpeg"]),
                Ins::Printout => d = d.add_filter(tr("Printable files"), &["pdf", "png", "jpg", "jpeg", "md", "txt"]),
                Ins::Attach => {}
            }
            if let Some(p) = d.pick_file() { self.insert_file(&p, how); }
        }
        #[cfg(target_os = "android")]
        {
            let (kind, mime) = match how {
                Ins::Picture => (1, "image/*"),
                Ins::Printout => (2, "application/pdf,image/*,text/*,text/markdown,application/octet-stream"),
                Ins::Attach => (3, "*/*"),
            };
            if let Err(e) = droid::pick(kind, mime, false) { self.st.say(tf!("⚠ {e}", e = e)); }
            self.st.picking_since = now_f();
        }
    }

    #[cfg(target_os = "android")]
    fn take_incoming(&mut self) {
        if !droid::has_incoming() { return; }
        for (kind, name, path) in droid::incoming() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            match kind {
                1 => self.insert_file_named(&path, Some(&name), Ins::Picture),
                2 => self.insert_file_named(&path, Some(&name), Ins::Printout),
                3 => self.insert_file_named(&path, Some(&name), Ins::Attach),
                4 => self.restore_backup(&path),
                5 => self.import_markdown(vec![path.clone()]),
                9 => if let Ok(t) = std::fs::read_to_string(&path) { self.insert_markdown(&t); },
                _ => self.insert_file_named(&path, Some(&name), match ext.as_str() { "pdf" => Ins::Printout, "png" | "jpg" | "jpeg" | "webp" => Ins::Picture, "md" | "txt" => Ins::Printout, _ => Ins::Attach }),
            }
            if matches!(kind, 0 | 9) {
                let title = self.page().title.clone();
                self.st.say(tf!("Added to “{page}”.", page = if title.trim().is_empty() { tr("Untitled page").to_string() } else { title }));
            }
            let _ = std::fs::remove_file(&path);
            self.st.picking_since = -1e9;
        }
    }

    fn new_page(&mut self, paper: Option<Paper>) -> bool {
        if !self.section_open_for_writing() { return false; }
        let mut p = Page { paper: paper.unwrap_or(self.st.p.new_paper), ..Page::new("") };
        let pg = self.pg;
        let sec = self.section();
        let at = if pg < sec.pages.len() { p.level = sec.pages[pg].level; pg + 1 } else { sec.pages.len() };
        sec.pages.insert(at, p);
        let (nb, s) = (self.nb, self.sec);
        self.go_to(nb, s, at);
        self.st.nav.focus_id = Some(egui::Id::unique("page-title"));
        self.st.canvas.tool = Tool::Type;
        true
    }

    fn export_page(&mut self) {
        let page = self.page().clone();
        let name = format!("{}.md", if page.title.trim().is_empty() { "Untitled page" } else { page.title.as_str() }.replace(['/', '\\', ':', '?', '*', '"', '<', '>', '|'], "-"));
        #[cfg(not(target_os = "android"))]
        if let Some(p) = rfd::FileDialog::new().set_file_name(&name).add_filter("Markdown", &["md"]).save_file() {
            let files = data_dir().join("files");
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let r = export::markdown_with_files(&page, &files, &p)
                    .map(|_| tf!("Exported to {p}", p = p.display()))
                    .map_err(|e| tf!("Export failed: {e}", e = e));
                let _ = tx.send(r);
            });
            self.st.jobs.bg_jobs.push(BgJob { kind: BgJobKind::Note, rx });
            self.st.say("Exporting…");
        }
        #[cfg(target_os = "android")]
        {
            let md = page.plain_text();
            let dir = data_dir().join("exports");
            let r = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(&name), md));
            match r { Ok(_) => droid::share(&dir.join(&name)), Err(e) => self.st.say(tf!("⚠ Export failed: {e}", e = e)) }
        }
    }

    fn ask_save_path(&mut self, name: &str, ext: &str) -> Option<PathBuf> {
        #[cfg(not(target_os = "android"))]
        { rfd::FileDialog::new().set_file_name(name).add_filter(ext.to_uppercase(), &[ext]).save_file() }
        #[cfg(target_os = "android")]
        { let dir = data_dir().join("exports"); let _ = std::fs::create_dir_all(&dir); let _ = ext; Some(dir.join(name)) }
    }

    fn export_pdf(&mut self, whole: bool) {
        let heights = self.st.canvas.heights.clone();
        let hf = move |id: u64| heights.get(&id).copied();
        let page = self.page().clone();
        let title = if page.title.trim().is_empty() { "Untitled page".to_string() } else { page.title.clone() };
        let file = format!("{}.pdf", title.replace(['/', '\\', ':', '?', '*', '"', '<', '>', '|'], "-"));
        if !whole && export::printout_source(&page, &hf).is_some() {
            let Some(out) = self.ask_save_path(&file, "pdf") else { return };
            let files = data_dir().join("files");
            let note_size = self.st.p.note_size;
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let r = export::export_printout(&page, &files, &hf, note_size, &out).map(|n| {
                    shared(&out);
                    let lost = export::unprintable(&page);
                    let note = if lost > 0 { tf!(" ⚠ {k} character(s) (Greek, maths symbols…) can't be printed this way and show as ?. “Export whole page as PDF” keeps them.", k = lost) } else { String::new() };
                    tf!("Exported the printout with your notes on {n} page(s) to {out}", n = n, out = where_saved(&out)) + &note
                });
                let _ = tx.send(r);
            });
            self.st.jobs.bg_jobs.push(BgJob { kind: BgJobKind::Note, rx });
            self.st.say("Exporting the page as PDF…");
            return;
        }
        let mut r: Option<egui::Rect> = None;
        for b in &page.blocks { let br = egui::Rect::from_min_size(Pos2::new(b.pos[0], b.pos[1]), Vec2::new(b.width, hf(b.id).unwrap_or(40.0) + 12.0)); r = Some(r.map_or(br, |r| r.union(br))); }
        for s in &page.strokes { for p in &s.points { let pr = egui::Rect::from_center_size(Pos2::new(p[0], p[1]), Vec2::splat(s.width + 4.0)); r = Some(r.map_or(pr, |r| r.union(pr))); } }
        let Some(content) = r else { self.st.say("This page is empty — nothing to export."); return };
        let Some(path) = self.ask_save_path(&file, "pdf") else { return };
        let keep = (self.st.p.show_nav, self.st.p.show_ai, self.st.p.ribbon_open);
        (self.st.p.show_nav, self.st.p.show_ai, self.st.p.ribbon_open) = (false, false, false);
        self.st.canvas.editing = None;
        self.st.canvas.selected.clear();
        self.st.canvas.capture = Some(canvas::Capture {
            page: page.id,
            content: content.expand(16.0),
            k: 0,
            tiles: vec![],
            wait: 0,
            rx: None,
            path,
            title,
            keep,
            view: self.st.canvas.views.get(&page.id).copied(),
            scale: self.st.canvas.scales.get(&page.id).copied(),
        });
        self.st.say("Exporting the page as PDF…");
    }

    fn new_deck(&mut self, title: &str, cards: Vec<(String, String)>) {
        if !self.section_open_for_writing() { return; }
        let mut p = deck_page(title);
        if !cards.is_empty() { p.cards = cards.iter().map(|(f, b)| DeckCard::new(f, b)).collect(); }
        let first = p.cards[0].id;
        let sec = self.section();
        sec.pages.push(p);
        let at = sec.pages.len() - 1;
        let (nb, s) = (self.nb, self.sec);
        self.go_to(nb, s, at);
        self.st.nav.focus_id = Some(if cards.is_empty() { egui::Id::unique("page-title") } else { egui::Id::unique(("cardf", first)) });
    }

    fn deck_from_notes(&mut self) {
        let page = self.page();
        let title = if page.title.trim().is_empty() { tr("Untitled page").to_string() } else { page.title.clone() };
        let cards: Vec<(String, String)> = flashcards(page).into_iter().map(|c| (c.front, c.back)).collect();
        if cards.is_empty() {
            self.st.say("No card-like notes found here (questions, “term :: meaning”, highlights, headings or tables). Try “With AI”, or start a new deck.");
            return;
        }
        let name = deck_name(&title);
        if let Some(d) = self.section().pages.iter_mut().find(|p| p.kind == PageKind::Deck && p.title == name) {
            let (before, snap) = (d.cards.len(), d.clone());
            for (f, b) in cards { if !d.cards.iter().any(|c| c.front.trim() == f.trim()) { d.cards.push(DeckCard::new(&f, &b)); } }
            let n = d.cards.len() - before;
            self.st.snap(&snap);
            self.st.say(tf!("Added {n} new card(s) to “{deck}”.", n = n, deck = name));
            return;
        }
        let n = cards.len();
        self.new_deck(&name, cards);
        self.st.say(tf!("Suggested {n} cards — edit or delete any you don't want, then press Study.", n = n));
    }

    fn add_card(&mut self, front: String, back: String) {
        let title = { let t = self.page().title.clone(); if t.trim().is_empty() { tr("Untitled page").to_string() } else { t } };
        let name = deck_name(&title);
        let sealed = self.section().sealed();
        let found = self.section().pages.iter_mut().find(|p| p.kind == PageKind::Deck && p.title == name);
        match found {
            Some(d) => { let snap = d.clone(); d.cards.push(DeckCard::new(&front, &back)); self.st.snap(&snap); }
            None if !sealed => {
                let (pg, sec) = (self.pg, self.section());
                let deck = Page { kind: PageKind::Deck, cards: vec![DeckCard::new(&front, &back)], ..Page::new(&name) };
                sec.pages.insert((pg + 1).min(sec.pages.len()), deck);
            }
            None => return,
        }
        self.st.dirty = true;
        self.st.cache.derived_dirty = true;
        self.st.say(tf!("Added a card to “{deck}”.", deck = name));
    }

    pub(crate) fn import_pdfs(&mut self, files: Vec<PathBuf>) {
        let mut n = 0;
        for f in files {
            if !self.new_page(None) { return; }
            self.page().title = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            self.insert_file(&f, Ins::Printout);
            n += 1;
        }
        self.st.nav.focus_id = None;
        self.st.say(tf!("Imported {n} PDF(s), one page each. Their pages appear in a moment.", n = n));
    }

    fn new_page_template(&mut self, which: usize) {
        let (title, paper, md) = TEMPLATES[which];
        if !self.new_page(Some(paper)) { return; }
        let p = page_from_markdown(tr(title), md, self.st.p.note_width);
        let page = self.page();
        page.blocks = p.blocks;
        page.title = format!("{} · {}", tr(title), chrome::short_date(now_secs()));
        self.st.nav.focus_id = None;
    }

    fn ai_cards(&mut self) {
        let page = self.page();
        let title = if page.title.trim().is_empty() { "Untitled page".to_string() } else { page.title.clone() };
        let text = page.plain_text();
        if text.split_whitespace().count() < 8 { self.st.say("Write a few notes on this page first."); return; }
        self.st.say("Writing flashcards…");
        self.start_ai_job(
            AiJobKind::Cards { title },
            &format!("You write short, clear study flashcards.{}", crate::i18n::reply_hint()),
            format!("Write 6 to 12 flashcards from these notes. One per line, exactly in the form: question :: answer\nKeep answers short. Use $...$ for maths. No numbering, no other text.\n\n{text}"),
            None,
            None,
        );
    }

    fn start_lecture(&mut self, record: bool) {
        if !self.new_page(None) { return; }
        self.page().title = format!("{} · {}", tr("Lecture"), chrome::short_date(now_secs()));
        self.st.nav.focus_id = None;
        if record && self.st.audio.recorder.is_none() { self.st.audio.rec_toggle = true; }
        self.st.say(if record { "New lecture page — recording starts once the microphone is allowed." } else { "New lecture page ready." });
    }

    pub(crate) fn start_sync(&mut self, first: Option<sync::First>) {
        if self.st.sync_st.rx.is_some() || !self.st.persist || !self.st.sync_st.settings.on() { return; }
        let sent = self.sealed().unwrap_or_else(|| self.store.clone());
        self.st.sync_st.sync_saves = self.st.sync_st.saves;
        let (cfg, local, home) = (self.st.sync_st.settings.clone(), sent.clone(), data_dir());
        let (tx, rx) = channel();
        std::thread::spawn(move || { let _ = tx.send(sync::run(&cfg, &home, &local, first, false)); });
        self.st.sync_st.rx = Some((sent, rx));
        self.st.sync_st.at = now_f();
        self.st.sync_st.pending = false;
        self.st.sync_st.now = false;
    }

    fn poll_sync(&mut self) {
        let Some((_, rx)) = &self.st.sync_st.rx else { return };
        let r = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => Err(tr("Sync stopped unexpectedly — trying again shortly.").into()),
        };
        let Some((sent, _)) = self.st.sync_st.rx.take() else { return };
        match r {
            Ok(sync::Outcome::AskFirst { pages }) => self.st.modals.push(ActiveModal::SyncAsk { pages }),
            Ok(sync::Outcome::Synced { store, down, base, problem, changed, .. }) => {
                let touched = self.st.dirty || self.st.sync_st.saves != self.st.sync_st.sync_saves;
                let current = touched.then(|| self.sealed().unwrap_or_else(|| self.store.clone()));
                let meanwhile = current.as_ref().is_some_and(|c| !sync::same(&sent, c));
                let merged = match &current { Some(c) if meanwhile => sync::merge(&sent, c, &store), _ => Ok(store) };
                match merged {
                    Ok(m) => {
                        let differs = match &current { Some(c) if meanwhile => !sync::same(&m, c), _ => changed };
                        if differs {
                            let open = self.store.notebooks.get(self.nb).and_then(|n| n.sections.get(self.sec)).and_then(|s| s.pages.get(self.pg)).map(|p| p.id);
                            let before = storage::all_page_ids(&self.store);
                            let old_secs: HashMap<u64, bool> = self.store.notebooks.iter().flat_map(|n| &n.sections).map(|s| (s.id, s.lock.is_some())).collect();
                            self.store = self.reopen(m);
                            let gone: Vec<u64> = before.difference(&storage::all_page_ids(&self.store)).copied().collect();
                            if !gone.is_empty() {
                                if self.st.persist { for p in &gone { let _ = std::fs::remove_file(data_dir().join("history").join(format!("{p}.jsonl"))); } }
                                self.st.index.forget(&gone, self.st.persist);
                            }
                            let secs: Vec<&Section> = self.store.notebooks.iter().flat_map(|n| &n.sections).collect();
                            if self.st.persist { for s in secs.iter().filter(|s| s.sealed() && old_secs.get(&s.id) == Some(&false)) { locks::lock_in_backups(&data_dir(), s); } }
                            let rescued: Vec<String> = secs.iter().filter(|s| s.lock.is_none() && !old_secs.contains_key(&s.id) && s.name.ends_with("(other device)")).map(|s| s.name.clone()).collect();
                            self.st.full_save = true;
                            self.st.undo.clear();
                            self.st.redo.clear();
                            self.st.cache.derived_dirty = true;
                            match open.and_then(|id| self.locate(id)) { Some((a, b, c)) => (self.nb, self.sec, self.pg) = (a, b, c), None => self.clamp() }
                            self.save_now();
                            self.st.say(if down > 0 { tf!("Synced changes from your other devices ({down} file(s)).", down = down) } else { "Synced changes from your other devices.".into() });
                            if let Some(name) = rescued.first() { self.st.say(tf!("A section was locked on another device while you wrote in it here. Those pages are in “{name}”: unlock the section and move them back in.", name = name)); }
                        }
                        self.st.sync_st.pending = meanwhile;
                        self.st.sync_st.ok = now_secs();
                        self.st.sync_st.err.clear();
                        if self.st.dirty { self.save_now(); self.st.sync_st.pending = meanwhile; }
                        if !self.st.dirty && self.st.persist { if let Err(e) = base.commit(&data_dir()) { self.st.sync_st.err = e; } }
                        if let Some(p) = problem { self.st.say(format!("⚠ {p}")); }
                    }
                    Err(e) => { self.st.sync_st.err = e; self.st.sync_st.pending = true; }
                }
            }
            Err(e) => {
                self.st.sync_st.err = e;
                self.st.sync_st.pending = true;
                self.st.sync_st.at = now_f() + 40.0;
            }
        }
    }

    fn cancel_capture(&mut self, msg: &str) {
        if let Some(c) = self.st.canvas.capture.take() {
            (self.st.p.show_nav, self.st.p.show_ai, self.st.p.ribbon_open) = c.keep;
            if let Some(v) = c.view { self.st.canvas.views.insert(c.page, v); }
            match c.scale { Some(s) => { self.st.canvas.scales.insert(c.page, s); } None => { self.st.canvas.scales.remove(&c.page); } }
            if !msg.is_empty() { self.st.say(msg); }
        }
    }

    fn toggle_record(&mut self) {
        if let Some((rec, pid, id, start)) = self.st.audio.recorder.take() {
            let file = rec.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let r = rec.stop();
            #[cfg(target_os = "android")]
            droid::recording(false);
            match r {
                Ok(secs) => {
                    let _ = std::fs::remove_file(rec_marker(id));
                    if self.attach_recording(pid, Recording { id, file, start, secs }) {
                        self.st.say("Recording saved — press ▶ under the title, or tap a note to hear that moment.");
                    } else {
                        self.st.say("The page you recorded on was deleted, so the recording was kept on this page.");
                    }
                }
                Err(e) => { self.st.say(format!("⚠ {e}")); self.recover_recordings(); }
            }
            return;
        }
        if !self.st.p.mic_ok { self.st.modals.push(ActiveModal::MicPrompt); return; }
        if !audio::mic_granted() {
            match audio::mic_request() {
                Ok(()) => { self.st.audio.mic_wait = Some(now_f()); self.st.say("Waiting for microphone permission…"); }
                Err(e) => self.st.say(tf!("⚠ Couldn't ask Android for the microphone ({e}). Allow it in the system settings under Apps ▸ Leaf Notes ▸ Permissions.", e = e)),
            }
            return;
        }
        let id = new_id();
        let dir = data_dir().join("files");
        let _ = std::fs::create_dir_all(&dir);
        match audio::Recorder::start(&dir.join(format!("{id}.wav"))) {
            Ok(r) => {
                let (pid, start) = (self.page().id, now_f());
                let _ = storage::write_atomic(&rec_marker(id), serde_json::json!({ "page": pid, "start": start }).to_string().as_bytes());
                self.st.audio.recorder = Some((r, pid, id, start));
                #[cfg(target_os = "android")]
                droid::recording(true);
                self.st.say("Recording — everything you write now is linked to this moment.");
            }
            Err(e) => self.st.say(format!("⚠ {e}")),
        }
    }

    fn attach_recording(&mut self, pid: u64, rec: Recording) -> bool {
        let found = self.find_page(pid).is_some();
        let on = if found { pid } else { self.page().id };
        for (i, p) in self.st.undo.iter_mut().chain(self.st.redo.iter_mut()) { if *i == on { p.recordings.push(rec.clone()); } }
        if let Some(p) = self.find_page(on) { p.recordings.push(rec); }
        self.st.dirty = true;
        found
    }

    fn recover_recordings(&mut self) {
        let dir = data_dir().join("files");
        let mut found = 0;
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(id) = name.strip_suffix(".rec.part").and_then(|n| n.parse::<u64>().ok()) else { continue };
            let m: serde_json::Value = std::fs::read(e.path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
            let (pid, start) = (m["page"].as_u64().unwrap_or(0), m["start"].as_f64().unwrap_or(0.0));
            if let Some(secs) = audio::finish_unfinished(&dir.join(format!("{id}.wav"))) {
                let known = self.store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).any(|p| p.recordings.iter().any(|r| r.id == id));
                if !known && secs > 0.0 {
                    self.attach_recording(pid, Recording { id, file: format!("{id}.wav"), start, secs });
                    found += 1;
                }
            }
            let _ = std::fs::remove_file(e.path());
        }
        if found > 0 { self.st.dirty = true; self.st.say(tf!("Recovered {n} unfinished recording(s).", n = found)); }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        // Android's Back: closes whatever is on top, one layer per press.
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::BrowserBack)) { self.back(ctx); return; }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            let lock_busy = matches!(self.st.locks.pending, Some(PendingLockOp::Lock { .. }));
            if self.st.canvas.popup.take().is_some() { return; }
            if self.st.canvas.capture.is_some() { self.cancel_capture("PDF export cancelled."); return; }
            if egui::Popup::is_any_open(ctx) { egui::Popup::close_all(ctx); return; }
            if self.st.modals.handle_escape(lock_busy) { return; }
            if self.st.nav.rename.take().is_some() { return; }
            if self.st.canvas.current.take().is_some() { self.st.canvas.anchor = None; return; }
            if self.st.canvas.editing.take().is_some() { return; }
            if !self.st.canvas.selected.is_empty() { self.st.clear_selection(); return; }
            if !self.st.nav.search.is_empty() || self.st.nav.tag_filter.is_some() || self.st.nav.search_open {
                self.st.nav.search.clear();
                self.st.nav.tag_filter = None;
                self.st.nav.search_open = false;
                return;
            }
            return;
        }
        // Global canvas/navigation/window shortcuts pause whenever any modal dialog is open.
        if !self.st.modals.is_empty() { return; }

        let typing = ctx.egui_wants_keyboard_input();
        let cmd = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, k));
        if !typing {
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)) || cmd(Key::Y) { self.undo(true); }
            if cmd(Key::Z) { self.undo(false); }
            let key = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
            if key(Key::T) { self.st.canvas.tool = Tool::Type; }
            if key(Key::P) { self.st.canvas.tool = Tool::Pen; }
            if key(Key::H) { self.st.canvas.tool = Tool::Highlighter; }
            if key(Key::E) { self.st.canvas.tool = Tool::Eraser; }
            if key(Key::L) { self.st.canvas.tool = Tool::Lasso; }
            if !self.st.canvas.selected.is_empty() && (key(Key::Delete) || key(Key::Backspace)) { self.delete_selected_ink(); }
            if self.st.canvas.tool == Tool::Lasso {
                if cmd(Key::A) { self.st.canvas.selected = self.page().strokes.iter().map(|s| s.id).collect(); }
                let ev = |f: fn(&egui::Event) -> bool| ctx.input(|i| i.events.iter().any(f));
                if !self.st.canvas.selected.is_empty() && (cmd(Key::C) || ev(|e| matches!(e, egui::Event::Copy))) { self.copy_ink(ctx, false); }
                if !self.st.canvas.selected.is_empty() && (cmd(Key::X) || ev(|e| matches!(e, egui::Event::Cut))) { self.copy_ink(ctx, true); }
            }
            if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(t) if is_ink_mark(t)))) || cmd(Key::V) { self.paste_ink(); }
            if let (Some(pl), false) = (&self.st.audio.player, ctx.input(|i| i.modifiers.alt)) {
                if key(Key::ArrowLeft) { pl.seek(pl.time() - 10.0); }
                if key(Key::ArrowRight) { pl.seek(pl.time() + 10.0); }
            }
            for (i, k) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9].into_iter().enumerate() {
                if i < self.st.p.pens.len() && key(k) { self.st.p.pen = i; self.st.canvas.tool = Tool::Pen; }
            }
            if key(Key::S) {
                self.st.canvas.tool = Tool::Shape(match self.st.canvas.tool {
                    Tool::Shape(Shape::Line) => Shape::Arrow,
                    Tool::Shape(Shape::Arrow) => Shape::Rect,
                    Tool::Shape(Shape::Rect) => Shape::Ellipse,
                    _ => Shape::Line,
                });
            }
            let alt = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::ALT, k));
            if alt(Key::Equals) || alt(Key::Plus) { self.st.canvas.zoom_req = Some(ZoomReq::By(1.25)); }
            if alt(Key::Minus) { self.st.canvas.zoom_req = Some(ZoomReq::By(1.0 / 1.25)); }
            if alt(Key::Num0) { self.st.canvas.zoom_req = Some(ZoomReq::To(1.0)); }
            if alt(Key::Num9) { self.st.canvas.zoom_req = Some(ZoomReq::FitWidth); }
        }
        if cmd(Key::N) { self.new_page(None); }
        let (nb, sec, pg) = (self.nb, self.sec, self.pg);
        let pages = self.store.notebooks[nb].sections[sec].pages.len();
        if cmd(Key::PageDown) && pg + 1 < pages { self.go_to(nb, sec, pg + 1); }
        if cmd(Key::PageUp) && pg > 0 { self.go_to(nb, sec, pg - 1); }
        let (mb, mf) = ctx.input(|i| (i.pointer.button_pressed(egui::PointerButton::Extra1), i.pointer.button_pressed(egui::PointerButton::Extra2)));
        if mb || (!typing && ctx.input_mut(|i| i.consume_key(Modifiers::ALT, Key::ArrowLeft))) { self.go_back(true); }
        if mf || (!typing && ctx.input_mut(|i| i.consume_key(Modifiers::ALT, Key::ArrowRight))) { self.go_back(false); }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F2)) { self.st.nav.focus_id = Some(egui::Id::unique("page-title")); }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F1)) { self.st.modals.push(ActiveModal::Shortcuts); }
        if !typing && cmd(Key::D) { self.duplicate_page(nb, sec, pg); }
        if !typing && cmd(Key::B) { self.st.p.show_nav ^= true; self.st.dirty = true; }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::T)) { self.st.nav.tasks ^= true; if self.st.nav.tasks { self.st.p.show_nav = true; } }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::R)) { self.start_review(); }
        if cmd(Key::Comma) { self.st.modals.open_settings(0); }
        if cmd(Key::F) {
            if self.st.nav.search_icon_mode {
                self.st.p.show_nav = true;
                if ctx.content_rect().width() < 760.0 { self.st.p.show_ai = false; }
                self.st.nav.tasks = false;
                self.st.nav.search_open = true;
                self.st.nav.focus_id = Some(egui::Id::unique("search-panel-input"));
            } else {
                if self.st.nav.narrow { self.st.p.show_nav = true; }
                self.st.nav.focus_id = Some(egui::Id::unique("search"));
            }
        }
        if cmd(Key::K) || cmd(Key::P) { self.st.modals.push(ActiveModal::Switcher { query: String::new(), sel: 0 }); }
    }

    fn back(&mut self, ctx: &egui::Context) {
        let lock_busy = matches!(self.st.locks.pending, Some(PendingLockOp::Lock { .. }));
        if egui::Popup::is_any_open(ctx) { egui::Popup::close_all(ctx); }
        else if self.st.canvas.popup.take().is_some() {}
        else if self.st.canvas.capture.is_some() { self.cancel_capture("PDF export cancelled."); }
        else if self.st.modals.handle_escape(lock_busy) || self.st.nav.rename.take().is_some() || self.st.canvas.editing.take().is_some() {}
        else if !self.st.canvas.selected.is_empty() { self.st.clear_selection(); }
        else if self.st.nav.narrow && self.st.p.show_ai { self.st.p.show_ai = false; }
        else if !self.st.nav.search.is_empty() || self.st.nav.tag_filter.is_some() || self.st.nav.search_open {
            self.st.nav.search.clear();
            self.st.nav.tag_filter = None;
            self.st.nav.search_open = false;
        }
        else if self.st.nav.narrow && self.st.p.show_nav { self.st.p.show_nav = false; }
        else if !self.st.nav.nav_back.is_empty() { self.go_back(true); }
        else {
            #[cfg(target_os = "android")]
            droid::to_background();
        }
    }

    pub(crate) fn apply_ink(&mut self, r: InkReview, keep: bool) {
        let Some(p) = self.find_page(r.page) else { return };
        let snap = p.clone();
        let (x, y) = if keep {
            (r.at[0], r.at[1] + p.strokes.iter().filter(|s| r.strokes.contains(&s.id)).flat_map(|s| s.points.iter()).map(|q| q[1]).fold(r.at[1], f32::max) - r.at[1] + 16.0)
        } else {
            (r.at[0], r.at[1])
        };
        if !keep { p.strokes.retain(|s| !r.strokes.contains(&s.id)); }
        let kind = if r.math { Kind::Math { src: r.text } } else { Kind::Text { text: r.text, heading: 0 } };
        p.blocks.push(block(x.max(0.0), y.max(0.0), r.width, kind));
        self.st.snap(&snap);
        self.st.canvas.selected.clear();
        self.st.say(if keep { "Added the typed version under the handwriting." } else { "Ink converted. Undo brings the handwriting back." });
    }

    pub(crate) fn copy_ink(&mut self, ctx: &egui::Context, cut: bool) {
        let sel = self.st.canvas.selected.clone();
        if sel.is_empty() { return; }
        self.st.canvas.ink_clip = self.page().strokes.iter().filter(|s| sel.contains(&s.id)).cloned().collect();
        ctx.copy_text(tr(INK_MARK).to_string());
        if cut { self.delete_selected_ink(); }
        self.st.say(if cut { "Cut. Paste with Ctrl+V." } else { "Copied. Paste with Ctrl+V, also on another page." });
    }

    pub(crate) fn paste_ink(&mut self) {
        if self.st.canvas.ink_clip.is_empty() { return; }
        let copies: Vec<Stroke> = self.st.canvas.ink_clip.iter().map(|s| Stroke { id: new_id(), points: s.points.iter().map(|p| [p[0] + 24.0, p[1] + 24.0, p[2]]).collect(), ..s.clone() }).collect();
        self.st.canvas.ink_clip = copies.clone();
        let ids = copies.iter().map(|s| s.id).collect();
        let page = self.page();
        let snap = page.clone();
        page.strokes.extend(copies);
        self.st.snap(&snap);
        self.st.canvas.selected = ids;
        self.st.canvas.tool = Tool::Lasso;
    }

    fn delete_selected_ink(&mut self) {
        let sel = std::mem::take(&mut self.st.canvas.selected);
        let page = self.page();
        let snap = page.clone();
        page.strokes.retain(|s| !sel.contains(&s.id));
        self.st.snap(&snap);
    }
}

impl eframe::App for NotesApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.st.canvas.frame_force = None;
        if std::mem::take(&mut self.st.canvas.restore_tool) {
            if let Some(t) = self.st.canvas.tool_before_erase.take() { self.st.canvas.tool = t; }
        }
        let mut tap = 0;
        ctx.input(|i| {
            let touched = i.events.iter().any(|e| matches!(e, egui::Event::Touch { .. }));
            for e in &i.events {
                match e {
                    egui::Event::Touch { force: None, phase, id, pos, .. } => {
                        let (cs, now) = (&mut self.st.canvas, i.time);
                        match phase {
                            egui::TouchPhase::Start => {
                                if cs.fingers.is_empty() { cs.gesture = (now, 0, false, f64::MAX); }
                                cs.fingers.insert(id.0, *pos);
                                cs.gesture.1 = cs.gesture.1.max(cs.fingers.len());
                            }
                            egui::TouchPhase::Move => if cs.fingers.get(&id.0).is_some_and(|p| p.distance(*pos) > 10.0) { cs.gesture.2 = true; },
                            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                                if *phase == egui::TouchPhase::Cancel { cs.gesture.2 = true; }
                                cs.gesture.3 = cs.gesture.3.min(now);
                                if cs.fingers.remove(&id.0).is_some() && cs.fingers.is_empty() && !cs.gesture.2 && now - cs.gesture.0 < 0.35 && now - cs.gesture.3 < 0.08 {
                                    tap = cs.gesture.1;
                                }
                            }
                        }
                        if *phase == egui::TouchPhase::Start {
                            self.st.canvas.input = Input::Finger;
                            if !self.st.p.pen_detect { self.st.p.pen_detect = true; self.st.dirty = true; }
                        }
                    }
                    egui::Event::Touch { force, phase, .. } => {
                        let (pressure, erase) = match *force { Some(f) if f < 0.0 => (Some(-f), true), Some(f) if f > 1.0 => (Some(f - 1.0), true), f => (f, false) };
                        if let Some(f) = pressure {
                            self.st.canvas.force = Some(f);
                            self.st.canvas.frame_force = Some(f);
                            self.st.canvas.last_pen_at = i.time;
                        }
                        if *phase == egui::TouchPhase::Start && erase && self.st.canvas.tool_before_erase.is_none() {
                            self.st.canvas.tool_before_erase = Some(self.st.canvas.tool);
                            self.st.canvas.tool = Tool::Eraser;
                        }
                        if matches!(phase, egui::TouchPhase::End | egui::TouchPhase::Cancel) && self.st.canvas.tool_before_erase.is_some() {
                            self.st.canvas.restore_tool = true;
                        }
                        if *phase == egui::TouchPhase::Start {
                            self.st.canvas.input = if force.is_some() { Input::Pen } else { Input::Finger };
                            if !self.st.p.pen_detect { self.st.p.pen_detect = true; self.st.dirty = true; }
                        }
                    }
                    egui::Event::PointerButton { pressed: true, .. } if !touched && i.time - self.st.canvas.last_pen_at > 0.25 => {
                        self.st.canvas.input = Input::Mouse;
                    }
                    egui::Event::WindowFocused(false) => self.st.canvas.fingers.clear(),
                    _ => {}
                }
            }
        });
        if tap >= 2 && self.st.modals.is_empty() {
            self.st.canvas.editing = None;
            self.st.canvas.current = None;
            if self.undo(tap >= 3) {
                self.st.say(if tap >= 3 { "Redone (three-finger tap)." } else { "Undone (two-finger tap). Tap with three fingers to redo." });
            }
        }
        if self.st.p.follow_system {
            if let Some(t) = ctx.system_theme() {
                let dark = t == egui::Theme::Dark;
                if dark != self.st.p.dark { self.st.p.dark = dark; apply_prefs(&ctx, &self.st.p); }
            }
        }
        for img in ctx.input(|i| i.events.iter().filter_map(|e| if let egui::Event::PasteImage(img) = e { Some(img.clone()) } else { None }).collect::<Vec<_>>()) {
            let file = format!("{}.png", new_id());
            let dir = data_dir().join("files");
            let pic = image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, img.as_raw().to_vec()).map(image::DynamicImage::ImageRgba8)
                .map(|p| if p.width().max(p.height()) > 2400 { p.resize(2400, 2400, image::imageops::FilterType::Triangle) } else { p });
            let r = std::fs::create_dir_all(&dir).map_err(|e| e.to_string())
                .and_then(|_| pic.ok_or_else(|| "not a picture".to_string())?.save(dir.join(format!(".{file}"))).map_err(|e| e.to_string()))
                .and_then(|_| {
                    let part = dir.join(format!(".{file}"));
                    storage::set_private_perms(&part);
                    std::fs::rename(part, dir.join(&file)).map_err(|e| e.to_string())
                });
            match r {
                Ok(()) => { self.insert(Kind::Image { file }); self.st.canvas.editing = None; self.st.say("Pasted the picture onto the page."); }
                Err(e) => self.st.say(tf!("⚠ Couldn't paste the picture: {e}", e = e)),
            }
        }
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            let p = f.path().to_path_buf();
            if p.as_os_str().is_empty() { continue; }
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
                self.st.modals.push(ActiveModal::DropPdf { path: p });
            } else {
                self.insert_file(&p, Ins::Printout);
            }
        }
        // Unattended idle auto-relock: pauses while recording; schedules wakeup even when unattended.
        let after = self.st.p.lock_after_min as f64 * 60.0;
        let user_active = ctx.input(|i| !i.events.is_empty() || i.pointer.is_moving()) || self.st.audio.recorder.is_some();
        if !self.st.locks.keys.is_empty() && after > 0.0 {
            let idle = now_f() - self.st.locks.last_input;
            if idle > after && self.st.audio.recorder.is_none() {
                for id in self.st.locks.keys.keys().copied().collect::<Vec<_>>() { self.relock(id); }
                self.st.say("Locked sections were closed again after a while without use.");
            } else {
                let left = (after - idle).max(1.0);
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.min(30.0)));
            }
        }
        if user_active { self.st.locks.last_input = now_f(); }
        if std::mem::take(&mut self.st.theme_dirty) { apply_theme(&ctx, self.st.p.dark); }
        if !self.st.opened_args {
            self.st.opened_args = true;
            for f in OPEN_AT_START.get().cloned().unwrap_or_default() {
                match f.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
                    Some("md" | "markdown" | "txt") => self.import_markdown(vec![f]),
                    Some("pdf") => self.st.modals.push(ActiveModal::DropPdf { path: f }),
                    _ => self.insert_file(&f, Ins::Printout),
                }
            }
        }
        self.handle_keys(&ctx);
        self.poll_ai(&ctx);
        self.poll_bg(&ctx);
        self.poll_locks(&ctx);
        if let Some(worker) = &self.st.save_worker {
            if let Some(t) = worker.poll_backup_done() {
                self.st.p.last_backup = t;
            }
            if let Some(e) = worker.poll_error() {
                self.st.say(tf!("⚠ Could not save: {e}", e = e));
                self.st.dirty = true;
            }
        }
        self.clamp();

        #[cfg(target_os = "android")]
        {
            let t = ctx.input(|i| i.time);
            if t - self.st.bars_at > if self.st.canvas.editing.is_some() { 0.3 } else { 1.0 } {
                self.st.bars_at = t;
                let mut px = system_bars().unwrap_or([0; 4]);
                if let Some(app) = &self.android {
                    let r = app.content_rect();
                    if let Some(w) = app.native_window() {
                        let (ww, wh) = (w.width(), w.height());
                        if r.right > r.left && r.bottom > r.top {
                            let native = [r.top, wh - r.bottom, r.left, ww - r.right];
                            for i in 0..4 { px[i] = px[i].max(native[i].max(0)); }
                        }
                    }
                }
                self.st.bars = px.map(|p| p as f32 / ctx.pixels_per_point());
            }
            if self.st.bars == [0.0; 4] && t < 15.0 { ctx.request_repaint_after(std::time::Duration::from_millis(500)); }
        }
        let [top, bottom, left, right] = self.st.bars;
        let blank = egui::Frame::new().fill(ui.visuals().panel_fill);
        if top > 0.0 { egui::Panel::top("bar-top").exact_size(top).resizable(false).show_separator_line(false).frame(blank).show(ui, |_| {}); }
        if bottom > 0.0 { egui::Panel::bottom("bar-bottom").exact_size(bottom).resizable(false).show_separator_line(false).frame(blank).show(ui, |_| {}); }
        if left > 0.0 { egui::Panel::left("bar-left").exact_size(left).resizable(false).show_separator_line(false).frame(blank).show(ui, |_| {}); }
        if right > 0.0 { egui::Panel::right("bar-right").exact_size(right).resizable(false).show_separator_line(false).frame(blank).show(ui, |_| {}); }

        let narrow = ctx.content_rect().width() < 1000.0;
        if narrow != self.st.nav.narrow {
            self.st.nav.narrow = narrow;
            self.st.p.show_nav = !narrow;
            if narrow { self.st.p.show_ai = false; }
        }
        let short = ctx.content_rect().height() < 520.0;
        if short != self.st.nav.short {
            self.st.nav.short = short;
            self.st.p.ribbon_open = !short;
        }
        let touch = cfg!(target_os = "android") || matches!(self.st.canvas.input, Input::Finger | Input::Pen);
        // Defer touch_ui layout spacing shift while an active stroke is in flight so coordinates don't jump mid-stroke.
        if touch != self.st.nav.touch_ui && self.st.canvas.current.is_none() {
            self.st.nav.touch_ui = touch;
            ctx.all_styles_mut(|s| {
                s.spacing.interact_size.y = if touch { 36.0 } else { 26.0 };
                s.spacing.button_padding = if touch { Vec2::new(14.0, 8.0) } else { Vec2::new(10.0, 5.0) };
                s.spacing.item_spacing = if touch { Vec2::new(10.0, 8.0) } else { Vec2::new(8.0, 6.0) };
            });
        }
        self.st.index.tick(&self.store, data_dir().join("files"), self.st.persist, now_f());
        #[cfg(target_os = "android")]
        self.take_incoming();
        #[cfg(target_os = "android")]
        { self.st.canvas.typing_kb = self.st.canvas.editing.is_some() && self.st.bars[1] > 150.0; }
        self.title_bar(ui);
        self.ribbon(ui);
        self.status_bar(ui);
        if self.st.p.show_nav { self.nav(ui); }
        if self.st.p.show_ai { self.ai_pane(ui); }
        self.clamp();
        let (nb, sec, pg) = (self.nb, self.sec, self.pg);
        let crumb = format!("{}  ·  {}", self.store.notebooks[nb].name, self.store.notebooks[nb].sections[sec].name);
        let mut page_action = None;

        // Recompute derived library stats only when dirty, on page change, or once a minute (for newly due cards).
        let here_id = self.store.notebooks[nb].sections[sec].pages[pg].id;
        if self.st.cache.derived_dirty || self.st.cache.backlinks_for != here_id || now_f() - self.st.cache.derived_at > 60.0 {
            self.st.cache.derived_dirty = false;
            self.st.cache.derived_at = now_f();
            self.st.cache.backlinks_for = here_id;
            self.st.cache.due_now = self.due_cards().len();
            self.st.cache.open_tasks_n = self.open_tasks().len();
            self.st.cache.conflict_pair = self.conflict_pair();
            let here = self.store.notebooks[nb].sections[sec].pages[pg].title.trim().to_lowercase();
            let all = || self.store.notebooks.iter().flat_map(|n| n.sections.iter()).flat_map(|s| s.pages.iter());
            rich::set_known_titles(all().map(|p| p.title.trim().to_lowercase()).filter(|t| !t.is_empty()).collect());
            self.st.cache.page_titles = all().filter(|p| p.id != here_id).map(|p| p.title.clone()).filter(|t| !t.trim().is_empty()).collect();
            let needle = format!("[[{here}]]");
            let mut backlinks = vec![];
            if !here.is_empty() {
                for (a, n) in self.store.notebooks.iter().enumerate() {
                    for (b, s) in n.sections.iter().enumerate() {
                        for (c, p) in s.pages.iter().enumerate() {
                            if (a, b, c) != (nb, sec, pg) && p.blocks.iter().any(|bl| match &bl.kind {
                                Kind::Text { text, .. } => text.to_lowercase().contains(&needle),
                                Kind::Checklist { items } => items.iter().any(|(_, t)| t.to_lowercase().contains(&needle)),
                                _ => false,
                            }) { backlinks.push((p.title.clone(), (a, b, c))); }
                        }
                    }
                }
            }
            self.st.cache.backlinks = backlinks;
        }
        let backlinks = self.st.cache.backlinks.clone();
        let fav = self.st.p.favorites.contains(&here_id);
        let conflict = self.st.cache.conflict_pair;
        // Keep Nav and AI mutually exclusive on narrower windows so sidebars never crush the canvas.
        if ctx.content_rect().width() < 760.0 && self.st.p.show_nav && self.st.p.show_ai {
            self.st.p.show_ai = false;
        }
        // On phones / narrow drawers, when a full-width drawer (Nav or AI) is open, skip CentralPanel so no sliver renders.
        let phone_covered = ctx.content_rect().width() < DRAWER_COVER_W && (self.st.p.show_nav || self.st.p.show_ai);
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            if phone_covered { return; }
            if !self.st.persist && self.screenshot.is_none() {
                let dark = ui.visuals().dark_mode;
                let bg = if dark { Color32::from_rgb(72, 34, 28) } else { Color32::from_rgb(254, 226, 226) };
                let fg = if dark { Color32::from_rgb(254, 202, 202) } else { Color32::from_rgb(153, 27, 27) };
                egui::Frame::new().fill(bg).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 7)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new(tr("⚠ Read-only mode — changes won't be saved.")).color(fg).strong());
                });
                ui.add_space(4.0);
            }
            if let Some((orig, copy)) = conflict {
                if NotesApp::conflict_banner(ui) {
                    self.st.modals.push(ActiveModal::Compare { orig, copy });
                }
            }
            let page = &mut self.store.notebooks[nb].sections[sec].pages[pg];
            page_action = canvas::page_view(ui, page, &mut self.st, &crumb, &backlinks, fav, &self.store.study);
        });
        if let Some(t) = self.st.canvas.open_link.take() { self.open_title(&t); }
        if let Some((f, b)) = self.st.canvas.make_card.take() { self.add_card(f, b); }
        if let Some(id) = self.st.jobs.transcribe_req.take() { self.start_transcribe(id); }
        if self.st.canvas.capture.as_ref().is_some_and(|c| c.page != self.store.notebooks[nb].sections[sec].pages[pg].id) {
            self.cancel_capture("PDF export cancelled because the page changed.");
        }
        if std::mem::take(&mut self.st.audio.rec_toggle) { self.toggle_record(); }
        if let Some(e) = self.st.audio.recorder.as_ref().and_then(|(r, ..)| r.error.lock().ok().and_then(|mut g| g.take())) {
            self.st.say(tf!("⚠ The microphone reported a problem ({e}). The recording may have gaps.", e = e));
        }
        if let Some(since) = self.st.audio.mic_wait {
            if audio::mic_granted() { self.st.audio.mic_wait = None; self.toggle_record(); }
            else if now_f() - since > 60.0 { self.st.audio.mic_wait = None; self.st.say("⚠ The microphone wasn't allowed. You can allow it for Leaf Notes in your phone's settings."); }
            else { ctx.request_repaint_after(std::time::Duration::from_millis(300)); }
        }
        if self.st.audio.recorder.is_some() || self.st.audio.player.as_ref().is_some_and(|p| p.playing()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
        if self.st.canvas.capture.as_ref().is_some_and(|c| c.done()) {
            let c = self.st.canvas.capture.take().unwrap_or_else(|| unreachable!("checked above"));
            (self.st.p.show_nav, self.st.p.show_ai, self.st.p.ribbon_open) = c.keep;
            if let Some(v) = c.view { self.st.canvas.views.insert(c.page, v); }
            match c.scale { Some(s) => { self.st.canvas.scales.insert(c.page, s); } None => { self.st.canvas.scales.remove(&c.page); } }
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let r = export::write_image_pdf_tiles(&c.title, &c.tiles, &c.path).map(|n| {
                    shared(&c.path);
                    tf!("Exported {n} PDF page(s) to {out}", n = n, out = where_saved(&c.path))
                });
                let _ = tx.send(r);
            });
            self.st.jobs.bg_jobs.push(BgJob { kind: BgJobKind::Note, rx });
        }
        match page_action {
            Some(canvas::PageAction::Export) => self.export_page(),
            Some(canvas::PageAction::Retitled(old)) => { let (id, new) = (self.page().id, std::mem::replace(&mut self.page().title, old)); self.rename(id, new); }
            Some(canvas::PageAction::Copy) => { let md = self.page().plain_text(); ctx.copy_text(md); self.st.say("Page copied as Markdown."); }
            Some(canvas::PageAction::Delete) => { let id = self.page().id; self.delete(id); }
            Some(canvas::PageAction::Duplicate) => self.duplicate_page(nb, sec, pg),
            Some(canvas::PageAction::History) => self.show_history(),
            Some(canvas::PageAction::Unlock(pw)) => {
                let id = self.section().id;
                self.start_unlock_section(id, pw);
            }
            Some(canvas::PageAction::Go(a, b, c)) => self.go_to(a, b, c),
            Some(canvas::PageAction::Nav(back)) => self.go_back(back),
            Some(canvas::PageAction::Study) => self.start_deck(),
            Some(canvas::PageAction::Pdf(whole)) => self.export_pdf(whole),
            Some(canvas::PageAction::ToggleFav) => {
                let id = self.page().id;
                if fav { self.st.p.favorites.retain(|f| *f != id); } else { self.st.p.favorites.push(id); }
                self.st.dirty = true;
            }
            None => {}
        }
        self.dialogs(&ctx);

        if self.st.nav.commit_rename {
            self.st.nav.commit_rename = false;
            if let Some((_, id, name)) = self.st.nav.rename.take() {
                if !name.trim().is_empty() { self.rename(id, name.trim().to_string()); self.st.dirty = true; }
            }
        }

        let now_t = ctx.input(|i| i.time);
        let dt = ((now_t - self.st.prev_t).max(0.0) as f32).min(2.0);
        self.st.prev_t = now_t;
        self.st.since_save += dt;
        self.st.status_age += dt;
        if self.st.dirty && self.st.since_save > 1.5 { self.save_now(); }
        if self.st.dirty { ctx.request_repaint_after(std::time::Duration::from_millis(600)); }
        if self.st.persist && self.st.sync_st.settings.on() {
            let since = now_f() - self.st.sync_st.at;
            if (self.st.sync_st.now || since > 180.0 || (self.st.sync_st.pending && since > 20.0)) && !self.st.modals.has_sync_ask() && !self.st.dirty {
                self.start_sync(None);
            }
            ctx.request_repaint_after(std::time::Duration::from_secs(if self.st.sync_st.rx.is_some() { 1 } else { 20 }));
        }
        if !self.st.modals.has_settings() && self.st.sync_st.at.is_infinite() { self.st.sync_st.at = now_f(); }
        self.poll_sync();
        self.poll_transcribe();
        let t = ctx.input(|i| i.time);
        self.st.cache.poll_images(&ctx, t);
        if let Some(rx) = &self.st.jobs.update_rx {
            if let Ok(r) = rx.try_recv() {
                let manual = std::mem::take(&mut self.st.jobs.update_manual);
                match r {
                    Ok(Some(u)) => self.st.jobs.update = Some(u),
                    Ok(None) => {
                        self.st.jobs.update = None;
                        if manual { self.st.say("Leaf Notes is up to date."); }
                    }
                    Err(e) => {
                        if manual { self.st.say(format!("⚠ {e}")); }
                    }
                }
                self.st.jobs.update_rx = None;
            }
        }
        if self.st.jobs.transcribe.is_some() { ctx.request_repaint_after(std::time::Duration::from_millis(500)); }
        if let Some((_, rx, _)) = &self.st.sync_st.google_rx {
            match rx.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(500)),
                r => {
                    self.st.sync_st.google_rx = None;
                    match r {
                        Ok(Ok((token, email))) => {
                            (self.st.sync_st.settings.via, self.st.sync_st.settings.google_token, self.st.sync_st.settings.google_email) = (sync::Via::Google, token, email);
                            self.st.sync_st.err.clear();
                            self.st.sync_st.now = true;
                            self.st.dirty = true;
                            self.st.say("Signed in to Google Drive — syncing your notes.");
                        }
                        Ok(Err(e)) => self.st.sync_st.err = e,
                        Err(_) => {}
                    }
                }
            }
        }

        #[cfg(target_os = "android")]
        if let Some(app) = &self.android {
            let wants = ctx.egui_wants_keyboard_input();
            let now = ctx.input(|i| i.time);
            if wants { self.st.kb_idle_since = None; } else if self.st.kb_idle_since.is_none() { self.st.kb_idle_since = Some(now); }
            let close = !wants && self.st.kb_idle_since.is_some_and(|t| now - t > 0.4);
            let tapped = ctx.input(|i| i.pointer.primary_released());
            if wants && (!self.keyboard_shown || tapped) { app.show_soft_input(true); self.keyboard_shown = true; }
            else if close && self.keyboard_shown { app.hide_soft_input(true); self.keyboard_shown = false; }
            if !wants && self.keyboard_shown { ctx.request_repaint_after(std::time::Duration::from_millis(450)); }
            let busy = self.st.dirty || self.st.audio.recorder.is_some() || self.st.audio.player.is_some() || self.st.canvas.capture.is_some()
                || self.st.jobs.ai_busy() || self.st.jobs.bg_busy() || self.st.sync_st.rx.is_some() || self.st.sync_st.google_rx.is_some() || self.st.focus.is_some()
                || self.st.audio.mic_wait.is_some() || self.st.bars == [0.0; 4] || (!wants && self.keyboard_shown) || self.st.index.busy() || self.st.jobs.transcribe.is_some() || now_f() - self.st.picking_since < 120.0;
            WAKE_FAST.store(busy, std::sync::atomic::Ordering::Relaxed);
            static TICKER: std::sync::Once = std::sync::Once::new();
            static WAKE_CTX: std::sync::Mutex<Option<egui::Context>> = std::sync::Mutex::new(None);
            if let Ok(mut c) = WAKE_CTX.lock() { if c.as_ref() != Some(&ctx) { *c = Some(ctx.clone()); } }
            TICKER.call_once(|| {
                std::thread::spawn(move || {
                    let mut idle = 0u32;
                    while ANDROID_ACTIVE.load(std::sync::atomic::Ordering::Acquire) {
                        let fast = WAKE_FAST.load(std::sync::atomic::Ordering::Relaxed);
                        std::thread::sleep(std::time::Duration::from_millis(if fast { 400 } else { 2000 }));
                        if !ANDROID_ACTIVE.load(std::sync::atomic::Ordering::Acquire) { break; }
                        idle += 1;
                        if fast || idle >= 15 || droid::has_incoming() {
                            idle = 0;
                            if let Some(c) = WAKE_CTX.lock().ok().and_then(|c| c.clone()) { c.request_repaint(); }
                        }
                    }
                });
            });
        }

        if let Some(path) = self.screenshot.clone() {
            self.frames += 1;
            SHOT_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ctx.request_repaint();
            let t = ctx.input(|i| i.time);
            if (self.frames >= 120 || t >= 2.5) && !self.shot_requested {
                self.shot_requested = true;
                let done = self.shot_done.clone();
                ctx.request_screenshot(move |img| {
                    if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
                    if let Err(e) = image::save_buffer(&path, img.as_raw(), img.size[0] as u32, img.size[1] as u32, image::ColorType::Rgba8) { eprintln!("screenshot failed: {e}"); }
                    done.store(true, std::sync::atomic::Ordering::SeqCst);
                });
            }
            if self.shot_done.load(std::sync::atomic::Ordering::SeqCst) || self.frames > 600 || t > 30.0 { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.st.audio.recorder.is_some() { self.toggle_record(); }
        if self.st.canvas.capture.is_some() { self.cancel_capture(""); }
        // If a background sync is already in flight, give it a brief window to finish cleanly so its merge & base aren't lost.
        if self.st.sync_st.rx.is_some() {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(4);
            while self.st.sync_st.rx.is_some() && std::time::Instant::now() < until {
                self.poll_sync();
                if self.st.sync_st.rx.is_some() { std::thread::sleep(std::time::Duration::from_millis(40)); }
            }
        }
        let unsynced = self.st.sync_st.pending || self.st.dirty;
        self.st.full_save = true;
        self.save_now();
        if let Some(w) = &self.st.save_worker { w.flush(); }
        self.st.index.flush_with(Some(&self.store), self.st.persist);
        if self.st.persist && self.st.sync_st.settings.on() && self.st.sync_st.rx.is_none() && !self.st.modals.has_sync_ask() && unsynced {
            let sealed = self.sealed();
            if let Ok(sync::Outcome::Synced { store, base, .. }) = sync::run(&self.st.sync_st.settings, &data_dir(), sealed.as_ref().unwrap_or(&self.store), None, true) {
                self.store = self.reopen(store);
                self.st.full_save = true;
                self.save_now();
                if let Some(w) = &self.st.save_worker { w.flush(); }
                let had_err = self.st.save_worker.as_ref().and_then(|w| w.poll_error()).is_some();
                if !self.st.dirty && !had_err { let _ = base.commit(&data_dir()); }
            }
        }
        #[cfg(target_os = "android")]
        {
            ANDROID_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
            audio::clear_activity();
        }
    }

    fn clear_color(&self, v: &egui::Visuals) -> [f32; 4] { v.panel_fill.to_normalized_gamma_f32() }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    if let Some(p) = app.internal_data_path() { let _ = ANDROID_DATA.set(p); }
    ANDROID_ACTIVE.store(true, std::sync::atomic::Ordering::Release);
    audio::set_activity(app.activity_as_ptr());
    let options = eframe::NativeOptions { android_app: Some(app.clone()), ..Default::default() };
    let _ = eframe::run_native("Leaf Notes", options, Box::new(move |cc| Ok(Box::new(NotesApp::new(cc, None).with_android(app)))));
    ANDROID_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
    audio::clear_activity();
}

#[cfg(target_os = "android")]
fn system_bars() -> Option<[i32; 4]> {
    use jni::{objects::{JIntArray, JObject}, strings::JNIString, Env};
    fn ask(env: &mut Env<'_>, act: &JObject<'_>) -> jni::errors::Result<Option<[i32; 4]>> {
        let obj = env.call_method(act, JNIString::new("getSystemBarInsets"), jni::jni_sig!("()[I"), &[])?.l()?;
        if obj.is_null() { return Ok(None); }
        let arr = env.cast_local::<JIntArray>(obj)?;
        let mut v = [0i32; 4];
        arr.get_region(env, 0, &mut v)?;
        Ok(Some(v))
    }
    audio::with_activity(|env, act| {
        let r = ask(env, &act);
        if r.is_err() { let _ = env.exception_clear(); }
        r
    }).ok().flatten()
}

#[cfg(test)]
mod picture_tests {
    #[test]
    fn big_pictures_are_stored_smaller() {
        let dir = std::env::temp_dir().join(format!("ln-pic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (big, small) = (dir.join("big.png"), dir.join("small.png"));
        image::RgbaImage::new(3000, 100).save(&big).unwrap();
        image::RgbaImage::new(300, 100).save(&small).unwrap();
        super::store_picture(&big, &dir.join("a.png")).unwrap();
        super::store_picture(&small, &dir.join("b.png")).unwrap();
        assert_eq!(image::image_dimensions(dir.join("a.png")).unwrap(), (2400, 80));
        assert_eq!(std::fs::read(dir.join("b.png")).unwrap(), std::fs::read(&small).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod undo_tests {
    use super::*;
    #[test]
    fn going_to_a_new_empty_notebook_gives_it_a_page() {
        let mut app = crate::safety::tests::app();
        app.store.notebooks.push(Notebook { id: new_id(), name: "New".into(), color: [0, 0, 0], sections: vec![] });
        let last = app.store.notebooks.len() - 1;
        app.go_to(last, 0, 0);
        assert!(app.store.notebooks[app.nb].sections[app.sec].pages.get(app.pg).is_some(), "the side panel reads this right away");
    }

    #[test]
    fn undo_keeps_recordings_and_skips_empty_steps() {
        let mut app = crate::safety::tests::app();
        let before = app.page().clone();
        app.st.snap(&before);
        app.st.snap(&before);
        assert_eq!(app.st.undo.len(), 1);
        let snap = app.page().clone();
        app.page().strokes.push(Stroke { id: 1, points: vec![[0.0, 0.0, 1.0]], color: [0, 0, 0, 255], width: 2.0, highlighter: false, pen: PenKind::Ballpoint, t: 0.0 });
        app.st.snap(&snap);
        let pid = app.page().id;
        assert!(app.attach_recording(pid, Recording { id: 9, file: "9.wav".into(), start: 0.0, secs: 1.0 }));
        assert!(app.undo(false), "takes back the stroke");
        assert_eq!(app.page().strokes.len(), before.strokes.len());
        assert!(app.page().recordings.iter().any(|r| r.id == 9), "the recording stays");
        assert!(!app.undo(false), "the focus step changed nothing, so there's nothing left");
        assert!(app.undo(true) && app.page().strokes.len() == before.strokes.len() + 1);
    }
}
