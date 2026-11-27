//! Application UI and runtime state, decomposed into focused sub-structs
//! plus a unified `ModalStack` and separated AI / background job channels.

use crate::*;
use eframe::egui::{self, Pos2, TextureHandle, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// Streaming or final output from an AI job.
#[allow(dead_code)]
pub(crate) enum AiEvent {
    Delta(String),
    Done(Result<String, String>),
}

/// An in-flight AI task (chat, rewrite, equation, flashcards, test, or AI handwriting).
pub(crate) struct AiJob {
    pub kind: AiJobKind,
    pub rx: Receiver<AiEvent>,
    pub cancel: Arc<AtomicBool>,
    /// Index into `JobsState::ai_log` for streaming chat messages.
    pub chat_idx: Option<usize>,
}

pub(crate) enum AiJobKind {
    Chat,
    Ink { math: bool, page: u64, strokes: Vec<u64>, at: [f32; 2], width: f32 },
    Rewrite { page: u64, block: u64, mode: Rewrite },
    Equation { page: u64 },
    Test,
    Cards { title: String },
}

/// A non-AI background task (offline handwriting, PDF printout, PDF/Markdown export, etc.).
/// Completely separate from `AiJob` so clicking AI "Stop" never kills PDF printouts or exports.
pub(crate) struct BgJob {
    pub kind: BgJobKind,
    pub rx: Receiver<Result<String, String>>,
}

pub(crate) enum BgJobKind {
    Printout { page: u64 },
    OfflineInk { math: bool, page: u64, strokes: Vec<u64>, at: [f32; 2], width: f32 },
    Note,
}

/// Every blocking dialog / modal in the app lives on a single stack.
/// Opening a sub-dialog from inside Settings (e.g. Trash, Shortcuts, Restore)
/// pushes on top; Esc / Android Back pops one layer back to the previous modal.
pub(crate) enum ActiveModal {
    Settings { tab: u8 },
    SyncAsk { pages: usize },
    History { page: u64, versions: Vec<(u64, Page)>, sel: usize },
    Compare { orig: u64, copy: u64 },
    LockPrompt(locks::LockPrompt),
    Tour { step: u8, fresh: bool },
    News,
    Help,
    Crash { path: PathBuf, text: String },
    Switcher { query: String, sel: usize },
    Study(chrome::Study),
    MicPrompt,
    RestoreAsk { path: PathBuf, parsed: Option<Store> },
    InkReview(InkReview),
    TranscribeAsk { rec_id: u64 },
    DropPdf { path: PathBuf },
    Trash,
    Shortcuts,
    #[allow(dead_code)]
    PathPrompt { how: Ins, path: String },
}

impl ActiveModal {
    fn tag(&self) -> u8 {
        match self {
            Self::Settings { .. } => 0,
            Self::SyncAsk { .. } => 1,
            Self::History { .. } => 2,
            Self::Compare { .. } => 3,
            Self::LockPrompt(_) => 4,
            Self::Tour { .. } => 5,
            Self::News => 6,
            Self::Help => 7,
            Self::Crash { .. } => 8,
            Self::Switcher { .. } => 9,
            Self::Study(_) => 10,
            Self::MicPrompt => 11,
            Self::RestoreAsk { .. } => 12,
            Self::InkReview(_) => 13,
            Self::TranscribeAsk { .. } => 14,
            Self::DropPdf { .. } => 15,
            Self::Trash => 16,
            Self::Shortcuts => 17,
            Self::PathPrompt { .. } => 18,
        }
    }

    /// True for dialogs that require an explicit answer and must not be dismissed by Esc/Back.
    pub(crate) fn blocks_escape(&self) -> bool {
        matches!(self, Self::SyncAsk { .. })
    }
}

#[derive(Default)]
pub(crate) struct ModalStack {
    stack: Vec<ActiveModal>,
}

impl ModalStack {
    pub(crate) fn is_empty(&self) -> bool { self.stack.is_empty() }

    #[allow(dead_code)]
    pub(crate) fn top(&self) -> Option<&ActiveModal> { self.stack.last() }

    pub(crate) fn pop(&mut self) -> Option<ActiveModal> { self.stack.pop() }

    /// Push a modal on top; if the same modal kind is already in the stack, replace/bring it to top.
    pub(crate) fn push(&mut self, modal: ActiveModal) {
        let t = modal.tag();
        self.stack.retain(|m| m.tag() != t);
        self.stack.push(modal);
    }

    /// Toggle a simple modal: if it's currently on top, pop it; otherwise push it.
    pub(crate) fn toggle(&mut self, modal: ActiveModal) {
        if self.stack.last().is_some_and(|m| m.tag() == modal.tag()) {
            self.stack.pop();
        } else {
            self.push(modal);
        }
    }

    pub(crate) fn open_settings(&mut self, tab: u8) {
        if let Some(ActiveModal::Settings { tab: cur }) = self.stack.last_mut() {
            *cur = tab;
        } else {
            self.push(ActiveModal::Settings { tab });
        }
    }

    pub(crate) fn toggle_settings(&mut self, tab: u8) {
        if matches!(self.stack.last(), Some(ActiveModal::Settings { .. })) {
            self.stack.pop();
        } else {
            self.open_settings(tab);
        }
    }

    pub(crate) fn has_settings(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, ActiveModal::Settings { .. }))
    }

    pub(crate) fn has_sync_ask(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, ActiveModal::SyncAsk { .. }))
    }

    #[allow(dead_code)]
    pub(crate) fn has_tour(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, ActiveModal::Tour { .. }))
    }

    #[allow(dead_code)]
    pub(crate) fn has_help(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, ActiveModal::Help))
    }

    pub(crate) fn has_trash(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, ActiveModal::Trash))
    }

    pub(crate) fn has_shortcuts(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, ActiveModal::Shortcuts))
    }

    pub(crate) fn has_switcher(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, ActiveModal::Switcher { .. }))
    }

    #[allow(dead_code)]
    pub(crate) fn has_study(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, ActiveModal::Study(_)))
    }

    /// Dismiss the topmost modal on Esc/Back. Returns true if a modal was present
    /// (whether dismissed or blocking dismissal like `SyncAsk`).
    pub(crate) fn handle_escape(&mut self) -> bool {
        let Some(top) = self.stack.last() else { return false };
        if !top.blocks_escape() {
            self.stack.pop();
        }
        true
    }
}

