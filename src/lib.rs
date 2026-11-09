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
pub(crate) use i18n::tr;

/// Frames the window has drawn in a `--screenshot` run (none means the system isn't giving it any).
pub static SHOT_FRAMES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Android: something is pending, so the wake-up thread repaints the app regularly.
#[cfg(target_os = "android")]
static WAKE_FAST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
mod keys;

use eframe::egui::{self, Color32, CornerRadius, Key, Modifiers, Pos2, Stroke as EStroke, TextureHandle, Vec2};
use model::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

#[cfg(target_os = "android")]
pub static ANDROID_DATA: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

const SETTINGS_FILE: &str = "ai-settings.json";
const PREFS_FILE: &str = "prefs.json";

fn c3(c: [u8; 3]) -> Color32 { Color32::from_rgb(c[0], c[1], c[2]) }
fn c4(c: [u8; 4]) -> Color32 { Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]) }
/// The person's accent colour (teal unless changed in Settings).
static ACCENT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x0F766E);
fn accent_base() -> Color32 { let v = ACCENT.load(std::sync::atomic::Ordering::Relaxed); Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8) }
/// Accent that reads well on the current background.
/// All tooltips go through `tip`: one without text would show as an empty bubble, so it's skipped.
pub(crate) trait Tip { fn tip(self, text: impl Into<egui::WidgetText>) -> Self; }
impl Tip for egui::Response {
    fn tip(self, text: impl Into<egui::WidgetText>) -> Self {
        let text = text.into();
        let translated = tr(text.text()).to_string();
        if translated.trim().is_empty() { self } else if translated != text.text() { self.on_hover_text(translated) } else { self.on_hover_text(text) }
    }
}

fn accent(ui: &egui::Ui) -> Color32 { if ui.visuals().dark_mode { mix(accent_base(), Color32::WHITE, 0.45) } else { accent_base() } }
fn mix(a: Color32, b: Color32, t: f32) -> Color32 { a.lerp_to_gamma(b, t) }

pub const ACCENTS: [(&str, [u8; 3]); 6] = [("Teal", [15, 118, 110]), ("Indigo", [67, 56, 202]), ("Coral", [214, 82, 62]), ("Forest", [21, 128, 61]), ("Plum", [147, 51, 134]), ("Slate", [51, 65, 85])];

#[derive(Clone, Copy, PartialEq, Debug)]
enum ZoomReq { By(f32), To(f32), FitWidth }

/// Which kind of input started the current gesture.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Input { Mouse, Finger, Pen }

#[derive(Clone, Copy, PartialEq)]
enum Tab { Home, Insert, Draw, Ai, Study, View }

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tool { Type, Pen, Highlighter, Eraser, Lasso, Shape(Shape) }

/// A formatting command from the ribbon, applied by the text note it targets.
#[derive(Clone, Copy, PartialEq)]
enum Fmt { Wrap(&'static str), Prefix(&'static str), Heading(u8) }

#[derive(Clone, Copy, PartialEq)]
enum Rewrite { Replace, Append }

enum Job {
    Chat,
    Ink { math: bool, page: u64, strokes: Vec<u64>, at: [f32; 2], width: f32 },
    Rewrite { page: u64, block: u64, mode: Rewrite },
    Equation { page: u64 },
    Test,
    Cards { title: String },
    Printout { page: u64 },
    /// A finished background task that only reports a message.
    Note,
}

/// How a chosen file goes onto the page.
#[derive(Clone, Copy, PartialEq)]
enum Ins { Picture, Printout, Attach }

/// Settings that follow the person between sessions.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub(crate) struct Prefs {
    dark: bool,
    zoom: f32,
    show_nav: bool,
    show_ai: bool,
    ribbon_open: bool,
    /// The person's pens and which one is chosen.
    pens: Vec<Pen>,
    pen: usize,
    /// 0 = raw ink, 1 = strongly smoothed lines.
    stabilizer: f32,
    /// How pen pressure maps to thickness (1 = as measured, below 1 bolder light touches).
    pressure_curve: f32,
    /// Playback speed of recordings (kept between sessions).
    speed: f32,
    /// Opened locked sections close again after this many idle minutes (0: only when the app closes).
    lock_after_min: u32,
    /// A release the person chose not to be told about again.
    skip_version: String,
    /// The page last open in each section (section id → page id), to come back to it.
    last_page: HashMap<u64, u64>,
    /// Colours picked by hand lately (Pen settings shows them next to the presets).
    recent_colors: Vec<[u8; 4]>,
    hl_color: [u8; 4],
    erase_partial: bool,
    /// Eraser size (radius, page points) and highlighter thickness.
    eraser_size: f32,
    hl_width: f32,
    /// Read handwriting with the person's AI instead of the built-in offline recognizer.
    ink_ai: bool,
    last: (usize, usize, usize),
    // --- Settings window ---
    follow_system: bool,
    accent: [u8; 3],
    note_size: f32,
    line: f32,
    serif: bool,
    new_paper: Paper,
    /// A stylus always draws; mouse and finger only when switched on.
    draw_mouse: bool,
    draw_finger: bool,
    pressure: bool,
    straighten_hl: bool,
    /// Scribbling back and forth over ink erases it.
    scratch_out: bool,
    click_to_type: bool,
    auto_lists: bool,
    note_width: f32,
    favorites: Vec<u64>,
    recent: Vec<u64>,
    /// The person agreed to let the app use the microphone.
    mic_ok: bool,
    /// This device has shown a touch without pressure, so pressure means a stylus here (some phones report it for fingers too).
    pen_detect: bool,
    /// Interface language: "" follows the system, else "en", "de" or "fr".
    lang: String,
    /// The version whose "What's new" was shown (the tour shows on a new install instead).
    seen_version: String,
    /// Full-strength text and stronger borders.
    high_contrast: bool,
    /// Look for a newer version once per start (asks GitHub).
    check_updates: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { dark: false, zoom: 1.0, show_nav: true, show_ai: false, ribbon_open: true, pens: default_pens(), pen: 0, stabilizer: 0.35, pressure_curve: 1.0, recent_colors: vec![], last_page: HashMap::new(), speed: 1.0, lock_after_min: 10, skip_version: String::new(),
            hl_color: [253, 224, 71, 120], erase_partial: true, eraser_size: 8.0, hl_width: 16.0, ink_ai: false, last: (0, 0, 0),
            follow_system: false, accent: [15, 118, 110], note_size: 15.0, line: 1.55, serif: true, new_paper: Paper::Plain,
            draw_mouse: false, draw_finger: false, pressure: true, straighten_hl: true, scratch_out: true, click_to_type: true, auto_lists: true, note_width: 480.0, favorites: vec![], recent: vec![], mic_ok: false, pen_detect: false, lang: String::new(), seen_version: String::new(), high_contrast: false, check_updates: true }
    }
}

/// Everything except the notebooks themselves, so page and UI state can be borrowed together.
struct St {
    ai: ai::AiSettings,
    p: Prefs,
    tab: Tab,
    tool: Tool,
    current: Option<Stroke>,
    anchor: Option<[f32; 2]>,
    lasso: Vec<Pos2>,
    selected: Vec<u64>,
    moving: bool,
    force: Option<f32>,
    frame_force: Option<f32>,
    editing: Option<u64>,
    /// Size of the page area last frame (to notice the on-screen keyboard taking room).
    canvas_avail: Vec2,
    /// Typing a note with the on-screen keyboard up: the toolbar and page header step aside.
    typing_kb: bool,
    /// The screen is short (a phone on its side): the toolbar starts folded away.
    short: bool,
    /// Where a press on a selection's buttons began (touch forgets it by the time the finger lifts).
    sel_press: Option<Pos2>,
    /// Fingers on the screen now (where each started), and the current touch gesture: when it began,
    /// most fingers at once, whether any moved (a quick two-finger tap undoes, three fingers redo).
    fingers: HashMap<u64, Pos2>,
    /// (began, most fingers, any moved, when the first finger lifted)
    gesture: (f64, usize, bool, f64),
    /// Each page's zoom.
    scales: HashMap<u64, f32>,
    /// The tool to go back to after erasing with the stylus's eraser end or side button.
    tool_before_erase: Option<Tool>,
    restore_tool: bool,
    /// A narrow window or phone: side panels fold away after picking a page.
    narrow: bool,
    /// When the Android file picker was opened (the app keeps checking for its answer for a while).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    picking_since: f64,
    /// Last input was touch: bigger buttons and spacing.
    touch_ui: bool,
    /// Time of the previous frame (for the save timer).
    prev_t: f64,
    /// Since when nothing wants the keyboard (it closes after a short pause, never in a flicker).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    kb_idle_since: Option<f64>,
    last_text: Option<u64>,
    focus_id: Option<egui::Id>,
    fmt: Option<Fmt>,
    input: Input,
    /// The all-tasks list is showing in the page column.
    tasks: bool,
    /// Flashcards due again, counted once per frame.
    due_now: usize,
    /// Focus timer: when the current period ends, and whether it's a break.
    focus: Option<(f64, bool)>,
    /// A pen tool is selected but this input (mouse/finger) doesn't draw: it works as usual instead.
    ink_pass: bool,
    mouse_hint: bool,
    settings_tab: u8,
    /// Visible part of each page's canvas (zoom and pan), in page coordinates.
    views: HashMap<u64, egui::Rect>,
    pan: Vec2,
    zoom_req: Option<ZoomReq>,
    zoom_now: f32,
    /// Where (and since when) the pen has been holding still — a hold snaps the shape.
    still: (Pos2, f64),
    snapped: bool,
    /// Resizing (0) or rotating (1) the selected ink, around this point.
    xform: Option<(u8, Pos2)>,
    /// The "/" menu or "[[" page list open in a text note.
    popup: Option<canvas::Popup>,
    open_link: Option<String>,
    page_titles: Vec<String>,
    /// Pages linking to `backlinks_for`, open tasks and when these were last gathered (see `ui`).
    backlinks: Vec<(String, (usize, usize, usize))>,
    backlinks_for: u64,
    open_tasks_n: usize,
    derived_at: f64,
    /// The last search (query, when, results) and Ctrl+K list (query, results): recomputed when the query changes, or after a second.
    search_cache: Option<(String, f64, Vec<chrome::SearchHit>)>,
    switch_cache: Option<(String, Vec<(f32, String, String, (usize, usize, usize))>)>,
    insert_below: Option<(u64, Kind)>,
    /// Quick switcher (Ctrl+K): query and highlighted row.
    switcher: Option<(String, usize)>,
    study: Option<chrome::Study>,
    /// A whole-page PDF export in progress (photographed tile by tile).
    capture: Option<canvas::Capture>,
    /// Recording in progress: recorder, page it belongs to, recording id, start time.
    recorder: Option<(audio::Recorder, u64, u64, f64)>,
    rec_toggle: bool,
    player: Option<audio::Player>,
    /// "Tap notes to jump": clicking a note or ink plays the recording from that moment.
    listen: bool,
    /// Playback speed of recordings (1.0 = normal).
    speed: f32,
    /// The "may we use the microphone?" question is showing; or we're waiting for the system's answer (since when).
    mic_prompt: bool,
    mic_wait: Option<f64>,
    jump: Option<f64>,
    recordings_span: Option<(f64, f64)>,
    fade_after: Option<(f64, f64, f64)>,
    want_snap: bool,
    /// Take an undo step from `press_page` (the page when the pointer went down) at the end of this frame.
    snap_press: bool,
    press_page: Option<Page>,
    /// A backup file picked to restore, waiting for "add" or "replace".
    restore_ask: Option<PathBuf>,
    /// A recording waiting for "yes, send it" before it goes to the AI service for a transcript.
    transcribe_ask: Option<u64>,
    /// When the person last did something (opened locked sections close after a while without).
    last_input: f64,
    /// When something was last moved to Recently deleted (an Undo button shows for a few seconds).
    undo_delete: Option<f64>,
    /// Scroll the page list to the open page (after moving to another page).
    reveal: bool,
    /// A PDF dropped on the window, waiting for "print it" or "attach it".
    drop_pdf: Option<PathBuf>,
    /// A tap on a printed PDF page: start a note there (screen position).
    note_at: Option<Pos2>,
    heights: HashMap<u64, f32>,
    textures: HashMap<String, Option<TextureHandle>>,
    /// Pictures decoded in this frame (frame time, how many).
    decodes: (f64, u8),
    undo: Vec<(u64, Page)>,
    redo: Vec<(u64, Page)>,
    dirty: bool,
    since_save: f32,
    search: String,
    tag_filter: Option<Tag>,
    show_settings: bool,
    show_trash: bool,
    show_shortcuts: bool,
    rename: Option<(u64, String)>,
    commit_rename: bool,
    path_prompt: Option<(Ins, String)>, // for platforms without a file dialog
    ai_prompt: String,
    eq_prompt: String,
    ai_log: Vec<(bool, String)>, // (from_ai, text)
    ai_rx: Vec<(Job, Receiver<Result<String, String>>)>,
    test_result: Option<Result<String, String>>,
    status: String,
    status_age: f32,
    /// Writes changed pages only (see storage.rs).
    saver: storage::Saver,
    /// Text read from handwriting and PDFs, for search.
    index: index::Index,
    /// A transcript being made: (page, when the recording started, where the text arrives).
    transcribe: Option<(u64, f64, Receiver<Result<Vec<(f32, String)>, String>>)>,
    transcribe_req: Option<u64>,
    /// A newer release: (version, its page), and the check still running.
    update: Option<(String, String, String)>,
    update_rx: Option<Receiver<Option<(String, String, String)>>>,
    /// The update check was asked for by hand (so "up to date" is said too).
    update_manual: bool,
    /// A report from an earlier crash, waiting for "send or delete".
    crash: Option<PathBuf>,
    /// Page history window: (page, its older versions, the one selected).
    history: Option<(u64, Vec<(u64, Page)>, usize)>,
    /// Comparing a page with the other device's version: (this device's, the copy).
    compare: Option<(u64, u64)>,
    /// Keys of locked sections opened in this session (memory only).
    keys: HashMap<u64, vault::Key>,
    /// The sealed form of each opened locked section, and the pages it was made from (hash).
    seal_cache: HashMap<u64, (u64, Lock)>,
    lock_prompt: Option<locks::LockPrompt>,
    unlock_pw: String,
    unlock_err: String,
    /// The welcome tour's current step.
    tour: Option<u8>,
    /// Compare every page at the next save (after changes elsewhere than the page in front), and when that last happened.
    full_save: bool,
    full_at: f64,
    /// Saves so far, and at the start of the running sync round (nothing new saved: nothing to merge).
    saves: u64,
    sync_saves: u64,
    /// Held while this window owns the data folder (see `instance_lock`).
    #[allow(dead_code)]
    instance: Option<std::fs::File>,
    /// The tour is the first-start one (only then may it offer to replace the sample notes).
    tour_fresh: bool,
    show_news: bool,
    show_help: bool,
    /// Android's status and navigation bars (top, bottom, left, right) in points, and when they were last asked for.
    bars: [f32; 4],
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    bars_at: f64,
    sync: sync::SyncSettings,
    /// A running sync: the notes as they were sent, and where its result arrives.
    sync_rx: Option<(Store, Receiver<Result<sync::Outcome, String>>)>,
    /// When the last sync started (or may next retry), wall-clock seconds.
    sync_at: f64,
    sync_ok: u64,
    sync_err: String,
    sync_pending: bool,
    sync_now: bool,
    /// First sync with a place that already has notes: how many pages it has (asks how to join).
    sync_ask: Option<usize>,
    /// A Google sign-in waiting for the browser: the sign-in page, and where the result arrives.
    google_rx: Option<(String, Receiver<Result<(String, String), String>>)>,
    persist: bool,
}

