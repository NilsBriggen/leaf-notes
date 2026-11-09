//! First-run tour, help, "What's new" after an update, and sending feedback.
use crate::*;
use eframe::egui::{RichText, Sense};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const ISSUES: &str = "https://github.com/NilsBriggen/leaf-notes/issues/new";

/// What changed in each version, newest first (shown once after an update).
const CHANGES: &[(&str, &[&str])] = &[
    ("0.3.1", &[
        "The eraser has its own settings: a size, and \"Part of a stroke\" or \"Whole stroke\" as one clear switch.",
        "The highlighter has a size and can take any colour.",
        "Pens can be removed with a visible button; recording speed is picked from a menu.",
        "A ⌃ button hides the toolbar, and View ▸ Dark mode stays on even when the theme followed the system.",
    ]),
    ("0.3.0", &[
        "Phones: every screen fits, pages fit the width when opened, the toolbar folds away on short screens, and Back closes whatever is open.",
        "Gestures: tap with two fingers to undo, three to redo; scribble back and forth over ink to erase it.",
        "Take a photo straight onto the page (Android). Pictures are stored upright and at a sensible size.",
        "Selected ink has its own Delete and Duplicate buttons.",
        "Recordings play at 0.75× to 2× speed.",
        "Search and the page finder forgive typos; tapping just above or below a note edits it.",
        "Exports open the share menu on Android, and saving is safer if the device switches off suddenly.",
    ]),
    ("0.2.3", &[
        "The title bar shows the same, more detailed logo as the app icon.",
        "The paper fills the whole page card, and panning stops at its edge instead of showing a blank border.",
    ]),
    ("0.2.2", &[
        "The paper fills the whole page card, and panning stops at its edge instead of showing a blank border.",
    ]),
    ("0.2.1", &[
        "More room to write: a slimmer page header with a clear edge where the paper starts.",
        "One set of pens: your pens, a highlighter group and the pen's settings, clearly labelled.",
        "Recordings can always be removed from their ⋯ menu, however narrow the window.",
        "Android: no more freezing when the screen rotates or dark mode switches.",
        "Typing with the on-screen keyboard keeps your note in view; tapping a page with a recording starts a note again.",
        "Many smaller layout, dark mode and wording fixes.",
    ]),
    ("0.2.0", &[
        "A short welcome tour, a help page and a feedback button.",
        "Page history: bring back an earlier version of any page (⋯ ▸ Page history).",
        "Lock a section with a password; its pages are encrypted.",
        "Restore a backup and import Markdown files and folders (Settings ▸ Your data).",
        "Pages changed on two devices can be compared side by side.",
        "Notes are stored one file per page: faster with big notebooks and safer.",
        "Search finds words in handwriting and PDF printouts.",
        "Recordings can be turned into text with your AI key.",
        "On phones the side panels fold away; better pen handling and accessibility.",
    ]),
];

impl NotesApp {
    /// Decide what to show at start: the tour on a new install, What's new after an update.
    pub(crate) fn greet(&mut self, fresh_install: bool) {
        if fresh_install { self.st.tour = Some(0); self.st.tour_fresh = true; }
        else if self.st.p.seen_version != VERSION && CHANGES.iter().any(|(v, _)| *v == VERSION) { self.st.show_news = true; }
        if self.st.p.seen_version != VERSION { self.st.p.seen_version = VERSION.into(); self.st.dirty = true; }
    }