/// Canvas interaction, ink, viewport, and block editing state.
pub(crate) struct CanvasState {
    pub tool: Tool,
    pub current: Option<Stroke>,
    pub anchor: Option<[f32; 2]>,
    pub lasso: Vec<Pos2>,
    pub selected: Vec<u64>,
    pub moving: bool,
    pub force: Option<f32>,
    pub frame_force: Option<f32>,
    pub editing: Option<u64>,
    /// Size of the page area last frame (to notice the on-screen keyboard taking room).
    pub canvas_avail: Vec2,
    /// Typing a note with the on-screen keyboard up: the toolbar and page header step aside.
    pub typing_kb: bool,
    /// Where a press on a selection's buttons began (touch forgets it by the time the finger lifts).
    pub sel_press: Option<Pos2>,
    /// Fingers on the screen now (where each started), and the current touch gesture:
    /// (began, most fingers, any moved, when the first finger lifted).
    pub fingers: HashMap<u64, Pos2>,
    pub gesture: (f64, usize, bool, f64),
    /// Each page's zoom.
    pub scales: HashMap<u64, f32>,
    /// Tool to restore after erasing with stylus eraser end or side button.
    pub tool_before_erase: Option<Tool>,
    pub restore_tool: bool,
    pub last_text: Option<u64>,
    pub fmt: Option<Fmt>,
    pub input: Input,
    /// A pen tool is selected but this input (mouse/finger) doesn't draw: it works as usual instead.
    pub ink_pass: bool,
    pub mouse_hint: bool,
    /// Visible part of each page's canvas (zoom and pan), in page coordinates.
    pub views: HashMap<u64, egui::Rect>,
    pub pan: Vec2,
    pub zoom_req: Option<ZoomReq>,
    pub zoom_now: f32,
    /// Where (and since when) the pen has been holding still — a hold snaps the shape.
    pub still: (Pos2, f64),
    pub snapped: bool,
    /// Resizing (0) or rotating (1) the selected ink, around this point.
    pub xform: Option<(u8, Pos2)>,
    /// The "/" menu or "[[" page list open in a text note.
    pub popup: Option<canvas::Popup>,
    pub open_link: Option<String>,
    pub insert_below: Option<(u64, Kind)>,
    /// Whole-page PDF export in progress (photographed tile by tile).
    pub capture: Option<canvas::Capture>,
    pub want_snap: bool,
    /// Where a text note was clicked to start editing it (in page coordinates).
    pub click_at: Option<(u64, Pos2)>,
    /// Take an undo step from `press_page` at the end of this frame.
    pub snap_press: bool,
    pub press_page: Option<Page>,
    /// A printed PDF page unpinned to be moved or resized.
    pub unpinned: Option<u64>,
    /// Ink copied or cut with the lasso, ready to paste.
    pub ink_clip: Vec<Stroke>,
    /// Search words to scroll to on the page just opened from a search result.
    pub find_text: Option<String>,
    /// A note to turn into a flashcard (front, back), from its menu.
    pub make_card: Option<(String, String)>,
    /// A tap on a printed PDF page: start a note there (screen position).
    pub note_at: Option<Pos2>,
    pub heights: HashMap<u64, f32>,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            tool: Tool::Type,
            current: None,
            anchor: None,
            lasso: vec![],
            selected: vec![],
            moving: false,
            force: None,
            frame_force: None,
            editing: None,
            canvas_avail: Vec2::ZERO,
            typing_kb: false,
            sel_press: None,
            fingers: HashMap::new(),
            gesture: (0.0, 0, false, 0.0),
            scales: HashMap::new(),
            tool_before_erase: None,
            restore_tool: false,
            last_text: None,
            fmt: None,
            input: Input::Mouse,
            ink_pass: false,
            mouse_hint: false,
            views: HashMap::new(),
            pan: Vec2::ZERO,
            zoom_req: None,
            zoom_now: 1.0,
            still: (Pos2::ZERO, 0.0),
            snapped: false,
            xform: None,
            popup: None,
            open_link: None,
            insert_below: None,
            capture: None,
            want_snap: false,
            click_at: None,
            snap_press: false,
            press_page: None,
            unpinned: None,
            ink_clip: vec![],
            find_text: None,
            make_card: None,
            note_at: None,
            heights: HashMap::new(),
        }
    }
}