impl St {
    /// Remember the page before a change so Undo can bring it back.
    fn snap(&mut self, page: &Page) {
        self.undo.push((page.id, page.clone()));
        if self.undo.len() > 80 { self.undo.remove(0); }
        self.redo.clear();
        self.dirty = true;
    }
    fn say(&mut self, msg: impl Into<String>) { let m = msg.into(); self.status = tr(&m).to_string(); self.status_age = 0.0; }
    /// The chosen pen (there is always at least one).
    fn pen(&mut self) -> &mut Pen {
        if self.p.pens.is_empty() { self.p.pens = default_pens(); }
        self.p.pen = self.p.pen.min(self.p.pens.len() - 1);
        &mut self.p.pens[self.p.pen]
    }
}

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
    // shortcut: relies on fonts the OS ships; bundle fonts if a platform turns up without any of these.
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
    // A serif for page titles and headings gives pages a calm, printed-notebook feel.
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

/// Card surfaces (page, page list, toolbar, assistant) float on a softly tinted desk.
/// A dialog's width, never wider than the screen (phones are about 400 points wide).
/// (72: room for the dialog's own frame and margins on both sides)
pub(crate) fn fit_w(ctx: &egui::Context, w: f32) -> f32 { w.min(ctx.content_rect().width() - 72.0).max(160.0) }

/// Copy a picture into the app, upright (phone photos keep their rotation in a separate tag) and at
/// most 2400 px on its long side (phone cameras take 12+ megapixels: slow to show and to sync).
/// Anything it can't decode is copied as it is. Returns the stored size in bytes.
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
        // JPEG has no transparency: flatten before saving
        Some(img) if matches!(dest.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(), Some("jpg" | "jpeg")) =>
            image::DynamicImage::ImageRgb8(img.to_rgb8()).save(dest).map_err(std::io::Error::other)?,
        Some(img) => img.save(dest).map_err(std::io::Error::other)?,
        None => { std::fs::copy(src, dest)?; }
    }
    std::fs::metadata(dest).map(|m| m.len())
}

/// Where an exported file can be found, for messages: the share menu's copy on Android, the file itself elsewhere.
fn where_saved(path: &std::path::Path) -> String {
    if cfg!(target_os = "android") { "Downloads/Leaf Notes".into() } else { path.display().to_string() }
}

/// A file was just exported: on Android it sits in the app's private folder, so open the share menu for it.
fn shared(path: &std::path::Path) {
    #[cfg(target_os = "android")]
    droid::share(path);
    let _ = path;
}

fn card(ui: &egui::Ui) -> egui::Frame {
    let v = ui.visuals();
    egui::Frame::new().fill(v.extreme_bg_color).corner_radius(14).stroke(v.widgets.noninteractive.bg_stroke)
        .shadow(egui::Shadow { offset: [0, 2], blur: 14, spread: 0, color: Color32::from_black_alpha(if v.dark_mode { 70 } else { 14 }) })
}

/// Push the person's look-and-feel preferences into the places that read them.
fn apply_prefs(ctx: &egui::Context, p: &Prefs) {
    i18n::set(&p.lang);
    use std::sync::atomic::Ordering::Relaxed;
    ACCENT.store(u32::from_be_bytes([0, p.accent[0], p.accent[1], p.accent[2]]), Relaxed);
    rich::LINE_X100.store((p.line.clamp(1.2, 2.0) * 100.0) as u32, Relaxed);
    rich::USE_SERIF.store(p.serif, Relaxed);
    HIGH_CONTRAST.store(p.high_contrast, Relaxed);
    apply_theme(ctx, p.dark);
}

fn apply_theme(ctx: &egui::Context, dark: bool) {
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
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(38, 48, 48);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(34, 40, 41);
        v.widgets.noninteractive.bg_stroke = EStroke::new(1.0, Color32::from_rgb(40, 46, 47));
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
    v.widgets.inactive.bg_stroke = EStroke::NONE;
    // text boxes get their own faint fill, so they read as fields on white cards
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
    // pin egui to this theme: otherwise a system dark/light switch flips egui to its other, unstyled
    // look (the app follows the system itself when asked to)
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals(v);
}

/// Lock the data folder for this window. Err: another window has it. Ok(None): locks don't work
/// here (an unusual file system), so go on without one.
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

/// The marker of a recording in progress (".part": never synced).
fn rec_marker(id: u64) -> PathBuf { data_dir().join("files").join(format!("{id}.rec.part")) }