    pub(crate) fn tour_window(&mut self, ctx: &egui::Context) {
        let Some(mut step) = self.st.tour else { return };
        const STEPS: u8 = 5;
        let (mut close, mut empty_start) = (false, false);
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(26)).corner_radius(20);
        egui::Modal::new(egui::Id::unique("tour")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 520.0));
            let big = |ui: &mut egui::Ui, t: &str| { ui.label(RichText::new(tr(t)).font(rich::serif(26.0)).color(ui.visuals().strong_text_color())); ui.add_space(6.0); };
            let text = |ui: &mut egui::Ui, t: &str| { ui.label(RichText::new(tr(t)).size(15.0)); ui.add_space(4.0); };
            // the steps differ in length: keep the buttons below them in one place
            ui.vertical(|ui| { ui.set_min_height(250.0); match step {
                0 => {
                    big(ui, "Welcome to Leaf Notes");
                    text(ui, "Your notes live in notebooks. A notebook has sections (like the tabs of a binder), and each section has pages.");
                    text(ui, "Pick a notebook at the top left, a section below it, and a page in the next column.");
                    text(ui, "Everything stays on this device and works offline.");
                    text(ui, "Ctrl+K jumps to any page; type [[ in a note to link another page.");
                }
                1 => {
                    big(ui, "Type or write");
                    text(ui, "Click anywhere on a page to start typing there. Type / for headings, lists, tables, equations and more.");
                    text(ui, "Choose a pen in the Draw tab to write by hand. A stylus always draws; handwriting can be turned into text or maths.");
                    text(ui, "Record a lecture while you write, then tap your notes to hear that moment again.");
                }
                2 => {
                    big(ui, "Make it yours");
                    ui.label(RichText::new(tr("Language")).strong());
                    ui.horizontal_wrapped(|ui| {
                        for (code, name) in i18n::LANGUAGES {
                            if ui.selectable_label(self.st.p.lang == code, if code.is_empty() { tr(name) } else { name }).clicked() { self.st.p.lang = code.into(); apply_prefs(ctx, &self.st.p); self.st.dirty = true; }
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new(tr("Theme")).strong());
                    ui.horizontal(|ui| {
                        for (dark, name) in [(false, "Light"), (true, "Dark")] {
                            if ui.selectable_label(self.st.p.dark == dark && !self.st.p.follow_system, tr(name)).clicked() { self.st.p.follow_system = false; self.st.p.dark = dark; apply_prefs(ctx, &self.st.p); self.st.dirty = true; }
                        }
                        if ui.selectable_label(self.st.p.follow_system, tr("System")).clicked() { self.st.p.follow_system = true; apply_prefs(ctx, &self.st.p); self.st.dirty = true; }
                    });
                    ui.add_space(8.0);
                    text(ui, "More in Settings (⚙, top right): pens, paper, text size and more.");
                }
                3 => {
                    big(ui, "Optional extras");
                    text(ui, "AI with your own key: summaries, flashcards, rewriting and reading handwriting. Your key stays on this device.");
                    text(ui, "Sync keeps notes the same on all your devices, through Google Drive, a synced folder or a WebDAV server.");
                    ui.horizontal(|ui| {
                        if ui.button(tr("Add an AI key…")).clicked() { self.st.show_settings = true; self.st.settings_tab = 3; close = true; }
                        if ui.button(tr("Set up sync…")).clicked() { self.st.show_settings = true; self.st.settings_tab = 5; close = true; }
                    });
                    ui.label(RichText::new(tr("Both can be set up any time later in Settings.")).weak());
                }
                _ => {
                    big(ui, "Ready");
                    // only on the first start: seen again from Help, the notes are the person's own
                    if self.st.tour_fresh {
                        text(ui, "There's a sample notebook to look around in. Keep it, or start with an empty one.");
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new(RichText::new(tr("Keep the sample notes")).color(Color32::WHITE)).fill(accent_base()).corner_radius(10)).clicked() { close = true; }
                            if ui.button(tr("Start with an empty notebook")).clicked() { empty_start = true; close = true; }
                        });
                    } else if ui.add(egui::Button::new(RichText::new(tr("Done")).color(Color32::WHITE)).fill(accent_base()).corner_radius(10)).clicked() { close = true; }
                    text(ui, "You can see this tour again from Help (?, top right).");
                }
            }});
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                // progress dots
                for i in 0..STEPS {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.0, if i == step { accent(ui) } else { ui.visuals().widgets.inactive.bg_fill });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if step + 1 < STEPS && ui.add(egui::Button::new(RichText::new(tr("Next")).color(Color32::WHITE)).fill(accent_base()).min_size(Vec2::new(90.0, 30.0))).clicked() { step += 1; }
                    if step > 0 && ui.button(tr("Back")).clicked() { step -= 1; }
                    if step + 1 < STEPS && ui.add(egui::Button::new(tr("Skip")).frame_when_inactive(false)).clicked() { close = true; }
                });
            });
        });
        if empty_start {
            // the sample goes to Recently deleted rather than away for good
            let now = now_secs();
            for n in std::mem::take(&mut self.store.notebooks) { self.store.trash.push(Trashed { label: format!("Notebook “{}”", n.name), deleted: now, item: TrashItem::Notebook(n), at: None }); }
            self.store.notebooks = vec![Notebook { id: new_id(), name: tr("My Notebook").into(), color: PALETTE[0], sections: vec![Section { id: new_id(), name: tr("Quick Notes").into(), color: PALETTE[1], pages: vec![Page::new(tr("First page"))], lock: None }] }];
            (self.nb, self.sec, self.pg) = (0, 0, 0);
            self.st.dirty = true;
        }
        self.st.tour = if close { None } else { Some(step) };
        if close { self.st.tour_fresh = false; }
    }

    pub(crate) fn news_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        egui::Window::new(tf!("What's new in Leaf Notes {v}", v = VERSION)).open(&mut open).collapsible(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).resizable(false).default_width(crate::fit_w(ctx, 460.0)).max_width(crate::fit_w(ctx, 460.0)).show(ctx, |ui| {
            if let Some((_, items)) = CHANGES.iter().find(|(v, _)| *v == VERSION) {
                for item in *items { ui.label(RichText::new(format!("•  {}", tr(item))).size(14.0)); }
            }
            ui.add_space(8.0);
            if ui.button(tr("Got it")).clicked() { self.st.show_news = false; }
        });
        if !open { self.st.show_news = false; }
    }

    pub(crate) fn help_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let topics: [(&str, &str); 11] = [
            ("Gestures", "Pinch to zoom; two fingers scroll. Tap with two fingers to undo, three to redo. Scribble back and forth over ink to erase it. Long-press (or right-click) for more options. On Android, Back closes whatever is open."),
            ("Organising", "Notebooks hold sections, sections hold pages. Right-click a notebook, section or page to rename, move, colour or delete it; deleted things wait in Recently deleted. Right-click a page ▸ Make subpage to tuck it under the one above. Ctrl+K jumps to any page."),
            ("Typing", "Click empty paper to type. Type / for headings, lists, checklists, tables, code, equations and more, and [[ to link to another page. **bold**, *italic*, ==highlight== and $maths$ work as you type."),
            ("Pen and handwriting", "Pick a pen in the Draw tab. A stylus always draws; the mouse and fingers only if you allow it in Settings ▸ Pen & input. Select ink with the lasso to move it, or convert it to text or maths. The built-in reader knows printed letters, also ä, é, ç and the like; for joined-up handwriting switch on Settings ▸ AI ▸ Use this AI for handwriting. Search also finds words in your handwriting and in PDF printouts."),
            ("Maths", "Write LaTeX between $ signs inside text, or insert an Equation note for bigger formulas."),
            ("Flashcards", "Make a deck from the Study tab, by hand, from your notes or with AI. Study shows the cards that are due."),
            ("Recordings", "Press Record while you take notes. Later, tap a note or ink to hear what was said at that moment, or turn the recording into text with an AI key."),
            ("AI", "Add your own key in Settings ▸ AI. The assistant can summarise, quiz you, rewrite and read handwriting. Only what you ask about is sent."),
            ("Sync and backups", "Settings ▸ Sync keeps devices in step. Settings ▸ Your data saves and restores backups; ⋯ ▸ Page history brings back earlier versions of a page."),
            ("Locked sections", "Right-click a section ▸ Lock with a password. Its pages are encrypted; without the password they can't be opened, by anyone."),
            ("Keyboard", "Settings ▸ Your data ▸ Keyboard shortcuts lists them all."),
        ];
        egui::Window::new(tr("Help")).open(&mut open).collapsible(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).default_size([crate::fit_w(ctx, 560.0), 520.0f32.min(ctx.content_rect().height() - 60.0)]).max_width(crate::fit_w(ctx, 2000.0)).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for (title, body) in topics {
                    ui.label(RichText::new(tr(title)).size(16.0).strong());
                    // touch screens have no right-click: say long-press instead
                    let body = if self.st.touch_ui { tr(body).replace("Right-click", tr("Long-press")).replace("Rechtsklick", tr("Long-press")).replace("Clic droit", tr("Long-press")) } else { tr(body).to_string() };
                    ui.label(body);
                    ui.add_space(8.0);
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("Show the welcome tour")).clicked() { self.st.tour = Some(0); self.st.show_help = false; }
                if ui.button(tr("What's new")).clicked() { self.st.show_news = true; }
                if ui.button(tr("✉  Send feedback")).clicked() {
                    let body = format!("\n\n---\nLeaf Notes {VERSION} · {} {}", std::env::consts::OS, std::env::consts::ARCH);
                    ctx.open_url(egui::OpenUrl::new_tab(format!("{ISSUES}?body={}", enc(&body))));
                }
            });
            ui.label(RichText::new(tf!("Leaf Notes {v} · free software under the GPL-3.0", v = VERSION)).size(11.5).weak());
        });
        if !open { self.st.show_help = false; }
    }
}