/// Navigation, responsive breakpoints, search, rename, and focus state.
pub(crate) struct NavState {
    /// A narrow window or phone: side panels fold away after picking a page.
    pub narrow: bool,
    /// The screen is short (a phone on its side): the toolbar starts folded away.
    pub short: bool,
    /// Last input was touch: bigger buttons and spacing.
    pub touch_ui: bool,
    /// The all-tasks list is showing in the page column.
    pub tasks: bool,
    pub search: String,
    /// Where search looks: 0 everywhere, 1 this notebook, 2 this section.
    pub search_scope: u8,
    pub tag_filter: Option<Tag>,
    /// Active inline rename: `(panel_tag, item_id, buffer)`.
    pub rename: Option<(u8, u64, String)>,
    pub commit_rename: bool,
    /// Item id queued for deletion from inside a context menu.
    pub pending_delete: Option<u64>,
    /// Pages visited before (back) and after (forward) the open one, by id.
    pub nav_back: Vec<u64>,
    pub nav_fwd: Vec<u64>,
    pub navigating: bool,
    /// Scroll the page list to the open page.
    pub reveal: bool,
    /// When something was last moved to Recently deleted (an Undo button shows for a few seconds).
    pub undo_delete: Option<f64>,
    pub focus_id: Option<egui::Id>,
    /// Page title as it was when its field got focus (to update `[[links]]` on blur).
    pub title_was: Option<String>,
}

impl Default for NavState {
    fn default() -> Self {
        Self {
            narrow: false,
            short: false,
            touch_ui: false,
            tasks: false,
            search: String::new(),
            search_scope: 0,
            tag_filter: None,
            rename: None,
            commit_rename: false,
            pending_delete: None,
            nav_back: vec![],
            nav_fwd: vec![],
            navigating: false,
            reveal: true,
            undo_delete: None,
            focus_id: None,
            title_was: None,
        }
    }
}

/// Progress or final result from a background audio transcription job.
pub(crate) enum TranscribeEvent {
    Progress(usize, usize),
    Done(Result<Vec<(f32, String)>, String>),
}