static HIGH_CONTRAST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl NotesApp {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>) -> Self {
        setup_fonts(&cc.egui_ctx);
        let persist = screenshot.is_none();
        // one window at a time saves into the data folder: a second one would delete the first one's new pages
        let instance = if persist { instance_lock() } else { Ok(None) };
        // a library that can't be read is never saved over: the app runs without saving and says why
        let (store, load_msg, store_ok) = if !persist { (sample(), None, false) } else {
            match storage::load(&data_dir()) {
                Ok(storage::Loaded::Fresh) => (sample(), None, true),
                Ok(storage::Loaded::Store(s, damaged)) if damaged.is_empty() => (s, None, true),
                Ok(storage::Loaded::Store(s, damaged)) => (s, Some(tf!("⚠ {n} page(s) couldn't be read and are shown empty: {list}. Older versions are in ⋯ ▸ Page history.", n = damaged.len(), list = damaged.join(", "))), true),
                Err(e) => (sample(), Some(tf!("⚠ Your notes couldn't be read ({e}). Nothing will be saved until this is fixed, so your files stay untouched.", e = e)), false),
            }
        };
        let (load_msg, store_ok) = if instance.is_err() { (Some(tr("⚠ Leaf Notes is already open in another window. This window shows your notes but doesn't save them.").to_string()), false) } else { (load_msg, store_ok) };
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
            ai, p, tab: Tab::Home, tool: Tool::Type, current: None, anchor: None, lasso: vec![], selected: vec![], moving: false,
            force: None, frame_force: None, editing: None, canvas_avail: Vec2::ZERO, typing_kb: false, short: false, sel_press: None, fingers: HashMap::new(), gesture: (0.0, 0, false, 0.0), scales: HashMap::new(), tool_before_erase: None, restore_tool: false, narrow: false, touch_ui: false, picking_since: -1e9, prev_t: 0.0, kb_idle_since: None, last_text: None, focus_id: None, fmt: None, input: Input::Mouse, tasks: false, due_now: 0, focus: None, ink_pass: false, mouse_hint: false, settings_tab: 0, views: HashMap::new(), pan: Vec2::ZERO, zoom_req: None, zoom_now: 1.0, still: (Pos2::ZERO, 0.0), snapped: false, xform: None, popup: None, open_link: None, page_titles: vec![], backlinks: vec![], backlinks_for: 0, open_tasks_n: 0, derived_at: -10.0, search_cache: None, switch_cache: None, insert_below: None, switcher: None, study: None, capture: None, recorder: None, rec_toggle: false, player: None, listen: true, speed: 1.0, mic_prompt: false, mic_wait: None, jump: None, recordings_span: None, fade_after: None, want_snap: false, snap_press: false, press_page: None, note_at: None, drop_pdf: None, undo_delete: None, reveal: true, last_input: now_f(), transcribe_ask: None, restore_ask: None, heights: HashMap::new(),
            textures: HashMap::new(), decodes: (0.0, 0), undo: vec![], redo: vec![], dirty: false, since_save: 0.0, search: String::new(), tag_filter: None,
            show_settings: false, show_trash: false, show_shortcuts: false, rename: None, commit_rename: false, path_prompt: None,
            ai_prompt: String::new(), eq_prompt: String::new(), ai_log: vec![], ai_rx: vec![], test_result: None,
            status: String::new(), status_age: 0.0, persist: persist && store_ok, saver: Default::default(), index: index::Index::load(persist && store_ok), transcribe: None, transcribe_req: None, update: None, update_rx: None, update_manual: false, crash: None, history: None, compare: None, keys: HashMap::new(), seal_cache: HashMap::new(), lock_prompt: None, unlock_pw: String::new(), unlock_err: String::new(), tour: None, tour_fresh: false, show_news: false, show_help: false, bars: [0.0; 4], bars_at: -10.0,
            full_save: true, full_at: 0.0, saves: 0, sync_saves: 0, instance: instance.ok().flatten(), sync, sync_rx: None, sync_at: 0.0, sync_ok: 0, sync_err: String::new(), sync_pending: false, sync_now: false, sync_ask: None, google_rx: None,
        };
        let mut app = NotesApp { store, st, nb, sec, pg, screenshot, frames: 0, shot_requested: false, shot_done: Default::default(),
            #[cfg(target_os = "android")] android: None, #[cfg(target_os = "android")] keyboard_shown: false };
        app.clamp();
        app.st.dirty = app.st.persist; // a first save moves secrets from older settings files into the keyring
        if let Some(m) = load_msg { app.st.say(m); app.st.status_age = -50.0; } // stays up for a minute
        app.st.speed = app.st.p.speed.clamp(0.5, 2.0);
        if app.st.persist { app.recover_recordings(); app.greet(fresh_install); welcome::install_crash_hook(); app.st.crash = welcome::crash_reports().into_iter().next(); }
        if app.st.persist && app.st.p.check_updates { app.st.update_rx = Some(updates::check(welcome::VERSION, app.st.p.skip_version.clone())); }
        if !persist {
            // A representative moment for the preview picture: a formatted note targeted, the AI pane mid-conversation.
            app.st.last_text = app.page().blocks.get(1).map(|b| b.id);
            app.st.ai_log = vec![
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
        if nb.sections.is_empty() { nb.sections.push(Section { id: new_id(), name: "Quick Notes".into(), color: PALETTE[1], pages: vec![], lock: None }); }
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
        if self.st.dirty { self.st.full_save = true; } // the page being left may have unsaved changes
        if self.st.narrow { self.st.p.show_nav = false; } // phones: show the page that was picked
        (self.nb, self.sec, self.pg) = (nb, sec, pg);
        self.st.reveal = true;
        if let Some((sid, id)) = self.store.notebooks.get(nb).and_then(|n| n.sections.get(sec)).and_then(|s| s.pages.get(pg).map(|p| (s.id, p.id))) {
            self.st.p.last_page.insert(sid, id);
            self.st.p.recent.retain(|r| *r != id);
            self.st.p.recent.insert(0, id);
            self.st.p.recent.truncate(15);
        }
        self.st.popup = None;
        // the assistant's answers were about the page left behind: say so in the chat, and don't send them along as context
        if self.st.ai_log.last().is_some_and(|(_, t)| !t.starts_with("———")) {
            let title = self.store.notebooks.get(nb).and_then(|n| n.sections.get(sec)).and_then(|s| s.pages.get(pg)).map(|p| p.title.clone()).unwrap_or_default();
            self.st.ai_log.push((true, format!("——— {} ———", if title.trim().is_empty() { tr("Untitled page").to_string() } else { title })));
        }
        self.st.player = None; // playback controls live on the page, so stop when leaving it
        self.st.fade_after = None;
        self.st.selected.clear();
        self.st.editing = None;
        self.st.last_text = None;
        self.st.dirty = true; // remembers the open page
    }

    /// Where a page lives: (notebook, section, page) indexes.
    /// Open a section where it was left (its last open page), or at its first page.
    pub(crate) fn open_section(&mut self, nb: usize, sec: usize) {
        let pg = self.store.notebooks.get(nb).and_then(|n| n.sections.get(sec)).and_then(|s| { let id = self.st.p.last_page.get(&s.id)?; s.pages.iter().position(|p| p.id == *id) }).unwrap_or(0);
        self.go_to(nb, sec, pg);
    }

    fn locate(&self, id: u64) -> Option<(usize, usize, usize)> {
        self.store.notebooks.iter().enumerate().find_map(|(a, n)| n.sections.iter().enumerate().find_map(|(b, s)| s.pages.iter().position(|p| p.id == id).map(|c| (a, b, c))))
    }

    /// Open the page with this title (any notebook), or create it in the current section.
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
        // links to a renamed page follow it, rather than pointing at a page that's gone (and making a blank one when clicked)
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
            if n.id == id { n.name = name; return; }
            for s in &mut n.sections {
                if s.id == id { s.name = name; return; }
                for p in &mut s.pages { if p.id == id { p.title = name; return; } }
            }
        }
    }

    /// Move a notebook, section or page to Recently deleted.
    fn delete(&mut self, id: u64) {
        let now = now_secs();
        let mut binned = vec![];
        if let Some(i) = self.store.notebooks.iter().position(|n| n.id == id) {
            let mut n = self.store.notebooks.remove(i);
            for s in &mut n.sections { self.close_for_bin(s); }
            binned.push(Trashed { label: format!("Notebook “{}”", n.name), deleted: now, item: TrashItem::Notebook(n), at: Some(i) });
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
                binned.push(Trashed { label: format!("Section “{}” from {}", s.name, n.name), deleted: now, item: TrashItem::Section { notebook: n.id, section: s }, at: Some(i) });
            }
            Some((a, b, Some(i))) => {
                let p = self.store.notebooks[a].sections[b].pages.remove(i);
                let (n, s) = (&self.store.notebooks[a], &self.store.notebooks[a].sections[b]);
                let (nid, nname) = (n.id, n.name.clone());
                if s.lock.is_none() {
                    binned.push(Trashed { label: format!("Page “{}” from {} › {}", p.title, nname, s.name), deleted: now, item: TrashItem::Page { notebook: nid, section: s.id, page: p }, at: Some(i) });
                } else {
                    // a page of an opened locked section waits in the bin encrypted, as a one-page locked section (same password)
                    let from = s.clone_meta();
                    match self.sealed_page_for_bin(&from, p) {
                        Ok(section) => binned.push(Trashed { label: format!("A page from {} › {} (locked)", nname, from.name), deleted: now, item: TrashItem::Section { notebook: nid, section }, at: None }),
                        Err(p) => { self.store.notebooks[a].sections[b].pages.insert(i, p); self.st.say("Couldn't delete this page."); return; }
                    }
                }
            }
            None => {}
        }
        if !binned.is_empty() { self.st.undo_delete = Some(now_f()); }
        self.store.trash.extend(binned);
        self.st.dirty = true;
        self.st.selected.clear();
        self.st.say("Moved to Recently deleted — restore it from the notebook menu.");
        self.clamp();
    }

    fn restore(&mut self, i: usize) {
        let t = self.store.trash.remove(i);
        let (nb, sec) = (self.nb, self.sec);
        // back where it was when that place still exists, else at the end
        fn put<T>(v: &mut Vec<T>, at: Option<usize>, x: T) { let i = at.unwrap_or(usize::MAX).min(v.len()); v.insert(i, x); }
        match t.item {
            TrashItem::Notebook(n) => put(&mut self.store.notebooks, t.at, n),
            TrashItem::Section { notebook, section } => {
                match self.store.notebooks.iter_mut().find(|n| n.id == notebook) { Some(n) => put(&mut n.sections, t.at, section), None => self.store.notebooks[nb].sections.push(section) }
            }
            TrashItem::Page { notebook, section, page } => {
                // never next to a closed locked section's placeholder: saving would drop it
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
        sync::dedupe(&mut self.store); // a page deleted here but kept on another device can come back twice
        self.st.dirty = true;
        self.st.say(tf!("Restored {label}", label = t.label));
    }

    /// Delete things in Recently deleted for good, with the earlier versions kept of their pages.
    pub(crate) fn purge(&mut self, gone: Vec<Trashed>) {
        if self.st.persist {
            let pages = gone.iter().flat_map(|t| match &t.item {
                TrashItem::Page { page, .. } => vec![page.id],
                TrashItem::Section { section, .. } => section.pages.iter().map(|p| p.id).collect(),
                TrashItem::Notebook(n) => n.sections.iter().flat_map(|s| &s.pages).map(|p| p.id).collect(),
            });
            for id in pages { let _ = std::fs::remove_file(data_dir().join("history").join(format!("{id}.jsonl"))); }
        }
        self.st.dirty = true;
    }

    fn duplicate_page(&mut self, nb: usize, sec: usize, pg: usize) {
        let mut p = self.store.notebooks[nb].sections[sec].pages[pg].clone();
        p.id = new_id();
        p.title = format!("{} (copy)", p.title);
        for b in &mut p.blocks { b.id = new_id(); }
        for s in &mut p.strokes { s.id = new_id(); }
        for c in &mut p.cards { c.id = new_id(); } // the copy's cards get their own study progress
        self.store.notebooks[nb].sections[sec].pages.insert(pg + 1, p);
        self.go_to(nb, sec, pg + 1);
    }

    fn save_now(&mut self) {
        self.st.saves += 1;
        self.st.dirty = false;
        self.st.since_save = 0.0;
        if !self.st.persist { return; }
        self.st.p.last = (self.nb, self.sec, self.pg);
        // secrets go to the keyring; the files keep them only where there is none
        let mut ai = self.st.ai.clone();
        ai.api_key = keys::store("ai-key", &ai.api_key);
        let mut sy = self.st.sync.clone();
        sy.password = keys::store("webdav-password", &sy.password);
        sy.google_token = keys::store("google-token", &sy.google_token);
        // locked sections are written sealed only
        let sealed = self.sealed();
        // during a whole-page PDF export the panels are hidden for the picture: saved as they really are
        let mut prefs = self.st.p.clone();
        if let Some(c) = &self.st.capture { (prefs.show_nav, prefs.show_ai, prefs.ribbon_open) = c.keep; }
        // a full comparison of every page now and then (and when asked), else the quick one (see `Saver::save_with`)
        let full = std::mem::take(&mut self.st.full_save) || now_f() - self.st.full_at > 20.0;
        if full { self.st.full_at = now_f(); }
        let front = (!full).then(|| self.page().id);
        let r = self.st.saver.save_with(&data_dir(), sealed.as_ref().unwrap_or(&self.store), front).and_then(|_| save(SETTINGS_FILE, &ai)).and_then(|_| save(PREFS_FILE, &prefs)).and_then(|_| save(sync::SYNC_FILE, &sy));
        self.st.sync_pending = true;
        if let Err(e) = r { self.st.say(tf!("⚠ Could not save: {e}", e = e)); self.st.dirty = true; }
    }

    fn undo(&mut self, redo: bool) {
        // only the page in front: undoing changes on a page you can't see would look like nothing happened
        let here = self.page().id;
        let stack = if redo { &mut self.st.redo } else { &mut self.st.undo };
        let popped = stack.iter().rposition(|(id, _)| *id == here).map(|i| stack.remove(i));
        let Some((id, old)) = popped else { self.st.say(if redo { "Nothing to redo." } else { "Nothing to undo." }); return };
        let Some(p) = self.find_page(id) else { return };
        let cur = std::mem::replace(p, old);
        if redo { self.st.undo.push((id, cur)) } else { self.st.redo.push((id, cur)) }
        self.st.dirty = true;
        self.st.selected.clear();
    }

    // ---------- AI ----------
    fn start_job(&mut self, job: Job, system: &str, prompt: String, png: Option<Vec<u8>>) {
        if !self.st.ai.ready() {
            self.st.show_settings = true;
            self.st.settings_tab = 3;
            self.st.say("Add your AI key first — it stays on this device.");
            return;
        }
        let (tx, rx) = channel();
        let (s, system) = (self.st.ai.clone(), system.to_string());
        let optional_picture = matches!(job, Job::Chat); // the chat's handwriting picture is extra: some models take no pictures
        std::thread::spawn(move || {
            let mut r = ai::ask(&s, &system, &prompt, png.as_deref());
            if r.is_err() && png.is_some() && optional_picture { r = ai::ask(&s, &system, &prompt, None); }
            let _ = tx.send(r);
        });
        self.st.ai_rx.push((job, rx));
    }

    fn chat(&mut self, prompt: String) {
        let mut page = self.page().plain_text();
        // a long page or printout: the start of it, said so (a huge request is slow, may be refused, and costs)
        const MAX: usize = 60_000;
        if page.len() > MAX { let cut = page.char_indices().nth(MAX).map_or(page.len(), |(i, _)| i); page.truncate(cut); page.push_str("\n[… the rest of the page was left out]"); }
        // the page's handwriting goes along as a picture, so questions about pen notes can be answered
        let strokes: Vec<&Stroke> = self.page().strokes.iter().filter(|s| !s.highlighter).collect();
        let png = if strokes.is_empty() { None } else { rasterize(&strokes) };
        // the conversation about this page only (since the last "——— page ———" divider)
        let since = self.st.ai_log.iter().rposition(|(_, t)| t.starts_with("———")).map_or(0, |i| i + 1);
        let history: String = self.st.ai_log[since..].iter().rev().take(6).rev().map(|(ai, t)| format!("{}: {t}\n", if *ai { "Assistant" } else { "User" })).collect();
        let full = format!("{}--- Current note page ---\n{page}\n--- Request ---\n{prompt}", if history.is_empty() { String::new() } else { format!("--- Conversation so far ---\n{history}") });
        self.st.ai_log.push((false, prompt));
        self.st.p.show_ai = true;
        let picture = if png.is_some() { " The attached picture shows the page's handwritten notes." } else { "" };
        self.start_job(Job::Chat, &format!("You are a helpful assistant inside a note-taking app. Reply in concise Markdown. Use $...$ for inline math and put display math on its own line as $$...$$. Write action items as '- [ ] item'.{picture}{}", crate::i18n::reply_hint()), full, png);
    }

    fn rewrite(&mut self, mode: Rewrite, instruction: &str) {
        let target = self.st.last_text;
        let page = self.page();
        let Some(b) = target.and_then(|id| page.blocks.iter().find(|b| b.id == id)) else { self.st.say("Click into a text note first."); return; };
        let Kind::Text { text, .. } = &b.kind else { return };
        let (pid, bid, text) = (page.id, b.id, text.clone());
        self.st.say("AI is writing…");
        self.start_job(Job::Rewrite { page: pid, block: bid, mode }, "You edit notes. Keep the author's Markdown formatting and $math$. Reply with only the resulting text.", format!("{instruction}\n\n---\n{text}", instruction = instruction, text = text), None);
    }

    fn equation_from_words(&mut self) {
        let desc = std::mem::take(&mut self.st.eq_prompt);
        if desc.trim().is_empty() { self.st.say("Describe the equation first, e.g. “quadratic formula”."); return; }
        let pid = self.page().id;
        self.st.say("Writing the equation…");
        self.start_job(Job::Equation { page: pid }, "You write LaTeX math.", format!("Write the LaTeX for: {desc}\nReply with only the LaTeX expression, no $ signs, no code fences.", desc = desc), None);
    }

    fn ink_convert(&mut self, math: bool) {
        let sel = self.st.selected.clone();
        // all of a page's ink at once would turn drawings into text too: pick what to convert
        if sel.is_empty() && self.page().strokes.iter().any(|s| !s.highlighter) {
            self.st.tool = Tool::Lasso;
            self.st.say("Circle the handwriting to convert with the lasso, then press the button again.");
            return;
        }
        let use_ai = self.st.p.ink_ai && self.st.ai.ready();
        let page = self.page();
        let strokes: Vec<&Stroke> = page.strokes.iter().filter(|s| !s.highlighter && (sel.is_empty() || sel.contains(&s.id))).collect();
        let Some(png) = rasterize(&strokes) else { self.st.say("Write something with the pen first (or lasso the ink to convert)."); return; };
        let ids = strokes.iter().map(|s| s.id).collect();
        let at = strokes.iter().flat_map(|s| s.points.iter()).fold([f32::MAX, f32::MAX], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
        // the typed version gets about the same width as the handwriting it replaces
        let width = (strokes.iter().flat_map(|s| s.points.iter()).fold(f32::MIN, |m, p| m.max(p[0])) - at[0] + 40.0).clamp(220.0, 900.0);
        let pid = page.id;
        if !use_ai {
            // Built-in recognizer: runs on this device, no key or internet needed.
            let owned: Vec<Stroke> = strokes.iter().map(|s| (*s).clone()).collect();
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let t = std::panic::catch_unwind(|| if math { hw::recognize_math(&owned) } else { hw::recognize_text(&owned) }).unwrap_or_default();
                let _ = tx.send(if t.trim().is_empty() { Err(tr("Couldn't read that handwriting — try writing a little larger and more separated.").into()) } else { Ok(t) });
            });
            self.st.ai_rx.push((Job::Ink { math, page: pid, strokes: ids, at, width }, rx));
            self.st.say(if math { "Reading your handwriting as math (offline)…" } else { "Reading your handwriting (offline)…" });
            return;
        }
        let (system, prompt) = if math {
            ("You convert handwritten math to LaTeX.", "Convert the handwritten math in this image to LaTeX. Reply with only the LaTeX expression, no $ signs, no code fences, no commentary.")
        } else {
            ("You transcribe handwriting.", "Transcribe the handwriting in this image. Reply with only the transcribed text, keeping line breaks. Use Markdown lists if the writing is a list. No commentary.")
        };
        self.st.say(if math { "Reading your handwriting as math…" } else { "Reading your handwriting…" });
        self.start_job(Job::Ink { math, page: pid, strokes: ids, at, width }, system, prompt.into(), Some(png));
    }

    fn test_ai(&mut self) {
        self.st.test_result = None;
        self.start_job(Job::Test, "Reply with the single word OK.", "Say OK".into(), None);
    }

    fn poll_ai(&mut self, ctx: &egui::Context) {
        let mut done = vec![];
        self.st.ai_rx.retain_mut(|(job, rx)| match rx.try_recv() {
            Ok(r) => { done.push((std::mem::replace(job, Job::Chat), r)); false }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            // the job's thread ended without an answer (it crashed): say so instead of waiting forever
            Err(_) => { done.push((std::mem::replace(job, Job::Chat), Err(tr("That stopped unexpectedly — please try again.").into()))); false }
        });
        if !self.st.ai_rx.is_empty() { ctx.request_repaint_after(std::time::Duration::from_millis(150)); }
        for (job, r) in done {
            match (job, r) {
                (Job::Test, r) => self.st.test_result = Some(r),
                (Job::Note, Ok(m)) => self.st.say(m),
                (Job::Cards { title }, Ok(t)) => {
                    // keep only well-formed "question :: answer" lines, each becomes a card in a new deck
                    let cards: Vec<(String, String)> = t.lines().filter_map(|l| l.trim().trim_start_matches(['-', '*', ' ']).split_once("::").map(|(q, a)| (q.trim().to_string(), a.trim().to_string())))
                        .filter(|(q, a)| !q.is_empty() && !a.is_empty()).collect();
                    if cards.is_empty() { self.st.say("⚠ The AI didn't return any flashcards — try again."); continue; }
                    let n = cards.len();
                    self.new_deck(&format!("{title} — cards"), cards);
                    self.st.say(tf!("Made a deck of {n} cards — edit anything you like, then press Study.", n = n));
                }
                (Job::Chat, Err(e)) => { self.st.ai_log.push((true, format!("⚠ {e}"))); self.st.say(e); }
                (_, Err(e)) => self.st.say(format!("⚠ {e}")),
                (Job::Chat, Ok(t)) => self.st.ai_log.push((true, t)),
                (Job::Ink { math, page, strokes, at, width }, Ok(t)) => {
                    let t = ai::clean(&t);
                    // an empty answer must never cost the handwriting
                    if t.trim().is_empty() { self.st.say("⚠ Nothing could be read there, so the handwriting stays as it is."); continue; }
                    let Some(p) = self.find_page(page) else { continue };
                    let snap = p.clone();
                    p.strokes.retain(|s| !strokes.contains(&s.id));
                    let kind = if math { Kind::Math { src: t } } else { Kind::Text { text: t, heading: 0 } };
                    p.blocks.push(block(at[0].max(0.0), at[1].max(0.0), width, kind));
                    self.st.snap(&snap);
                    self.st.selected.clear();
                    self.st.say("Ink converted. Undo brings the handwriting back.");
                }
                (Job::Rewrite { page, block: bid, mode }, Ok(t)) => {
                    let Some(p) = self.find_page(page) else { continue };
                    let snap = p.clone();
                    if let Some(Kind::Text { text, .. }) = p.blocks.iter_mut().find(|b| b.id == bid).map(|b| &mut b.kind) {
                        match mode { Rewrite::Replace => *text = t.trim().to_string(), Rewrite::Append => { text.push_str("\n\n"); text.push_str(t.trim()); } }
                    }
                    self.st.snap(&snap);
                    self.st.say("Done. Undo restores the previous text.");
                }
                (Job::Equation { page }, Ok(t)) => {
                    if self.page().id == page { self.insert(Kind::Math { src: ai::clean(&t) }); self.st.editing = None; self.st.say("Equation added."); }
                    else { self.st.say("The equation is ready, but you moved to another page — ask again there."); }
                }
                (Job::Printout { page }, Ok(names)) => {
                    // stack the printed pages below everything else, at a readable width
                    let Some(p) = self.find_page(page) else { continue };
                    let snap = p.clone();
                    self.st.snap(&snap);
                    let mut extra = String::new();
                    for name in names.lines() {
                        if let Some(n) = name.strip_prefix('…') { extra = format!(" (first 60 of {n} pages)"); continue; }
                        let (w, h) = image::image_dimensions(files_path(name)).unwrap_or((800, 1000));
                        let width = 640.0;
                        let y = { let heights = std::mem::take(&mut self.st.heights); let y = self.find_page(page).map_or(8.0, |p| p.blocks.iter().map(|b| b.pos[1] + heights.get(&b.id).copied().unwrap_or(40.0) + 18.0).fold(8.0, f32::max)); self.st.heights = heights; y };
                        let b = block(24.0, y, width, Kind::Image { file: name.to_string() });
                        self.st.heights.insert(b.id, width * h as f32 / w.max(1) as f32);
                        if let Some(p) = self.find_page(page) { p.blocks.push(b); }
                    }
                    self.st.say(tf!("Printout added{extra} — pick a pen to write on it.", extra = extra));
                }
            }
        }
    }

    // ---------- inserting ----------
    fn next_free_y(&mut self) -> f32 {
        let heights = std::mem::take(&mut self.st.heights);
        let y = self.page().blocks.iter().map(|b| b.pos[1] + heights.get(&b.id).copied().unwrap_or(40.0) + 18.0).fold(8.0, f32::max);
        self.st.heights = heights;
        y
    }

    fn insert(&mut self, kind: Kind) {
        if self.page().kind == PageKind::Locked { return; }
        let y = self.next_free_y();
        let w = match kind { Kind::Table { .. } | Kind::Checklist { .. } | Kind::Link { .. } | Kind::File { .. } => 380.0, _ => self.st.p.note_width };
        // never wider than the paper on screen (phones)
        let pid = self.page().id;
        let w = self.st.views.get(&pid).map_or(w, |v| w.min(v.right() - 24.0 - 12.0).max(160.0));
        let b = block(24.0, y, w, kind);
        let id = b.id;
        let page = self.page();
        let snap = page.clone();
        page.blocks.push(b);
        self.st.snap(&snap);
        self.st.focus_id = Some(egui::Id::unique(("ed", id)));
        self.st.editing = Some(id);
        self.st.tool = Tool::Type;
        // bring the new note into view if the page is scrolled or zoomed elsewhere
        if let Some(v) = self.st.views.get_mut(&pid) {
            if y < v.top() || y + 60.0 > v.bottom() || v.left() > 24.0 { *v = v.translate(Vec2::new(-v.left(), (y - 80.0).max(-40.0) - v.top())); }
        }
    }

    /// Insert an AI reply as proper notes (text, math, checklists, tables, code).
    fn insert_markdown(&mut self, md: &str) {
        let snap = self.page().clone();
        self.st.snap(&snap);
        for k in blocks_from_markdown(md) {
            let y = self.next_free_y();
            let lines = k.summary().len() as f32 / 60.0 + md.lines().count().min(12) as f32;
            let w = if matches!(k, Kind::Checklist { .. } | Kind::Table { .. }) { 380.0 } else { self.st.p.note_width };
            let b = block(24.0, y, w, k);
            self.st.heights.insert(b.id, 24.0 * lines.max(1.0)); // estimate until drawn, so blocks don't overlap
            self.page().blocks.push(b);
        }
        self.st.tool = Tool::Type;
        self.st.say("Inserted into the page.");
    }

    /// Put a file on the page: pictures as pictures; for printouts, PDF pages as pictures you can
    /// write on (plus the original attached) and Markdown/text as notes; anything else attached.
    fn insert_file(&mut self, path: &std::path::Path, how: Ins) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        // stored as "<id>.<ext>": an extension that isn't short and plain would keep the file from syncing
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
        let copied = std::fs::create_dir_all(&dir).and_then(|_| if is_image { store_picture(path, &dir.join(&stored)) } else { std::fs::copy(path, dir.join(&stored)) });
        match copied {
            Err(e) => self.st.say(tf!("⚠ Could not add {path}: {e}", path = path.display(), e = e)),
            Ok(size) => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.insert(if is_image { Kind::Image { file: stored.clone() } } else { Kind::File { file: stored.clone(), name, size } });
                self.st.editing = None;
                if how == Ins::Printout && ext == "pdf" { self.start_printout(dir.join(&stored)); }
            }
        }
    }

    /// Render a PDF's pages to pictures on a worker thread (pure Rust, so it works offline everywhere).
    fn start_printout(&mut self, pdf: PathBuf) {
        const MAX_PAGES: usize = 60;
        let pid = self.page().id;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let r = print_pdf(&pdf, MAX_PAGES);
            let _ = tx.send(r);
        });
        self.st.ai_rx.push((Job::Printout { page: pid }, rx));
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
            let (kind, mime) = match how { Ins::Picture => (1, "image/*"), Ins::Printout => (2, "application/pdf,image/*,text/*"), Ins::Attach => (3, "*/*") };
            if let Err(e) = droid::pick(kind, mime, false) { self.st.say(tf!("⚠ {e}", e = e)); }
            self.st.picking_since = now_f();
        }
    }

    /// Android: files from the picker or shared from other apps.
    #[cfg(target_os = "android")]
    fn take_incoming(&mut self) {
        // a cheap question first: fetching the list every frame costs a string across JNI each time
        if !droid::has_incoming() { return; }
        for (kind, path) in droid::incoming() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            match kind {
                1 => self.insert_file(&path, Ins::Picture),
                2 => self.insert_file(&path, Ins::Printout),
                3 => self.insert_file(&path, Ins::Attach),
                4 => self.restore_backup(&path),
                5 => self.import_markdown(vec![path.clone()]),
                9 => if let Ok(t) = std::fs::read_to_string(&path) { self.insert_markdown(&t); },
                // shared from another app: by kind of file
                _ => self.insert_file(&path, match ext.as_str() { "pdf" => Ins::Printout, "png" | "jpg" | "jpeg" | "webp" => Ins::Picture, "md" | "txt" => Ins::Printout, _ => Ins::Attach }),
            }
            // shared from another app: say where it went (it lands on the page that was open)
            if matches!(kind, 0 | 9) {
                let title = self.page().title.clone();
                self.st.say(tf!("Added to “{page}”.", page = if title.trim().is_empty() { tr("Untitled page").to_string() } else { title }));
            }
            let _ = std::fs::remove_file(&path); // the app keeps its own copy
            self.st.picking_since = -1e9;
        }
    }

    /// A new empty page on the chosen background (or the one from Settings).
    fn new_page(&mut self, paper: Option<Paper>) -> bool {
        if !self.section_open_for_writing() { return false; }
        let mut p = Page { paper: paper.unwrap_or(self.st.p.new_paper), ..Page::new("") };
        // right after the page you're on (a subpage next to subpages), not at the end of a long list
        let pg = self.pg;
        let sec = self.section();
        let at = if pg < sec.pages.len() { p.level = sec.pages[pg].level; pg + 1 } else { sec.pages.len() };
        sec.pages.insert(at, p);
        let (nb, s) = (self.nb, self.sec);
        self.go_to(nb, s, at);
        self.st.focus_id = Some(egui::Id::unique("page-title"));
        self.st.tool = Tool::Type;
        true
    }

    fn export_page(&mut self) {
        let page = self.page();
        let md = page.plain_text();
        let name = format!("{}.md", if page.title.trim().is_empty() { "Untitled page" } else { page.title.as_str() }.replace(['/', '\\', ':', '?', '*', '"', '<', '>', '|'], "-"));
        #[cfg(not(target_os = "android"))]
        if let Some(p) = rfd::FileDialog::new().set_file_name(&name).add_filter("Markdown", &["md"]).save_file() {
            match storage::write_atomic(&p, md.as_bytes()) { Ok(_) => self.st.say(tf!("Exported to {p}", p = p.display())), Err(e) => self.st.say(tf!("⚠ Export failed: {e}", e = e)) }
        }
        #[cfg(target_os = "android")]
        {
            let dir = data_dir().join("exports");
            let r = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(&name), md));
            match r { Ok(_) => droid::share(&dir.join(&name)), Err(e) => self.st.say(tf!("⚠ Export failed: {e}", e = e)) }
        }
    }

    /// Where to save an exported file: ask on desktop; on Android a private folder, then the share menu (see `shared`).
    fn ask_save_path(&mut self, name: &str, ext: &str) -> Option<PathBuf> {
        #[cfg(not(target_os = "android"))]
        { rfd::FileDialog::new().set_file_name(name).add_filter(ext.to_uppercase(), &[ext]).save_file() }
        #[cfg(target_os = "android")]
        { let dir = data_dir().join("exports"); let _ = std::fs::create_dir_all(&dir); let _ = ext; Some(dir.join(name)) }
    }

    /// PDF export: the printout with notes on top when there is one (unless `whole`), otherwise
    /// a picture of the written-on part of the page.
    fn export_pdf(&mut self, whole: bool) {
        let heights = self.st.heights.clone();
        let hf = move |id: u64| heights.get(&id).copied();
        let page = self.page().clone();
        let title = if page.title.trim().is_empty() { "Untitled page".to_string() } else { page.title.clone() };
        let file = format!("{}.pdf", title.replace(['/', '\\', ':', '?', '*', '"', '<', '>', '|'], "-"));
        if !whole && export::printout_source(&page, &hf).is_some() {
            let Some(out) = self.ask_save_path(&file, "pdf") else { return };
            match export::export_printout(&page, &data_dir().join("files"), &hf, self.st.p.note_size, &out) {
                Ok(n) => {
                    shared(&out);
                    let lost = export::unprintable(&page);
                    let note = if lost > 0 { tf!(" ⚠ {k} character(s) (Greek, maths symbols…) can't be printed this way and show as ?. “Export whole page as PDF” keeps them.", k = lost) } else { String::new() };
                    self.st.say(tf!("Exported the printout with your notes on {n} page(s) to {out}", n = n, out = where_saved(&out)) + &note)
                }
                Err(e) => self.st.say(format!("⚠ {e}")),
            }
            return;
        }
        // the area that has something on it
        let mut r: Option<egui::Rect> = None;
        for b in &page.blocks { let br = egui::Rect::from_min_size(Pos2::new(b.pos[0], b.pos[1]), Vec2::new(b.width, hf(b.id).unwrap_or(40.0) + 12.0)); r = Some(r.map_or(br, |r| r.union(br))); }
        for s in &page.strokes { for p in &s.points { let pr = egui::Rect::from_center_size(Pos2::new(p[0], p[1]), Vec2::splat(s.width + 4.0)); r = Some(r.map_or(pr, |r| r.union(pr))); } }
        let Some(content) = r else { self.st.say("This page is empty — nothing to export."); return };
        let Some(path) = self.ask_save_path(&file, "pdf") else { return };
        let keep = (self.st.p.show_nav, self.st.p.show_ai, self.st.p.ribbon_open);
        (self.st.p.show_nav, self.st.p.show_ai, self.st.p.ribbon_open) = (false, false, false);
        self.st.editing = None;
        self.st.selected.clear();
        self.st.capture = Some(canvas::Capture { page: page.id, content: content.expand(16.0), k: 0, tiles: vec![], wait: 0, rx: None, path, title, keep, view: self.st.views.get(&page.id).copied(), scale: self.st.scales.get(&page.id).copied() });
        self.st.say("Exporting the page as PDF…");
    }

    /// A new flashcard deck in this section (optionally pre-filled), opened and ready to type in.
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
        // a fresh deck starts at its title; a suggested one at its first card
        self.st.focus_id = Some(if cards.is_empty() { egui::Id::unique("page-title") } else { egui::Id::unique(("cardf", first)) });
    }

    /// Offline: suggest a deck from definitions, Q/A lines, highlights, headings and tables on this page.
    fn deck_from_notes(&mut self) {
        let page = self.page();
        let title = if page.title.trim().is_empty() { "Untitled page".to_string() } else { page.title.clone() };
        let cards: Vec<(String, String)> = flashcards(page).into_iter().map(|c| (c.front, c.back)).collect();
        if cards.is_empty() {
            self.st.say("No card-like notes found here (questions, “term :: meaning”, highlights, headings or tables). Try “With AI”, or start a new deck.");
            return;
        }
        let n = cards.len();
        self.new_deck(&format!("{title} — cards"), cards);
        self.st.say(tf!("Suggested {n} cards — edit or delete any you don't want, then press Study.", n = n));
    }

    /// Ask the AI for flashcards written as "question :: answer" lines; they become a new deck.
    fn ai_cards(&mut self) {
        let page = self.page();
        let title = if page.title.trim().is_empty() { "Untitled page".to_string() } else { page.title.clone() };
        let text = page.plain_text();
        if text.split_whitespace().count() < 8 { self.st.say("Write a few notes on this page first."); return; }
        self.st.say("Writing flashcards…");
        self.start_job(Job::Cards { title }, &format!("You write short, clear study flashcards.{}", crate::i18n::reply_hint()),
            format!("Write 6 to 12 flashcards from these notes. One per line, exactly in the form: question :: answer\nKeep answers short. Use $...$ for maths. No numbering, no other text.\n\n{text}"), None);
    }

    /// A dated, empty lecture page, optionally recording straight away.
    fn start_lecture(&mut self, record: bool) {
        if !self.new_page(None) { return; }
        let d = chrome::format_date(now_secs(), false);
        let short = d.rsplit_once(',').map_or(d.clone(), |(a, _)| a.to_string()); // drop the year
        self.page().title = format!("Lecture · {short}");
        self.st.focus_id = None;
        if record && self.st.recorder.is_none() { self.st.rec_toggle = true; }
        self.st.say(if record { "New lecture page — recording starts once the microphone is allowed." } else { "New lecture page ready." });
    }

    /// Sync in the background; whatever is written meanwhile is merged with the result when it arrives.
    pub(crate) fn start_sync(&mut self, first: Option<sync::First>) {
        if self.st.sync_rx.is_some() || !self.st.persist || !self.st.sync.on() { return; }
        let sent = self.sealed().unwrap_or_else(|| self.store.clone());
        self.st.sync_saves = self.st.saves;
        let (cfg, local, home) = (self.st.sync.clone(), sent.clone(), data_dir());
        let (tx, rx) = channel();
        std::thread::spawn(move || { let _ = tx.send(sync::run(&cfg, &home, &local, first, false)); });
        self.st.sync_rx = Some((sent, rx));
        self.st.sync_at = now_f();
        self.st.sync_pending = false;
        self.st.sync_now = false;
    }

    fn poll_sync(&mut self) {
        let Some((_, rx)) = &self.st.sync_rx else { return };
        let r = match rx.try_recv() { Ok(r) => r, Err(std::sync::mpsc::TryRecvError::Empty) => return, Err(_) => Err(tr("Sync stopped unexpectedly — trying again shortly.").into()) };
        let Some((sent, _)) = self.st.sync_rx.take() else { return };
        match r {
            Ok(sync::Outcome::AskFirst { pages }) => self.st.sync_ask = Some(pages),
            Ok(sync::Outcome::Synced { store, down, base, problem, changed, .. }) => {
                // compare and merge in the sealed form; opened locked sections are opened again after.
                // Nothing saved or edited since the round began: the notes are still what was sent, so the
                // costly whole-library comparisons are skipped (the round already knows whether it changed them)
                let touched = self.st.dirty || self.st.saves != self.st.sync_saves;
                let current = touched.then(|| self.sealed().unwrap_or_else(|| self.store.clone()));
                let meanwhile = current.as_ref().is_some_and(|c| !sync::same(&sent, c));
                let merged = match &current { Some(c) if meanwhile => sync::merge(&sent, c, &store), _ => Ok(store) };
                match merged {
                    Ok(m) => {
                        let differs = match &current { Some(c) if meanwhile => !sync::same(&m, c), _ => changed };
                        if differs {
                            // stay on the open page wherever it is now
                            let open = self.store.notebooks.get(self.nb).and_then(|n| n.sections.get(self.sec)).and_then(|s| s.pages.get(self.pg)).map(|p| p.id);
                            self.store = self.reopen(m);
                            self.st.full_save = true; // pages other than this one changed
                            // undo steps are whole pages from before the merge: they would undo the other device's changes
                            self.st.undo.clear();
                            self.st.redo.clear();
                            match open.and_then(|id| self.locate(id)) { Some((a, b, c)) => (self.nb, self.sec, self.pg) = (a, b, c), None => self.clamp() }
                            self.save_now();
                            self.st.say(if down > 0 { tf!("Synced changes from your other devices ({down} file(s)).", down = down) } else { "Synced changes from your other devices.".into() });
                        }
                        self.st.sync_pending = meanwhile;
                        self.st.sync_ok = now_secs();
                        self.st.sync_err.clear();
                        // the agreed state only once these notes are on this device's disk
                        if self.st.dirty { self.save_now(); self.st.sync_pending = meanwhile; }
                        if !self.st.dirty && self.st.persist { if let Err(e) = base.commit(&data_dir()) { self.st.sync_err = e; } }
                        if let Some(p) = problem { self.st.say(format!("⚠ {p}")); }
                    }
                    Err(e) => { self.st.sync_err = e; self.st.sync_pending = true; }
                }
            }
            Err(e) => {
                self.st.sync_err = e;
                self.st.sync_pending = true;
                self.st.sync_at = now_f() + 40.0; // offline or misconfigured: retry in about a minute
            }
        }
    }

    /// Stop a whole-page PDF export and put the hidden panels back.
    fn cancel_capture(&mut self, msg: &str) {
        if let Some(c) = self.st.capture.take() {
            (self.st.p.show_nav, self.st.p.show_ai, self.st.p.ribbon_open) = c.keep;
            if let Some(v) = c.view { self.st.views.insert(c.page, v); }
            match c.scale { Some(s) => { self.st.scales.insert(c.page, s); } None => { self.st.scales.remove(&c.page); } }
            if !msg.is_empty() { self.st.say(msg); }
        }
    }

    /// Start recording on this page, or stop and keep the recording.
    fn toggle_record(&mut self) {
        if let Some((rec, pid, id, start)) = self.st.recorder.take() {
            let file = rec.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let r = rec.stop();
            let _ = std::fs::remove_file(rec_marker(id));
            #[cfg(target_os = "android")]
            droid::recording(false);
            match r {
                Ok(secs) => {
                    let rec = Recording { id, file, start, secs };
                    match self.find_page(pid) {
                        Some(p) => p.recordings.push(rec),
                        None => { self.page().recordings.push(rec); self.st.say("The page you recorded on was deleted, so the recording was kept on this page."); return; }
                    }
                    self.st.dirty = true;
                    self.st.say("Recording saved — press ▶ under the title, or tap a note to hear that moment.");
                }
                Err(e) => self.st.say(format!("⚠ {e}")),
            }
            return;
        }
        // ask first: once in the app, and (on Android) the system's own permission dialog
        if !self.st.p.mic_ok { self.st.mic_prompt = true; return; }
        if !audio::mic_granted() {
            match audio::mic_request() {
                Ok(()) => { self.st.mic_wait = Some(now_f()); self.st.say("Waiting for microphone permission…"); }
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
                // if the app stops before the recording does, the next start finds it through this
                let _ = storage::write_atomic(&rec_marker(id), serde_json::json!({ "page": pid, "start": start }).to_string().as_bytes());
                self.st.recorder = Some((r, pid, id, start));
                #[cfg(target_os = "android")]
                droid::recording(true);
                self.st.say("Recording — everything you write now is linked to this moment.");
            }
            Err(e) => self.st.say(format!("⚠ {e}")),
        }
    }

    /// Recordings that were still going when the app last stopped: finished and put back on their page.
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
                    let rec = Recording { id, file: format!("{id}.wav"), start, secs };
                    match self.find_page(pid) { Some(p) => p.recordings.push(rec), None => self.page().recordings.push(rec) }
                    found += 1;
                }
            }
            let _ = std::fs::remove_file(e.path());
        }
        if found > 0 { self.st.dirty = true; self.st.say(tf!("Recovered {n} unfinished recording(s).", n = found)); }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let cmd = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, k));
        if !typing {
            // While typing, the text box handles its own undo.
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)) || cmd(Key::Y) { self.undo(true); }
            if cmd(Key::Z) { self.undo(false); }
            let key = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
            if key(Key::T) { self.st.tool = Tool::Type; }
            if key(Key::P) { self.st.tool = Tool::Pen; }
            if key(Key::H) { self.st.tool = Tool::Highlighter; }
            if key(Key::E) { self.st.tool = Tool::Eraser; }
            if key(Key::L) { self.st.tool = Tool::Lasso; }
            if !self.st.selected.is_empty() && (key(Key::Delete) || key(Key::Backspace)) { self.delete_selected_ink(); }
            // ← / → skip 10 seconds in the recording that's playing
            if let Some(pl) = &self.st.player {
                if key(Key::ArrowLeft) { pl.seek(pl.time() - 10.0); }
                if key(Key::ArrowRight) { pl.seek(pl.time() + 10.0); }
            }
            // 1–9: My pens; S: the next shape (not while studying, where 1–3 rate cards)
            if self.st.study.is_none() {
                for (i, k) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9].into_iter().enumerate() {
                    if i < self.st.p.pens.len() && key(k) { self.st.p.pen = i; self.st.tool = Tool::Pen; }
                }
            }
            if key(Key::S) {
                self.st.tool = Tool::Shape(match self.st.tool { Tool::Shape(Shape::Line) => Shape::Arrow, Tool::Shape(Shape::Arrow) => Shape::Rect, Tool::Shape(Shape::Rect) => Shape::Ellipse, _ => Shape::Line });
            }
            // the page's own zoom (Ctrl +/− sizes the whole window's text instead)
            let alt = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::ALT, k));
            if alt(Key::Equals) || alt(Key::Plus) { self.st.zoom_req = Some(ZoomReq::By(1.25)); }
            if alt(Key::Minus) { self.st.zoom_req = Some(ZoomReq::By(1.0 / 1.25)); }
            if alt(Key::Num0) { self.st.zoom_req = Some(ZoomReq::To(1.0)); }
            if alt(Key::Num9) { self.st.zoom_req = Some(ZoomReq::FitWidth); }
        }
        if cmd(Key::N) { self.new_page(None); }
        // moving around and opening things without the mouse
        let (nb, sec, pg) = (self.nb, self.sec, self.pg);
        let pages = self.store.notebooks[nb].sections[sec].pages.len();
        if cmd(Key::PageDown) && pg + 1 < pages { self.go_to(nb, sec, pg + 1); }
        if cmd(Key::PageUp) && pg > 0 { self.go_to(nb, sec, pg - 1); }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F2)) { self.st.focus_id = Some(egui::Id::unique("page-title")); }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F1)) { self.st.show_shortcuts = true; }
        if !typing && cmd(Key::D) { self.duplicate_page(nb, sec, pg); }
        if !typing && cmd(Key::B) { self.st.p.show_nav ^= true; }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::T)) { self.st.tasks ^= true; }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::R)) { self.start_review(); }
        if cmd(Key::Comma) { self.st.show_settings = true; }
        if cmd(Key::F) { if self.st.narrow { self.st.p.show_nav = true; } self.st.focus_id = Some(egui::Id::unique("search")); }
        if cmd(Key::K) || cmd(Key::P) { self.st.switcher = Some((String::new(), 0)); }
        // Android's Back: closes whatever is on top, one thing per press; with nothing open, leaves the app
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::BrowserBack)) { self.back(ctx); return; }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            if self.st.popup.take().is_some() || self.st.switcher.take().is_some() { return; }
            if self.st.capture.is_some() { self.cancel_capture("PDF export cancelled."); return; }
            // Esc is taken here every frame, so the windows that say "Close (Esc)" are closed here too
            let st = &mut self.st;
            if egui::Popup::is_any_open(ctx) { egui::Popup::close_all(ctx); return; }
            if st.tour.take().is_some() { st.tour_fresh = false; return; }
            if st.study.take().is_some() || st.history.take().is_some() || st.compare.take().is_some() || st.lock_prompt.take().is_some()
                || std::mem::take(&mut st.show_settings) || std::mem::take(&mut st.show_help) || std::mem::take(&mut st.show_news)
                || std::mem::take(&mut st.show_trash) || std::mem::take(&mut st.show_shortcuts) || std::mem::take(&mut st.mic_prompt) { return; }
            self.st.selected.clear();
            self.st.editing = None;
            self.st.search.clear();
            self.st.tag_filter = None;
            self.st.current = None;
        }
    }

    fn back(&mut self, ctx: &egui::Context) {
        let st = &mut self.st;
        if egui::Popup::is_any_open(ctx) { egui::Popup::close_all(ctx); }
        else if st.popup.take().is_some() || st.switcher.take().is_some() {}
        else if st.tour.take().is_some() { st.tour_fresh = false; }
        else if st.capture.is_some() { self.cancel_capture("PDF export cancelled."); }
        else if st.study.take().is_some() || st.history.take().is_some() || st.compare.take().is_some() {}
        else if std::mem::take(&mut st.show_settings) || std::mem::take(&mut st.show_help) || std::mem::take(&mut st.show_news) || std::mem::take(&mut st.show_trash) || std::mem::take(&mut st.show_shortcuts) {}
        else if st.editing.take().is_some() || !std::mem::take(&mut st.selected).is_empty() {}
        else if st.narrow && st.p.show_ai { st.p.show_ai = false; }
        else if !st.search.is_empty() || st.tag_filter.is_some() { st.search.clear(); st.tag_filter = None; }
        else if st.narrow && st.p.show_nav { st.p.show_nav = false; }
        else {
            #[cfg(target_os = "android")]
            droid::to_background();
        }
    }

    fn delete_selected_ink(&mut self) {
        let sel = std::mem::take(&mut self.st.selected);
        let page = self.page();
        let snap = page.clone();
        page.strokes.retain(|s| !sel.contains(&s.id));
        self.st.snap(&snap);
    }
}