/// Crash reports: if the app itself panics, a short report is kept; next start asks whether to send it.
pub(crate) fn install_crash_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    static UI: std::sync::OnceLock<std::thread::ThreadId> = std::sync::OnceLock::new();
    ONCE.call_once(|| {
        let _ = UI.set(std::thread::current().id());
        let dir = data_dir();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // background jobs (reading handwriting, PDFs …) catch their own failures: only the app itself counts
            if UI.get() == Some(&std::thread::current().id()) {
                // where it happened, never what was being worked on: quoted text (a note's words in a
                // "byte index … of `…`" message) is cut out, and the home folder's name with it
                let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
                let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
                let trace = std::backtrace::Backtrace::force_capture().to_string();
                let text = without_home(&format!("Leaf Notes {VERSION} · {} {}\n\npanicked at {at}: {}\n\n{trace}", std::env::consts::OS, std::env::consts::ARCH, redact(&msg)));
                let _ = std::fs::write(dir.join(format!("crash-{}.txt", now_secs())), text);
            }
            previous(info);
        }));
    });
}

/// Reports left by earlier crashes, newest first.
pub(crate) fn crash_reports() -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(data_dir()).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))).collect();
    v.sort();
    v.reverse();
    v
}

impl NotesApp {
    pub(crate) fn crash_window(&mut self, ctx: &egui::Context) {
        let Some(path) = self.st.crash.clone() else { return };
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let (mut send, mut delete) = (false, false);
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
        egui::Modal::new(egui::Id::unique("crash")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 520.0));
            ui.label(RichText::new(tr("Leaf Notes closed unexpectedly")).font(rich::serif(22.0)).color(ui.visuals().strong_text_color()));
            ui.label(tr("Your notes were saved up to a few seconds before. A short report of what went wrong can help fix it; it contains no notes, only where in the program it happened. You'll see it before it's sent."));
            ui.add_space(6.0);
            // exactly what would be sent
            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| { ui.label(RichText::new(report_part(&text)).monospace().size(11.0).weak()); });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(tr("Send report…")).color(Color32::WHITE)).fill(accent_base())).clicked() { send = true; }
                if ui.button(tr("Delete it")).clicked() { delete = true; }
            });
        });
        if send {
            let body: String = format!("What were you doing when it happened?\n\n\n```\n{}\n```", report_part(&text));
            ctx.open_url(egui::OpenUrl::new_tab(format!("{ISSUES}?title={}&body={}", enc("Crash report"), enc(&body))));
        }
        if send || delete {
            for p in crash_reports() { let _ = std::fs::remove_file(p); }
            self.st.crash = None;
        }
    }
}