/// Background jobs: AI requests, non-AI jobs (printout/export/offline ink), transcription, updates.
#[derive(Default)]
pub(crate) struct JobsState {
    pub ai_jobs: Vec<AiJob>,
    pub bg_jobs: Vec<BgJob>,
    pub ai_prompt: String,
    pub eq_prompt: String,
    pub ai_log: Vec<(bool, String)>,
    pub test_result: Option<Result<String, String>>,
    /// Insert the next AI answer below the note last written in.
    pub insert_near: bool,
    /// Transcript in progress: `(page, recording start, cancel token, event rx)`.
    pub transcribe: Option<(u64, f64, Arc<AtomicBool>, Receiver<TranscribeEvent>)>,
    /// `(chunks_done, chunks_total)` for the running transcription.
    pub transcribe_progress: Option<(usize, usize)>,
    pub transcribe_req: Option<u64>,
    /// Newer release: `(version, url, notes)`, and the running check.
    pub update: Option<(String, String, String)>,
    pub update_rx: Option<Receiver<crate::updates::UpdateResult>>,
    pub update_manual: bool,
}

impl JobsState {
    pub(crate) fn ai_busy(&self) -> bool { !self.ai_jobs.is_empty() }
    pub(crate) fn bg_busy(&self) -> bool { !self.bg_jobs.is_empty() }
    pub(crate) fn cancel_ai(&mut self) {
        for j in &self.ai_jobs { j.cancel.store(true, Ordering::Relaxed); }
        self.ai_jobs.clear();
    }
}

/// Audio recording and playback state.
pub(crate) struct AudioState {
    /// Recording in progress: recorder, page id, recording id, start time.
    pub recorder: Option<(audio::Recorder, u64, u64, f64)>,
    pub rec_toggle: bool,
    pub player: Option<audio::Player>,
    /// Page id and title of the currently loaded player (for global mini-player bar).
    pub player_page: Option<(u64, String)>,
    /// "Tap notes to jump": clicking a note or ink plays the recording from that moment.
    pub listen: bool,
    /// Playback speed of recordings (1.0 = normal).
    pub speed: f32,
    /// Waiting for Android's microphone permission dialog (since when).
    pub mic_wait: Option<f64>,
    pub jump: Option<f64>,
    pub recordings_span: Option<(f64, f64)>,
    pub fade_after: Option<(f64, f64, f64)>,
}

impl Default for AudioState {
    fn default() -> Self {
        Self {
            recorder: None,
            rec_toggle: false,
            player: None,
            player_page: None,
            listen: true,
            speed: 1.0,
            mic_wait: None,
            jump: None,
            recordings_span: None,
            fade_after: None,
        }
    }
}

/// Sync settings, active sync channel, and Google OAuth sign-in state.
pub(crate) struct SyncState {
    pub settings: sync::SyncSettings,
    /// Running sync: the notes as they were sent, and where its result arrives.
    pub rx: Option<(Store, Receiver<Result<sync::Outcome, String>>)>,
    /// When the last sync started (or may next retry), wall-clock seconds.
    pub at: f64,
    pub ok: u64,
    pub err: String,
    pub pending: bool,
    pub now: bool,
    pub google_rx: Option<GoogleWait>,
    pub google_paste: String,
    /// Saves so far, and at the start of the running sync round.
    pub saves: u64,
    pub sync_saves: u64,
}

impl SyncState {
    pub(crate) fn new(settings: sync::SyncSettings) -> Self {
        Self {
            settings,
            rx: None,
            at: 0.0,
            ok: 0,
            err: String::new(),
            pending: false,
            now: false,
            google_rx: None,
            google_paste: String::new(),
            saves: 0,
            sync_saves: 0,
        }
    }
}

pub(crate) type UnlockOutcome = Result<(String, vault::Key, Vec<Page>, Option<Lock>), String>;
pub(crate) type LockOutcome = Result<(String, vault::Key, Lock), String>;

/// Background Argon2id lock / unlock operation so key derivation never stalls the UI thread.
pub(crate) enum PendingLockOp {
    Unlock {
        section: u64,
        /// `(salt, key, decrypted_pages, optional_upgraded_v1_lock)`
        rx: Receiver<UnlockOutcome>,
    },
    Lock {
        section: u64,
        /// `(salt, key, sealed_lock)`
        rx: Receiver<LockOutcome>,
    },
}