impl eframe::App for NotesApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Pen pressure arrives as touch force on platforms that report it.
        self.st.frame_force = None;
        // the eraser end / side button erased one stroke: back to the tool from before (a frame later,
        // so the stroke finishes as an eraser stroke)
        if std::mem::take(&mut self.st.restore_tool) { if let Some(t) = self.st.tool_before_erase.take() { self.st.tool = t; } }
        let mut tap = 0; // fingers in a quick tap that just ended
        ctx.input(|i| {
            let touched = i.events.iter().any(|e| matches!(e, egui::Event::Touch { .. }));
            for e in &i.events {
                match e {
                    // fingers (no pressure): also watched for the quick multi-finger taps
                    egui::Event::Touch { force: None, phase, id, pos, .. } => {
                        let (st, now) = (&mut self.st, i.time);
                        match phase {
                            egui::TouchPhase::Start => {
                                if st.fingers.is_empty() { st.gesture = (now, 0, false, f64::MAX); }
                                st.fingers.insert(id.0, *pos);
                                st.gesture.1 = st.gesture.1.max(st.fingers.len());
                            }
                            // a tap barely moves: a short pinch or scroll nudge isn't one
                            egui::TouchPhase::Move => if st.fingers.get(&id.0).is_some_and(|p| p.distance(*pos) > 10.0) { st.gesture.2 = true; },
                            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                                if *phase == egui::TouchPhase::Cancel { st.gesture.2 = true; }
                                st.gesture.3 = st.gesture.3.min(now);
                                // all fingers lift together in a tap (a palm and a finger lifting apart don't count)
                                if st.fingers.remove(&id.0).is_some() && st.fingers.is_empty() && !st.gesture.2 && now - st.gesture.0 < 0.35 && now - st.gesture.3 < 0.08 { tap = st.gesture.1; }
                            }
                        }
                        if *phase == egui::TouchPhase::Start { self.st.input = Input::Finger; if !self.st.p.pen_detect { self.st.p.pen_detect = true; self.st.dirty = true; } }
                    }
                    egui::Event::Touch { force, phase, .. } => {
                        // a stylus encodes its eraser end as pressure below 0 and a held side button as above 1
                        let (pressure, erase) = match *force { Some(f) if f < 0.0 => (Some(-f), true), Some(f) if f > 1.0 => (Some(f - 1.0), true), f => (f, false) };
                        if let Some(f) = pressure { self.st.force = Some(f); self.st.frame_force = Some(f); }
                        if *phase == egui::TouchPhase::Start && erase && self.st.tool_before_erase.is_none() {
                            self.st.tool_before_erase = Some(self.st.tool);
                            self.st.tool = Tool::Eraser;
                        }
                        if matches!(phase, egui::TouchPhase::End | egui::TouchPhase::Cancel) && self.st.tool_before_erase.is_some() { self.st.restore_tool = true; }
                        // a stylus reports pressure; most fingers on touch screens don't
                        if *phase == egui::TouchPhase::Start {
                            self.st.input = if force.is_some() { Input::Pen } else { Input::Finger };
                            if force.is_none() && !self.st.p.pen_detect { self.st.p.pen_detect = true; self.st.dirty = true; }
                        }
                    }
                    egui::Event::PointerButton { pressed: true, .. } if !touched => self.st.input = Input::Mouse,
                    // a lost window can swallow the fingers' "lifted": forget them, or taps would stop working
                    egui::Event::WindowFocused(false) => self.st.fingers.clear(),
                    _ => {}
                }
            }
        });
        if tap >= 2 {
            // the first finger may also have tapped the paper and started an empty note: drop it
            self.st.editing = None;
            self.st.current = None;
            self.undo(tap >= 3);
            self.st.say(if tap >= 3 { "Redone (three-finger tap)." } else { "Undone (two-finger tap). Tap with three fingers to redo." });
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
            // a big screenshot is stored at a sensible size (like picked pictures), so pages and sync stay quick
            let pic = image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, img.as_raw().to_vec()).map(image::DynamicImage::ImageRgba8)
                .map(|p| if p.width().max(p.height()) > 2400 { p.resize(2400, 2400, image::imageops::FilterType::Triangle) } else { p });
            let r = std::fs::create_dir_all(&dir).map_err(|e| e.to_string())
                .and_then(|_| pic.ok_or_else(|| "not a picture".to_string())?.save(dir.join(&file)).map_err(|e| e.to_string()));
            match r { Ok(()) => { self.insert(Kind::Image { file }); self.st.editing = None; self.st.say("Pasted the picture onto the page."); } Err(e) => self.st.say(tf!("⚠ Couldn't paste the picture: {e}", e = e)) }
        }
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            let p = f.path().to_path_buf();
            if p.as_os_str().is_empty() { continue; }
            // a dropped PDF could be meant to write on or just to keep: ask (printing makes a picture of every page)
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) { self.st.drop_pdf = Some(p); } else { self.insert_file(&p, Ins::Printout); }
        }
        // opened locked sections close again after a while without use (a laptop or phone left lying around)
        if ctx.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
            let idle = now_f() - self.st.last_input;
            let after = self.st.p.lock_after_min as f64 * 60.0;
            if after > 0.0 && idle > after && !self.st.keys.is_empty() {
                for id in self.st.keys.keys().copied().collect::<Vec<_>>() { self.relock(id); }
                self.st.say("Locked sections were closed again after a while without use.");
            }
            self.st.last_input = now_f();
        }
        self.handle_keys(&ctx);
        self.poll_ai(&ctx);
        self.clamp();

        // Android draws the app edge to edge: keep clear of the status and navigation bars
        #[cfg(target_os = "android")]
        {
            let t = ctx.input(|i| i.time);
            // (more often while typing: the bottom inset is how we learn the keyboard opened or closed)
            if t - self.st.bars_at > if self.st.editing.is_some() { 0.3 } else { 1.0 } {
                self.st.bars_at = t;
                // two sources, the larger wins: Java's window insets, and the content area Android gives native apps
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
            // keep asking until Android has told us (it can take a moment after start)
            if self.st.bars == [0.0; 4] && t < 15.0 { ctx.request_repaint_after(std::time::Duration::from_millis(500)); }
        }
        let [top, bottom, left, right] = self.st.bars;
        let blank = egui::Frame::new().fill(ui.visuals().panel_fill);
        if top > 0.0 { egui::Panel::top("bar-top").exact_size(top).resizable(false).show_separator_line(false).frame(blank).show(ui, |_| {}); }
        if bottom > 0.0 { egui::Panel::bottom("bar-bottom").exact_size(bottom).resizable(false).show_separator_line(false).frame(blank).show(ui, |_| {}); }
        if left > 0.0 { egui::Panel::left("bar-left").exact_size(left).resizable(false).show_separator_line(false).frame(blank).show(ui, |_| {}); }
        if right > 0.0 { egui::Panel::right("bar-right").exact_size(right).resizable(false).show_separator_line(false).frame(blank).show(ui, |_| {}); }
        // phones and narrow windows: the side panels fold away (☰ brings them back) and close again
        // after picking a page; touch gets larger targets
        let narrow = ctx.content_rect().width() < 1000.0;
        if narrow != self.st.narrow {
            self.st.narrow = narrow;
            self.st.p.show_nav = !narrow;
            if narrow { self.st.p.show_ai = false; }
        }
        let short = ctx.content_rect().height() < 520.0;
        if short != self.st.short {
            self.st.short = short;
            self.st.p.ribbon_open = !short; // the tabs stay: tapping one opens its toolbar
        }
        let touch = cfg!(target_os = "android") || matches!(self.st.input, Input::Finger | Input::Pen); // (Android: from the very first frame)
        if touch != self.st.touch_ui {
            self.st.touch_ui = touch;
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
        // the bottom inset includes the on-screen keyboard: only a keyboard is this tall (the Back key can close it without us knowing otherwise)
        { self.st.typing_kb = self.st.editing.is_some() && self.st.bars[1] > 150.0; }
        self.title_bar(ui);
        self.ribbon(ui);
        self.status_bar(ui);
        if self.st.p.show_nav { self.nav(ui); }
        if self.st.p.show_ai { self.ai_pane(ui); }
        self.clamp();
        let (nb, sec, pg) = (self.nb, self.sec, self.pg);
        let crumb = format!("{}  ·  {}", self.store.notebooks[nb].name, self.store.notebooks[nb].sections[sec].name);
        let mut page_action = None;
        // What's gathered from the whole library (cards due, open tasks, page titles for the "[[" list,
        // pages linking here): at most once a second, and at once on another page, not every frame.
        let here_id = self.store.notebooks[nb].sections[sec].pages[pg].id;
        if now_f() - self.st.derived_at > 1.0 || self.st.backlinks_for != here_id {
            self.st.derived_at = now_f();
            self.st.backlinks_for = here_id;
            self.st.due_now = self.due_cards().len();
            self.st.open_tasks_n = self.open_tasks().len();
            let here = self.store.notebooks[nb].sections[sec].pages[pg].title.trim().to_lowercase();
            let all = || self.store.notebooks.iter().flat_map(|n| n.sections.iter()).flat_map(|s| s.pages.iter());
            rich::set_known_titles(all().map(|p| p.title.trim().to_lowercase()).filter(|t| !t.is_empty()).collect());
            // the "[[" list: every other page (linking a page to itself is no use)
            self.st.page_titles = all().filter(|p| p.id != here_id).map(|p| p.title.clone()).filter(|t| !t.trim().is_empty()).collect();
            let needle = format!("[[{here}]]");
            let mut backlinks = vec![];
            if !here.is_empty() {
                for (a, n) in self.store.notebooks.iter().enumerate() { for (b, s) in n.sections.iter().enumerate() { for (c, p) in s.pages.iter().enumerate() {
                    if (a, b, c) != (nb, sec, pg) && p.blocks.iter().any(|bl| match &bl.kind {
                        Kind::Text { text, .. } => text.to_lowercase().contains(&needle),
                        Kind::Checklist { items } => items.iter().any(|(_, t)| t.to_lowercase().contains(&needle)),
                        _ => false,
                    }) { backlinks.push((p.title.clone(), (a, b, c))); }
                }}}
            }
            self.st.backlinks = backlinks;
        }
        let backlinks = self.st.backlinks.clone();
        let fav = self.st.p.favorites.contains(&self.store.notebooks[nb].sections[sec].pages[pg].id);
        let conflict = self.conflict_pair();
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            if conflict.is_some() && NotesApp::conflict_banner(ui) { self.st.compare = conflict; }
            let page = &mut self.store.notebooks[nb].sections[sec].pages[pg];
            page_action = canvas::page_view(ui, page, &mut self.st, &crumb, &backlinks, fav, &self.store.study);
        });
        if let Some(t) = self.st.open_link.take() { self.open_title(&t); }
        if let Some(id) = self.st.transcribe_req.take() { self.start_transcribe(id); }
        if self.st.capture.as_ref().is_some_and(|c| c.page != self.store.notebooks[nb].sections[sec].pages[pg].id) { self.cancel_capture("PDF export cancelled because the page changed."); }
        if std::mem::take(&mut self.st.rec_toggle) { self.toggle_record(); }
        if let Some(e) = self.st.recorder.as_ref().and_then(|(r, ..)| r.error.lock().ok().and_then(|mut g| g.take())) {
            self.st.say(tf!("⚠ The microphone reported a problem ({e}). The recording may have gaps.", e = e));
        }
        if let Some(since) = self.st.mic_wait {
            // the system dialog answers asynchronously: start as soon as it's allowed
            if audio::mic_granted() { self.st.mic_wait = None; self.toggle_record(); }
            else if now_f() - since > 60.0 { self.st.mic_wait = None; self.st.say("⚠ The microphone wasn't allowed. You can allow it for Leaf Notes in your phone's settings."); }
            else { ctx.request_repaint_after(std::time::Duration::from_millis(300)); }
        }
        if self.st.recorder.is_some() || self.st.player.as_ref().is_some_and(|p| p.playing()) { ctx.request_repaint_after(std::time::Duration::from_millis(120)); }
        // a finished whole-page capture: put the panels back and write the PDF in the background
        if self.st.capture.as_ref().is_some_and(|c| c.done()) {
            let c = self.st.capture.take().unwrap_or_else(|| unreachable!("checked above"));
            (self.st.p.show_nav, self.st.p.show_ai, self.st.p.ribbon_open) = c.keep;
            if let Some(v) = c.view { self.st.views.insert(c.page, v); }
            match c.scale { Some(s) => { self.st.scales.insert(c.page, s); } None => { self.st.scales.remove(&c.page); } }
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let w = c.tiles.iter().map(|t| t.width()).max().unwrap_or(1);
                let h: u32 = c.tiles.iter().map(|t| t.height()).sum();
                let mut all = image::RgbImage::from_pixel(w, h.max(1), image::Rgb([255, 255, 255]));
                let mut y = 0;
                for t in &c.tiles { image::imageops::overlay(&mut all, &image::DynamicImage::ImageRgba8(t.clone()).to_rgb8(), 0, y as i64); y += t.height(); }
                let r = export::write_image_pdf(&c.title, &all, &c.path).map(|n| { shared(&c.path); tf!("Exported {n} PDF page(s) to {out}", n = n, out = where_saved(&c.path)) });
                let _ = tx.send(r);
            });
            self.st.ai_rx.push((Job::Note, rx));
        }
        match page_action {
            Some(canvas::PageAction::Export) => self.export_page(),
            Some(canvas::PageAction::Copy) => { let md = self.page().plain_text(); ctx.copy_text(md); self.st.say("Page copied as Markdown."); }
            Some(canvas::PageAction::Delete) => { let id = self.page().id; self.delete(id); }
            Some(canvas::PageAction::Duplicate) => self.duplicate_page(nb, sec, pg),
            Some(canvas::PageAction::History) => self.show_history(),
            Some(canvas::PageAction::Unlock(pw)) => {
                let id = self.section().id;
                match self.unlock_section(id, &pw) { Ok(()) => { self.st.unlock_err.clear(); self.st.say("Section unlocked until you lock it again or close Leaf Notes."); } Err(e) => self.st.unlock_err = e }
            }
            Some(canvas::PageAction::Go(a, b, c)) => self.go_to(a, b, c),
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

        if self.st.commit_rename {
            self.st.commit_rename = false;
            if let Some((id, name)) = self.st.rename.take() { if !name.trim().is_empty() { self.rename(id, name.trim().to_string()); self.st.dirty = true; } }
        }

        // real seconds since the last frame (stable_dt assumes 60 fps, so an idle app, which draws
        // rarely, would wait close to a minute to save)
        let now_t = ctx.input(|i| i.time);
        let dt = ((now_t - self.st.prev_t).max(0.0) as f32).min(2.0);
        self.st.prev_t = now_t;
        self.st.since_save += dt;
        self.st.status_age += dt;
        if self.st.dirty && self.st.since_save > 1.5 { self.save_now(); }
        if self.st.dirty { ctx.request_repaint_after(std::time::Duration::from_millis(600)); }
        if self.st.persist && self.st.sync.on() {
            // every 3 minutes, 20 s after a change, or when asked
            let since = now_f() - self.st.sync_at;
            if (self.st.sync_now || since > 180.0 || (self.st.sync_pending && since > 20.0)) && self.st.sync_ask.is_none() && !self.st.dirty { self.start_sync(None); }
            ctx.request_repaint_after(std::time::Duration::from_secs(if self.st.sync_rx.is_some() { 1 } else { 20 }));
        }
        self.poll_sync();
        self.poll_transcribe();
        if let Some(rx) = &self.st.update_rx { if let Ok(r) = rx.try_recv() {
            if r.is_none() && std::mem::take(&mut self.st.update_manual) { self.st.say("Leaf Notes is up to date."); }
            self.st.update = r;
            self.st.update_rx = None;
        } }
        if self.st.transcribe.is_some() { ctx.request_repaint_after(std::time::Duration::from_millis(500)); }
        if let Some((_, rx)) = &self.st.google_rx {
            match rx.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(500)),
                r => {
                    self.st.google_rx = None;
                    match r {
                        Ok(Ok((token, email))) => {
                            (self.st.sync.via, self.st.sync.google_token, self.st.sync.google_email) = (sync::Via::Google, token, email);
                            self.st.sync_err.clear();
                            self.st.sync_now = true;
                            self.st.dirty = true;
                            self.st.say("Signed in to Google Drive — syncing your notes.");
                        }
                        Ok(Err(e)) => self.st.sync_err = e,
                        Err(_) => {}
                    }
                }
            }
        }

        #[cfg(target_os = "android")]
        if let Some(app) = &self.android {
            let wants = ctx.egui_wants_keyboard_input();
            // open at once; close only after nothing has wanted it for a moment, so a frame without
            // focus (the layout shifting as the keyboard appears) can't start an open/close loop
            let now = ctx.input(|i| i.time);
            if wants { self.st.kb_idle_since = None; } else if self.st.kb_idle_since.is_none() { self.st.kb_idle_since = Some(now); }
            let close = !wants && self.st.kb_idle_since.is_some_and(|t| now - t > 0.4);
            // a tap into the note again brings the keyboard back after the system's Back hid it (the app never hears that Back)
            let tapped = ctx.input(|i| i.pointer.primary_released());
            if wants && (!self.keyboard_shown || tapped) { app.show_soft_input(true); self.keyboard_shown = true; }
            else if close && self.keyboard_shown { app.hide_soft_input(true); self.keyboard_shown = false; }
            if !wants && self.keyboard_shown { ctx.request_repaint_after(std::time::Duration::from_millis(450)); }
            // Android doesn't wake an idle app for delayed repaints, so saving, timers and progress would
            // stall until the next touch. A small thread wakes it while something is pending.
            let busy = self.st.dirty || self.st.recorder.is_some() || self.st.player.is_some() || self.st.capture.is_some()
                || !self.st.ai_rx.is_empty() || self.st.sync_rx.is_some() || self.st.google_rx.is_some() || self.st.focus.is_some()
                || self.st.mic_wait.is_some() || self.st.bars == [0.0; 4] || (!wants && self.keyboard_shown) || self.st.index.busy() || self.st.transcribe.is_some() || now_f() - self.st.picking_since < 120.0;
            WAKE_FAST.store(busy, std::sync::atomic::Ordering::Relaxed);
            static TICKER: std::sync::Once = std::sync::Once::new();
            TICKER.call_once(|| {
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let mut idle = 0u32;
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(400));
                        idle += 1;
                        // fast while busy; every 30 s otherwise (sync checks, clocks)
                        // also when a file was picked or shared (Android delivers it to Java)
                        if WAKE_FAST.load(std::sync::atomic::Ordering::Relaxed) || idle >= 75 || droid::has_incoming() { idle = 0; ctx.request_repaint(); }
                    }
                });
            });
        }

        if let Some(path) = self.screenshot.clone() {
            self.frames += 1;
            SHOT_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ctx.request_repaint();
            // 120 frames, or 2.5 s if the system slows the window down (hidden or covered windows get few frames)
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
        if self.st.recorder.is_some() { self.toggle_record(); }
        if self.st.capture.is_some() { self.cancel_capture(""); } // put the panels back before saving settings
        let unsynced = self.st.sync_pending || self.st.dirty;
        // save first (everything compared): a slow or killed exit sync must never cost the last edits
        self.st.full_save = true;
        self.save_now();
        self.st.index.flush(self.st.persist);
        // send the last changes on the way out (short timeouts, so closing never hangs for long)
        if self.st.persist && self.st.sync.on() && self.st.sync_rx.is_none() && self.st.sync_ask.is_none() && unsynced {
            let sealed = self.sealed();
            if let Ok(sync::Outcome::Synced { store, base, .. }) = sync::run(&self.st.sync, &data_dir(), sealed.as_ref().unwrap_or(&self.store), None, true) {
                // the synced notes are sealed: open the opened locked sections again, or saving would seal their placeholder over them
                self.store = self.reopen(store);
                self.save_now();
                if !self.st.dirty { let _ = base.commit(&data_dir()); }
            }
        }
    }

    /// The desk colour shows between the cards.
    fn clear_color(&self, v: &egui::Visuals) -> [f32; 4] { v.panel_fill.to_normalized_gamma_f32() }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    if let Some(p) = app.internal_data_path() { let _ = ANDROID_DATA.set(p); }
    // before the app starts: loading the settings already asks the activity (key store, language)
    audio::set_activity(app.activity_as_ptr());
    let options = eframe::NativeOptions { android_app: Some(app.clone()), ..Default::default() };
    let _ = eframe::run_native("Leaf Notes", options, Box::new(move |cc| Ok(Box::new(NotesApp::new(cc, None).with_android(app)))));
}