/// The part of a report that's sent: short enough for a web address (each character can take 3).
fn report_part(text: &str) -> String { text.chars().take(2000).collect() }

/// A panic message without the text it quotes, on one line.
fn redact(msg: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    for c in msg.lines().next().unwrap_or("").chars() {
        match quote {
            Some(q) if c == q => { quote = None; out.push('…'); out.push(c); }
            Some(_) => {}
            None => { out.push(c); if matches!(c, '`' | '"') { quote = Some(c); } }
        }
    }
    out.chars().take(300).collect()
}

fn without_home(s: &str) -> String {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    if home.len() > 1 { s.replace(&home, "~") } else { s.to_string() }
}

fn enc(s: &str) -> String { s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect() }

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_a_report_when_the_app_crashes() {
        let dir = std::env::temp_dir().join(format!("ln-crash-{}", crate::model::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("LEAFNOTES_DATA", &dir);
        super::install_crash_hook();
        let _ = std::panic::catch_unwind(|| panic!("test crash"));
        let reports = super::crash_reports();
        std::env::remove_var("LEAFNOTES_DATA");
        assert_eq!(reports.len(), 1);
        let text = std::fs::read_to_string(&reports[0]).unwrap();
        assert!(text.contains("test crash") && text.contains(super::VERSION));
        // a note's words in the message never reach the report
        let r = super::redact("byte index 3 is not a char boundary of `my secret note`\nmore \"also secret\"");
        assert!(!r.contains("secret") && r.contains("byte index 3"), "{r}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