/// Locked section keys, seal cache, and idle auto-relock timer.
pub(crate) struct LockState {
    /// `(salt, key)` of locked sections opened in this session (memory only).
    pub keys: HashMap<u64, (String, vault::Key)>,
    /// Sealed form of each opened locked section, and the pages hash it was made from.
    pub seal_cache: HashMap<u64, (u64, Lock)>,
    pub unlock_pw: String,
    pub unlock_err: String,
    pub pending: Option<PendingLockOp>,
    /// When the person last did something (opened locked sections close after idle timeout).
    pub last_input: f64,
}

impl LockState {
    pub(crate) fn new() -> Self {
        Self {
            keys: HashMap::new(),
            seal_cache: HashMap::new(),
            unlock_pw: String::new(),
            unlock_err: String::new(),
            pending: None,
            last_input: now_f(),
        }
    }
}

impl Drop for LockState {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.unlock_pw);
    }
}

/// Background image / PDF-page decoder thread so opening image-heavy pages never stalls the UI.
pub(crate) struct ImageLoader {
    req_tx: std::sync::mpsc::Sender<(String, PathBuf)>,
    res_rx: Receiver<(String, Option<egui::ColorImage>)>,
    pub in_flight: std::collections::HashSet<String>,
}

impl ImageLoader {
    fn start() -> Self {
        let (req_tx, req_rx) = std::sync::mpsc::channel::<(String, PathBuf)>();
        let (res_tx, res_rx) = std::sync::mpsc::channel::<(String, Option<egui::ColorImage>)>();
        std::thread::spawn(move || {
            while let Ok((file, path)) = req_rx.recv() {
                let ci = (|| -> Option<egui::ColorImage> {
                    let mut img = image::ImageReader::open(&path).ok()?.with_guessed_format().ok()?.decode().ok()?;
                    if img.width().max(img.height()) > 4096 {
                        img = img.resize(4096, 4096, image::imageops::FilterType::Triangle);
                    }
                    let img = img.to_rgba8();
                    Some(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
                })();
                if res_tx.send((file, ci)).is_err() {
                    break;
                }
            }
        });
        Self { req_tx, res_rx, in_flight: std::collections::HashSet::new() }
    }
}

/// Derived caches (backlinks, search hits, switcher hits, textures).
pub(crate) struct CacheState {
    pub page_titles: Vec<String>,
    pub backlinks: Vec<(String, (usize, usize, usize))>,
    pub backlinks_for: u64,
    pub open_tasks_n: usize,
    pub due_now: usize,
    pub derived_at: f64,
    pub derived_dirty: bool,
    pub search_cache: Option<(String, u8, f64, Vec<chrome::SearchHit>)>,
    pub switch_cache: Option<(String, Vec<chrome::SwitchHit>)>,
    /// Decoded pictures: `(texture_handle, rgba_bytes, last_used_time)`.
    pub textures: HashMap<String, (Option<TextureHandle>, usize, f64)>,
    /// Background image decoder thread (lazily spawned on first image request).
    pub image_loader: Option<ImageLoader>,
    pub conflict_pair: Option<(u64, u64)>,
}

impl Default for CacheState {
    fn default() -> Self {
        Self {
            page_titles: vec![],
            backlinks: vec![],
            backlinks_for: 0,
            open_tasks_n: 0,
            due_now: 0,
            derived_at: -10.0,
            derived_dirty: true,
            search_cache: None,
            switch_cache: None,
            textures: HashMap::new(),
            image_loader: None,
            conflict_pair: None,
        }
    }
}

impl CacheState {
    pub(crate) fn request_image(&mut self, file: String, path: PathBuf) {
        let loader = self.image_loader.get_or_insert_with(ImageLoader::start);
        if loader.in_flight.insert(file.clone()) {
            let _ = loader.req_tx.send((file, path));
        }
    }