/// Sizes of the system bars around the app (top, bottom, left, right) in pixels; the soft keyboard counts in the bottom one.
#[cfg(target_os = "android")]
fn system_bars() -> Option<[i32; 4]> {
    use jni::{objects::JObject, strings::JNIString, Env};
    fn ask(env: &mut Env<'_>, act: &JObject<'_>) -> jni::errors::Result<Option<[i32; 4]>> {
        let win = env.call_method(act, JNIString::new("getWindow"), jni::jni_sig!("()Landroid/view/Window;"), &[])?.l()?;
        let decor = env.call_method(&win, JNIString::new("getDecorView"), jni::jni_sig!("()Landroid/view/View;"), &[])?.l()?;
        let ins = env.call_method(&decor, JNIString::new("getRootWindowInsets"), jni::jni_sig!("()Landroid/view/WindowInsets;"), &[])?.l()?;
        if ins.is_null() { return Ok(None); } // not attached yet
        let mut v = [0; 4];
        for (i, m) in ["getSystemWindowInsetTop", "getSystemWindowInsetBottom", "getSystemWindowInsetLeft", "getSystemWindowInsetRight"].into_iter().enumerate() {
            v[i] = env.call_method(&ins, JNIString::new(m), jni::jni_sig!("()I"), &[])?.i()?;
        }
        Ok(Some(v))
    }
    audio::with_activity(|env, act| {
        let r = ask(env, &act);
        if r.is_err() { let _ = env.exception_clear(); } // a pending Java exception would break every later call
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
        assert_eq!(std::fs::read(dir.join("b.png")).unwrap(), std::fs::read(&small).unwrap()); // small ones are copied as they are
        let _ = std::fs::remove_dir_all(&dir);
    }
}