    pub(crate) fn poll_images(&mut self, ctx: &egui::Context, now: f64) {
        let mut uploaded = false;
        if let Some(loader) = &mut self.image_loader {
            while let Ok((file, ci)) = loader.res_rx.try_recv() {
                loader.in_flight.remove(&file);
                match ci {
                    Some(ci) => {
                        let bytes = ci.size[0].saturating_mul(ci.size[1]).saturating_mul(4);
                        let tex = ctx.load_texture(file.clone(), ci, egui::TextureOptions::LINEAR);
                        self.textures.insert(file, (Some(tex), bytes, now));
                    }
                    None => {
                        if files_path(&file).exists() {
                            self.textures.insert(file, (None, 0, now));
                        }
                    }
                }
                uploaded = true;
            }
            if !loader.in_flight.is_empty() {
                ctx.request_repaint_after(std::time::Duration::from_millis(40));
            }
        }
        if uploaded {
            ctx.request_repaint();
        }
        // Bounded VRAM LRU cache: evict entries idle > 300s, and cap total RGBA bytes (64 MiB mobile, 128 MiB desktop).
        self.textures.retain(|_, (_, _, used)| now - *used < 300.0);
        const VRAM_BUDGET: usize = if cfg!(target_os = "android") { 64 * 1024 * 1024 } else { 128 * 1024 * 1024 };
        let mut total: usize = self.textures.values().map(|(_, b, _)| *b).sum();
        if total > VRAM_BUDGET {
            let mut order: Vec<(String, f64, usize)> = self.textures.iter()
                .filter(|(_, (_, b, used))| *b > 0 && now - *used >= 1.0)
                .map(|(k, (_, b, used))| (k.clone(), *used, *b))
                .collect();
            order.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (k, _, b) in order {
                if total <= VRAM_BUDGET { break; }
                self.textures.remove(&k);
                total = total.saturating_sub(b);
            }
        }
    }
}

/// Everything except the notebooks themselves, so page and UI state can be borrowed together.
pub(crate) struct St {
    pub ai: ai::AiSettings,
    pub p: Prefs,
    pub tab: Tab,
    pub modals: ModalStack,
    pub canvas: CanvasState,
    pub nav: NavState,
    pub jobs: JobsState,
    pub audio: AudioState,
    pub sync_st: SyncState,
    pub locks: LockState,
    pub cache: CacheState,
    /// Focus timer: when the current period ends, and whether it's a break.
    pub focus: Option<(f64, bool)>,
    pub undo: Vec<(u64, Page)>,
    pub redo: Vec<(u64, Page)>,
    pub dirty: bool,
    pub since_save: f32,
    /// Time of the previous frame (for the save timer).
    pub prev_t: f64,
    pub status: String,
    pub status_age: f32,
    /// Dedicated background autosave writer thread (present when `persist` is true).
    pub save_worker: Option<storage::SaveWorker>,
    /// Text read from handwriting and PDFs, for search.
    pub index: index::Index,
    /// Compare every page at the next save (after changes elsewhere than the page in front).
    pub full_save: bool,
    pub full_at: f64,
    /// Held while this window owns the data folder (see `instance_lock`).
    #[allow(dead_code)]
    pub instance: Option<std::fs::File>,
    /// Android's status and navigation bars (top, bottom, left, right) in points.
    pub bars: [f32; 4],
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub bars_at: f64,
    /// When the Android file picker was opened.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub picking_since: f64,
    /// Since when nothing wants the keyboard on Android.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub kb_idle_since: Option<f64>,
    pub opened_args: bool,
    pub theme_dirty: bool,
    pub persist: bool,
}

impl St {
    /// Remember the page before a change so Undo can bring it back.
    pub(crate) fn snap(&mut self, page: &Page) {
        if self.undo.iter().rev().find(|(id, _)| *id == page.id).is_some_and(|(_, p)| same_page(p, page)) {
            self.dirty = true;
            self.cache.derived_dirty = true;
            return;
        }
        self.undo.push((page.id, page.clone()));
        if self.undo.len() > 80 { self.undo.remove(0); }
        self.redo.clear();
        self.dirty = true;
        self.cache.derived_dirty = true;
    }

    pub(crate) fn say(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        self.status = tr(&m).to_string();
        self.status_age = 0.0;
    }

    /// The chosen pen (there is always at least one).
    pub(crate) fn pen(&mut self) -> &mut Pen {
        if self.p.pens.is_empty() { self.p.pens = default_pens(); }
        self.p.pen = self.p.pen.min(self.p.pens.len() - 1);
        &mut self.p.pens[self.p.pen]
    }
}
