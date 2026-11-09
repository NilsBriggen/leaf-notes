//! The Settings window: appearance, pen & input, notes & pages, AI, your data and sync.
use crate::chrome::{ago, heading, row, row_below, toggle};
use crate::*;
use eframe::egui::{Align, Layout, RichText, Sense};

impl NotesApp {
    pub(crate) fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut test = false;
        #[allow(unused_mut)]
        let (mut restore, mut import, mut backup_to): (Option<std::path::PathBuf>, Vec<std::path::PathBuf>, Option<std::path::PathBuf>) = (None, vec![], None);
        #[cfg_attr(target_os = "android", allow(unused_mut))] // the PDF picker is desktop-only
        let mut pdfs: Vec<std::path::PathBuf> = vec![];
        let before = serde_json::to_string(&self.st.p).unwrap_or_default();
        // phones: a thinner frame, and the section list shows only icons
        let narrow = ctx.content_rect().width() < 640.0;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(if narrow { 12 } else { 22 })).corner_radius(18);
        let modal = egui::Modal::new(egui::Id::unique("settings")).frame(frame).show(ctx, |ui| {
            let size = Vec2::new(if narrow { ctx.content_rect().width() - 40.0 } else { (ctx.content_rect().width() - 80.0).clamp(560.0, 780.0) }, (ctx.content_rect().height() - 120.0 - self.st.bars[0] - self.st.bars[1]).clamp(200.0, 560.0)); // (minus Android's bars)
            ui.set_width(size.x);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Settings")).font(rich::serif(24.0)).color(ui.visuals().strong_text_color()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("✕").size(15.0)).frame_when_inactive(false)).tip("Close (Esc)").clicked() { open = false; }
                });
            });
            ui.add_space(10.0);
            ui.horizontal_top(|ui| {
                ui.set_height(size.y);
                // section list
                ui.vertical(|ui| {
                    ui.set_width(if narrow { 40.0 } else { 170.0 });
                    for (i, (icon, name)) in [("🎨", "Appearance"), ("✏", "Pen & input"), ("¶", "Notes & pages"), ("✦", "AI"), ("💾", "Your data"), ("🔄", "Sync")].iter().enumerate() {
                        let sel = self.st.settings_tab == i as u8;
                        // icons in their own fixed column, so the names line up whatever the icon's width
                        let (rect, r) = ui.allocate_exact_size(Vec2::new(if narrow { 38.0 } else { 166.0 }, if narrow { 40.0 } else { 34.0 }), Sense::click());
                        if sel || r.hovered() { ui.painter().rect_filled(rect, 8.0, if sel { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.hovered.weak_bg_fill }); }
                        let col = if sel { accent(ui) } else { ui.visuals().text_color() };
                        ui.painter().text(rect.left_center() + Vec2::new(18.0, 0.0), egui::Align2::CENTER_CENTER, *icon, egui::FontId::proportional(14.0), col);
                        if !narrow { ui.painter().text(rect.left_center() + Vec2::new(38.0, 0.0), egui::Align2::LEFT_CENTER, tr(name), egui::FontId::proportional(14.0), col); }
                        crate::chrome::focus_ring(ui, &r, rect, 8.0);
                        r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, sel, tr(name)));
                        if r.clicked() { self.st.settings_tab = i as u8; }
                    }
                });
                ui.add_space(if narrow { 2.0 } else { 8.0 });
                ui.separator();
                ui.add_space(if narrow { 2.0 } else { 8.0 });
                egui::ScrollArea::vertical().id_salt("settings-scroll").auto_shrink([false, false]).max_height(size.y).show(ui, |ui| ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    let p = &mut self.st.p;
                    match self.st.settings_tab {
                        0 => {
                            heading(ui, "Appearance");
                            row_below(ui, "Theme", "Light, dark, or the same as the system.", |ui| {
                                let mut mode = if p.follow_system { 2 } else if p.dark { 1 } else { 0 };
                                for (i, l) in ["Light", "Dark", "System"].iter().enumerate() { ui.selectable_value(&mut mode, i, tr(l)); }
                                p.follow_system = mode == 2;
                                if mode < 2 { p.dark = mode == 1; }
                            });
                            row_below(ui, "Accent colour", "Used for the toolbar, selections and highlights.", |ui| {
                                for (name, c) in crate::ACCENTS {
                                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                                    ui.painter().circle_filled(r.center(), 11.0, c3(c));
                                    if p.accent == c { ui.painter().circle_stroke(r.center(), 13.0, EStroke::new(2.0, ui.visuals().strong_text_color())); }
                                    if resp.tip(name).clicked() { p.accent = c; }
                                }
                                let mut col = c3(p.accent);
                                if ui.color_edit_button_srgba(&mut col).tip("Any colour").changed() { p.accent = [col.r(), col.g(), col.b()]; }
                            });
                            row(ui, "Interface size", if self.st.touch_ui { "Makes everything larger or smaller." } else { "Makes everything larger or smaller (also Ctrl + / Ctrl −)." }, |ui| {
                                let mut z = ctx.zoom_factor();
                                if ui.add(egui::Slider::new(&mut z, 0.7..=2.0).step_by(0.05).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).changed() { ctx.set_zoom_factor(z); p.zoom = z; }
                            });
                            row_below(ui, "Language", "Choose the language of menus, buttons and messages.", |ui| {
                                for (code, name) in crate::i18n::LANGUAGES { ui.selectable_value(&mut p.lang, code.to_string(), if code.is_empty() { tr(name) } else { name }); }
                            });
                            row(ui, "High contrast", "Full-strength text and stronger borders, easier to read.", |ui| { toggle(ui, &mut p.high_contrast); });
                            row(ui, "Serif headings", "Page titles and headings in a book-style typeface.", |ui| { toggle(ui, &mut p.serif); });
                            row(ui, "Show the assistant pane", "The AI pane on the right of the page.", |ui| { toggle(ui, &mut p.show_ai); });
                            row(ui, "Keep the toolbar open", "Off: the toolbar folds away until you click a tab.", |ui| { toggle(ui, &mut p.ribbon_open); });
                        }
                        1 => {
                            heading(ui, "Pen & input");
                            ui.label(RichText::new(tr("A stylus always draws. Choose what else may draw when a pen, highlighter or shape is picked. Otherwise the mouse keeps working as usual (click notes, drag to move the page) and a finger scrolls.")).weak());
                            #[cfg(not(target_os = "android"))]
                            row(ui, "Draw with the mouse or trackpad", "On Linux, drawing tablets act like a mouse — switch this on to draw with them.", |ui| { toggle(ui, &mut p.draw_mouse); });
                            row(ui, "Draw with a finger", "Some phones report finger pressure like a pen; on those, fingers may still draw.", |ui| { toggle(ui, &mut p.draw_finger); });
                            row(ui, "Pressure-sensitive lines", "Press harder for thicker ink (fountain pen, pencil). Off: even lines.", |ui| { toggle(ui, &mut p.pressure); });
                            row(ui, "Smoothing", "Irons out shaky lines; higher values trail the pen slightly.", |ui| { ui.add(egui::Slider::new(&mut p.stabilizer, 0.0..=1.0).show_value(false)); });
                            row(ui, "Hold still to snap shapes", "Draw a line, box or circle and keep the pen still at the end: it turns into a perfect one.", |ui| {
                                toggle(ui, &mut p.snap_shapes);
                                if p.snap_shapes { ui.add(egui::Slider::new(&mut p.snap_delay, 0.3..=1.5).suffix(" s")).tip("How long to hold still"); }
                            });
                            row(ui, "Scribble to erase", "Scribbling back and forth over ink erases it (Undo brings it back).", |ui| { toggle(ui, &mut p.scratch_out); });
                            row(ui, "Straighten highlighter strokes", "Nearly straight highlights snap to a ruler-straight line.", |ui| { toggle(ui, &mut p.straighten_hl); });
                            row_below(ui, "Eraser", "Erase only what you touch, or whole strokes.", |ui| {
                                ui.selectable_value(&mut p.erase_partial, true, tr("Partial"));
                                ui.selectable_value(&mut p.erase_partial, false, tr("Whole strokes"));
                            });
                            row(ui, "My pens", "Restore the original six pens.", |ui| { if ui.button(tr("Reset pens")).clicked() { p.pens = default_pens(); p.pen = 0; } });
                        }
                        2 => {
                            heading(ui, "Notes & pages");
                            row(ui, "Text size", "Size of text in your notes.", |ui| { ui.add(egui::Slider::new(&mut p.note_size, 12.0..=22.0).step_by(0.5).suffix(" pt")); });
                            row_below(ui, "Line spacing", "Room between lines of text.", |ui| {
                                for (v, l) in [(1.3, "Compact"), (1.55, "Comfortable"), (1.8, "Airy")] { ui.selectable_value(&mut p.line, v, tr(l)); }
                            });
                            row(ui, "New note width", "How wide a new text note starts.", |ui| { ui.add(egui::Slider::new(&mut p.note_width, 280.0..=900.0).step_by(10.0).suffix(" px")); });
                            row_below(ui, "Paper for new pages", "Change any page's paper in View ▸ Paper.", |ui| {
                                for (v, _, l) in PAPERS { ui.selectable_value(&mut p.new_paper, v, tr(l)); }
                            });
                            row(ui, "Click empty paper to type", "Off: add notes from the Insert tab only.", |ui| { toggle(ui, &mut p.click_to_type); });
                            row(ui, "Continue lists", "Enter after a list item starts the next one.", |ui| { toggle(ui, &mut p.auto_lists); });
                        }
                        3 => {
                            heading(ui, "AI (bring your own key)");
                            let a = &mut self.st.ai;
                            ui.label(RichText::new(tr("Pick a service, paste your key. Your key stays on this device and is sent only to the server below.")).weak());
                            ui.horizontal_wrapped(|ui| {
                                if ui.button(tr("Anthropic")).clicked() { *a = ai::AiSettings { api_key: a.api_key.clone(), ..Default::default() }; }
                                if ui.button(tr("OpenAI")).clicked() { a.provider = ai::Provider::OpenAiCompatible; a.base_url = "https://api.openai.com/v1".into(); a.model = "gpt-4o".into(); }
                                if ui.button(tr("OpenRouter")).clicked() { a.provider = ai::Provider::OpenAiCompatible; a.base_url = "https://openrouter.ai/api/v1".into(); a.model = "anthropic/claude-sonnet-5.5".into(); }
                                if ui.button(tr("Ollama (on this computer)")).clicked() { a.provider = ai::Provider::OpenAiCompatible; a.base_url = "http://localhost:11434/v1".into(); a.model = "llama3.2-vision".into(); }
                                if ui.button(tr("LM Studio (on this computer)")).clicked() { a.provider = ai::Provider::OpenAiCompatible; a.base_url = "http://localhost:1234/v1".into(); a.model = "local-model".into(); }
                                if ui.button(tr("Groq")).clicked() { a.provider = ai::Provider::OpenAiCompatible; a.base_url = "https://api.groq.com/openai/v1".into(); a.model = "llama-3.3-70b-versatile".into(); a.transcribe_model = "whisper-large-v3".into(); }
                                if ui.button(tr("Mistral")).clicked() { a.provider = ai::Provider::OpenAiCompatible; a.base_url = "https://api.mistral.ai/v1".into(); a.model = "mistral-large-latest".into(); }
                            });
                            // where a key comes from, for someone who has never made one (and the free option)
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(tr("Get a key:")).size(12.0).weak());
                                ui.hyperlink_to(RichText::new("Anthropic").size(12.0), "https://console.anthropic.com/settings/keys");
                                ui.hyperlink_to(RichText::new("OpenAI").size(12.0), "https://platform.openai.com/api-keys");
                                ui.hyperlink_to(RichText::new("Groq").size(12.0), "https://console.groq.com/keys");
                                ui.label(RichText::new(tr("· or run one free on this computer with")).size(12.0).weak());
                                ui.hyperlink_to(RichText::new("Ollama").size(12.0), "https://ollama.com");
                            });
                            egui::Grid::new("ai_grid").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                                ui.label(tr("Service"));
                                ui.horizontal(|ui| {
                                    ui.radio_value(&mut a.provider, ai::Provider::Anthropic, tr("Anthropic API"));
                                    ui.radio_value(&mut a.provider, ai::Provider::OpenAiCompatible, tr("OpenAI-compatible"));
                                });
                                ui.end_row();
                                ui.label(tr("Server address")); ui.add(egui::TextEdit::singleline(&mut a.base_url).desired_width(300.0)); ui.end_row();
                                ui.label(tr("API key")); ui.add(egui::TextEdit::singleline(&mut a.api_key).password(true).hint_text(tr("paste your key")).desired_width(300.0)); ui.end_row();
                                ui.label(tr("Model")); ui.add(egui::TextEdit::singleline(&mut a.model).desired_width(300.0)); ui.end_row();
                                if a.provider == ai::Provider::OpenAiCompatible {
                                    ui.label(tr("Speech to text")); ui.add(egui::TextEdit::singleline(&mut a.transcribe_model).desired_width(300.0)).tip("Used to turn recordings into text, e.g. whisper-1 (OpenAI) or whisper-large-v3 (Groq)"); ui.end_row();
                                }
                            });
                            ui.label(RichText::new(tf!("Your key is kept in {place}.", place = crate::keys::place())).size(11.5).weak());
                            row_below(ui, "Your own prompts", "One per line; each becomes a button in the assistant (e.g. “Make 5 exam questions from this page”).", |ui| {
                                ui.add(egui::TextEdit::multiline(&mut p.prompts).desired_rows(3).desired_width(f32::INFINITY));
                            });
                            row(ui, "Use this AI for handwriting", "Off: the built-in recognizer reads handwriting offline. On: sent to your AI (better for cursive; needs a model that reads images).", |ui| { toggle(ui, &mut p.ink_ai); });
                            ui.horizontal(|ui| {
                                if ui.button(tr("Test connection")).clicked() { test = true; }
                                if self.st.ai_rx.iter().any(|(j, _)| matches!(j, Job::Test)) { ui.spinner(); }
                                match &self.st.test_result {
                                    Some(Ok(_)) => { ui.label(RichText::new(tr("✓ Connected")).color(accent(ui))); }
                                    Some(Err(e)) => { ui.label(RichText::new(format!("⚠ {e}")).color(Color32::from_rgb(220, 80, 60))); }
                                    None => {}
                                }
                            });
                        }
                        5 => {
                            heading(ui, "Sync");
                            ui.label(RichText::new(tr("Keep your notes the same on all your devices. Changes merge note by note; if the same page was changed on two devices at once, you keep both versions.")).weak());
                            let (mut g_start, mut g_cancel, mut g_out) = (false, false, false);
                            let s = &mut self.st.sync;
                            let before = s.clone();
                            row_below(ui, "Sync through", "Choose the same place on every device.", |ui| {
                                ui.selectable_value(&mut s.via, sync::Via::Off, tr("Off"));
                                ui.selectable_value(&mut s.via, sync::Via::Google, tr("Google Drive"));
                                // Android apps can't pick or reach other apps' folders: Drive or WebDAV there
                                #[cfg(not(target_os = "android"))]
                                ui.selectable_value(&mut s.via, sync::Via::Folder, tr("A synced folder"));
                                ui.selectable_value(&mut s.via, sync::Via::WebDav, tr("A WebDAV server"));
                            });
                            match s.via {
                                sync::Via::Folder => row_below(ui, "Folder", "A folder that Dropbox, OneDrive, Google Drive, iCloud or Syncthing keeps in sync. Leaf Notes puts a LeafNotes folder inside it.", |ui| {
                                    ui.add(egui::TextEdit::singleline(&mut s.folder).hint_text(tr("e.g. /home/you/Dropbox")).desired_width(320.0));
                                    #[cfg(not(target_os = "android"))]
                                    if ui.button(tr("Choose…")).clicked() { if let Some(d) = rfd::FileDialog::new().pick_folder() { s.folder = d.display().to_string(); } }
                                }),
                                sync::Via::WebDav => {
                                    ui.label(RichText::new(tr("Works with Nextcloud, ownCloud, Koofr, Box and most NAS boxes. Nextcloud: https://your-cloud/remote.php/dav/files/USERNAME/ with an app password (Settings ▸ Security).")).size(12.0).weak());
                                    egui::Grid::new("dav_grid").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                                        ui.label(tr("Server address")); ui.add(egui::TextEdit::singleline(&mut s.url).hint_text(tr("https://")).desired_width(320.0)); ui.end_row();
                                        ui.label(tr("User name")); ui.add(egui::TextEdit::singleline(&mut s.user).desired_width(320.0)); ui.end_row();
                                        ui.label(tr("Password")); ui.add(egui::TextEdit::singleline(&mut s.password).password(true).desired_width(320.0)); ui.end_row();
                                    });
                                }
                                sync::Via::Google => {
                                    if !sync::google_ready() {
                                        ui.label(RichText::new(tr("Google Drive sign-in isn't set up in this build yet.")).weak());
                                    } else if s.google_token.is_empty() {
                                        ui.label(RichText::new(tr("Your notes go to a “Leaf Notes” folder in your Google Drive. Leaf Notes can only see the files it made itself, nothing else in your Drive.")).size(12.0).weak());
                                        ui.horizontal(|ui| {
                                            match &self.st.google_rx {
                                                Some((url, _)) => {
                                                    ui.spinner();
                                                    ui.label(tr("Finish signing in in your browser…"));
                                                    ui.hyperlink_to(tr("Open the page again"), url);
                                                    if ui.button(tr("Cancel")).clicked() { g_cancel = true; }
                                                }
                                                None => {
                                                    if ui.add(egui::Button::new(RichText::new(tr("Sign in with Google")).color(Color32::WHITE)).fill(accent_base()).corner_radius(8)).clicked() { g_start = true; }
                                                    ui.hyperlink_to(RichText::new(tr("Privacy policy")).size(12.0), "https://leafnotes.briggen.dev/privacy");
                                                }
                                            }
                                        });
                                    } else {
                                        let who = if s.google_email.is_empty() { tr("your Google account").to_string() } else { s.google_email.clone() };
                                        row(ui, "Google Drive", &tf!("Signed in as {who}. Notes are in the “Leaf Notes” folder.", who = who), |ui| { if ui.button(tr("Sign out")).clicked() { g_out = true; } });
                                    }
                                }
                                sync::Via::Off => {}
                            }
                            let on = s.on();
                            // changed here: wait for "Connect" (or for the window to close) rather than trying half-typed details
                            if *s != before { self.st.sync_err.clear(); self.st.sync_pending = true; self.st.sync_at = f64::INFINITY; self.st.dirty = true; }
                            if on {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let first = self.st.sync_ok == 0;
                                    let b = if first { egui::Button::new(RichText::new(tr("Connect")).color(Color32::WHITE)).fill(accent_base()) } else { egui::Button::new(tr("Sync now")) };
                                    if ui.add_enabled(self.st.sync_rx.is_none(), b).clicked() { self.st.sync_now = true; }
                                    if self.st.sync_rx.is_some() { ui.spinner(); ui.label(tr("Syncing…")); }
                                    else if !self.st.sync_err.is_empty() { ui.label(RichText::new(format!("⚠ {}", self.st.sync_err)).color(Color32::from_rgb(220, 80, 60))); }
                                    else if self.st.sync_ok > 0 { ui.label(RichText::new(tf!("✓ Synced {x}", x = ago(self.st.sync_ok))).color(accent(ui))); }
                                });
                                ui.label(RichText::new(tf!("Sign-ins and passwords are kept in {place}. Pictures, PDFs and recordings sync too.", place = crate::keys::place())).size(11.5).weak());
                                let s = &mut self.st.sync;
                                row(ui, "Big recordings", "Recordings over this size stay on this device (they are large: about 115 MB an hour).", |ui| {
                                    egui::ComboBox::from_id_salt("rec-limit").selected_text(if s.max_recording_mb == 0 { tr("Always sync").to_string() } else { tf!("Over {n} MB stay here", n = s.max_recording_mb) }).show_ui(ui, |ui| {
                                        for n in [0u32, 25, 100, 250] { ui.selectable_value(&mut s.max_recording_mb, n, if n == 0 { tr("Always sync").to_string() } else { tf!("Over {n} MB stay here", n = n) }); }
                                    });
                                });
                            }
                            if g_start {
                                match sync::google_start() {
                                    Ok((url, sign_in)) => {
                                        ctx.open_url(egui::OpenUrl::new_tab(&url));
                                        let (tx, rx) = std::sync::mpsc::channel();
                                        std::thread::spawn(move || { let _ = tx.send(sync::google_finish(sign_in)); });
                                        self.st.google_rx = Some((url, rx));
                                        self.st.sync_err.clear();
                                    }
                                    Err(e) => self.st.sync_err = e,
                                }
                            }
                            if g_cancel { self.st.google_rx = None; }
                            if g_out {
                                let t = std::mem::take(&mut self.st.sync.google_token);
                                self.st.sync.google_email.clear();
                                std::thread::spawn(move || sync::google_revoke(&t));
                                self.st.dirty = true;
                            }
                            if self.st.sync.via == sync::Via::Google && !self.st.sync.on() && !self.st.sync_err.is_empty() {
                                ui.label(RichText::new(format!("⚠ {}", self.st.sync_err)).color(Color32::from_rgb(220, 80, 60)));
                            }
                        }
                        _ => {
                            heading(ui, "Your data");
                            let dir = data_dir();
                            row(ui, "Where your notes live", &dir.display().to_string(), |ui| {
                                let _ = &ui; // the button below is desktop-only
                                #[cfg(not(target_os = "android"))]
                                if ui.button(tr("Open folder")).clicked() { let _ = std::fs::create_dir_all(&dir); crate::canvas::open_path(&dir); }
                            });
                            row(ui, "Backup", "Save a copy of every notebook as one file (pictures and attachments stay in the folder above).", |ui| {
                                let _ = &ui;
                                #[cfg(not(target_os = "android"))]
                                if ui.button(tr("Save a backup…")).clicked() {
                                    if let Some(path) = rfd::FileDialog::new().set_file_name("leaf-notes-backup.json").save_file() { backup_to = Some(path); }
                                }
                                #[cfg(target_os = "android")]
                                if ui.button(tr("Save a backup…")).clicked() { let d = data_dir().join("exports"); let _ = std::fs::create_dir_all(&d); backup_to = Some(d.join("leaf-notes-backup.json")); }
                            });
                            row(ui, "Restore a backup", "Adds the notebooks from a backup file as extra notebooks. Nothing you have now is replaced.", |ui| {
                                let _ = &ui;
                                #[cfg(not(target_os = "android"))]
                                if ui.button(tr("Restore…")).clicked() { if let Some(f) = rfd::FileDialog::new().add_filter("Leaf Notes backup", &["json"]).pick_file() { restore = Some(f); } }
                                #[cfg(target_os = "android")]
                                if ui.button(tr("Restore…")).clicked() { let _ = crate::droid::pick(4, "application/json,*/*", false); }
                            });
                            #[cfg(not(target_os = "android"))]
                            row(ui, "Import PDFs", "Each PDF becomes a page of its own, printed on it to write on (lecture slides).", |ui| {
                                if ui.button(tr("Files…")).clicked() { if let Some(f) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_files() { pdfs = f; } }
                            });
                            row(ui, "Daily backups", "A copy of your notes each day in the backups folder (pictures and recordings stay in the files folder); the last 7 are kept.", |ui| { toggle(ui, &mut p.auto_backup); });
                            // what takes the space, roughly
                            let info = storage_summary(&data_dir());
                            row(ui, "Storage", &info, |_| {});
                            row(ui, "Import Markdown", "Each Markdown file becomes a page in this section; a folder becomes a new section.", |ui| {
                                let _ = &ui;
                                #[cfg(not(target_os = "android"))]
                                {
                                    if ui.button(tr("Folder…")).clicked() { if let Some(d) = rfd::FileDialog::new().pick_folder() { import = vec![d]; } }
                                    if ui.button(tr("Files…")).clicked() { if let Some(f) = rfd::FileDialog::new().add_filter("Markdown", &["md", "markdown", "txt"]).pick_files() { import = f; } }
                                }
                                #[cfg(target_os = "android")]
                                if ui.button(tr("Files…")).clicked() { let _ = crate::droid::pick(5, "text/*", true); }
                            });
                            row(ui, "Recently deleted", &if self.store.trash.is_empty() { tr("Nothing in here right now.").to_string() } else { tf!("{trash_n} item(s) waiting to be restored or removed.", trash_n = self.store.trash.len()) }, |ui| { if ui.button(tr("Open")).clicked() { self.st.show_trash = true; } });
                            row(ui, "Lock sections again", "Opened locked sections close again after this long without use (and always when Leaf Notes closes).", |ui| {
                                egui::ComboBox::from_id_salt("lock-after").selected_text(if p.lock_after_min == 0 { tr("Only when closing").to_string() } else { tf!("After {m} min", m = p.lock_after_min) }).show_ui(ui, |ui| {
                                    for m in [5u32, 10, 30, 60, 0] { ui.selectable_value(&mut p.lock_after_min, m, if m == 0 { tr("Only when closing").to_string() } else { tf!("After {m} min", m = m) }); }
                                });
                            });
                            row(ui, "Check for updates", "Once per start, asks GitHub whether a newer version is out (only the app's version is sent).", |ui| {
                                toggle(ui, &mut p.check_updates);
                                if self.st.update_rx.is_some() { ui.spinner(); }
                                else if ui.button(tr("Check now")).clicked() { self.st.update_rx = Some(crate::updates::check(crate::welcome::VERSION, String::new())); self.st.update_manual = true; }
                            });
                            row(ui, "Keyboard shortcuts", "Every shortcut on one card.", |ui| { if ui.button(tr("Show")).clicked() { self.st.show_shortcuts = true; } });
                            row(ui, "Privacy and terms", &tf!("Notes stay on your devices; secrets are kept in {place}. Leaf Notes is free software under the GPL-3.0.", place = crate::keys::place()), |ui| {
                                ui.hyperlink_to(tr("Terms"), "https://leafnotes.briggen.dev/terms");
                                ui.hyperlink_to(tr("Privacy policy"), "https://leafnotes.briggen.dev/privacy");
                            });
                            row(ui, "Reset settings", "Appearance, pen, writing and view settings go back to how they started. Your notes, pens, favourites, AI and sync settings stay.", |ui| {
                                let sure_id = egui::Id::unique("reset-sure");
                                if !ui.data(|d| d.get_temp::<bool>(sure_id)).unwrap_or(false) {
                                    if ui.button(tr("Reset")).clicked() { ui.data_mut(|d| d.insert_temp(sure_id, true)); }
                                } else {
                                    if ui.button(RichText::new(tr("Yes, reset")).color(Color32::from_rgb(200, 60, 50))).clicked() {
                                        // what's the person's own rather than a setting: kept
                                        let keep = Prefs { last: p.last, pens: p.pens.clone(), favorites: p.favorites.clone(), recent: p.recent.clone(), mic_ok: p.mic_ok, pen_detect: p.pen_detect, seen_version: p.seen_version.clone(), ..Prefs::default() };
                                        *p = keep;
                                        ctx.set_zoom_factor(1.0);
                                        ui.data_mut(|d| d.remove::<bool>(sure_id));
                                    }
                                    if ui.button(tr("Cancel")).clicked() { ui.data_mut(|d| d.remove::<bool>(sure_id)); }
                                }
                            });
                        }
                    }
                }));
            });
        });
        if modal.should_close() { open = false; }
        if test { self.test_ai(); }
        if let Some(f) = restore { self.st.restore_ask = Some(f); }
        if let Some(path) = backup_to {
            // locked sections stay sealed in backups too
            let r = serde_json::to_vec_pretty(&self.sealed().unwrap_or_else(|| self.store.clone())).map_err(|e| e.to_string()).and_then(|b| crate::storage::write_atomic(&path, &b).map_err(|e| e.to_string()));
            if r.is_ok() { crate::shared(&path); }
            self.st.say(match r { Ok(_) => tf!("Backup saved to {path}", path = crate::where_saved(&path)), Err(e) => tf!("⚠ Backup failed: {e}", e = e) });
        }
        if !import.is_empty() { self.import_markdown(import); }
        if !pdfs.is_empty() { self.import_pdfs(pdfs); }
        if serde_json::to_string(&self.st.p).unwrap_or_default() != before { apply_prefs(ctx, &self.st.p); self.st.dirty = true; }
        if !open {
            self.st.show_settings = false;
            self.st.dirty = true;
            if self.st.sync_at.is_infinite() { self.st.sync_at = now_f(); } // sync settings were changed without "Connect": try them now
        }
    }

    /// The first sync with a place that already has notes: combine, or take the synced notes.
    pub(crate) fn sync_ask_window(&mut self, ctx: &egui::Context) {
        let Some(pages) = self.st.sync_ask else { return };
        let mut choice = None;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
        egui::Modal::new(egui::Id::unique("sync-ask")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 460.0));
            ui.label(RichText::new(tr("Join your synced notes")).font(rich::serif(22.0)).color(ui.visuals().strong_text_color()));
            ui.add_space(6.0);
            ui.label(tf!("The sync location already has notes ({pages} pages), and this device has its own. How should this device join?", pages = pages));
            ui.add_space(12.0);
            if ui.add(egui::Button::new(RichText::new(tr("Combine both")).size(14.0).color(Color32::WHITE)).fill(accent_base()).min_size(Vec2::new(ui.available_width(), 36.0)).corner_radius(10)).tip("Every notebook from both sides, on every device").clicked() { choice = Some(Some(sync::First::Merge)); }
            if ui.add(egui::Button::new(RichText::new(tr("Use only the synced notes")).size(14.0)).min_size(Vec2::new(ui.available_width(), 36.0)).corner_radius(10)).tip("This device's current notes are saved to a backup file first").clicked() { choice = Some(Some(sync::First::UseShared)); }
            if ui.add(egui::Button::new(tr("Not now — turn sync off")).frame_when_inactive(false)).clicked() { choice = Some(None); }
        });
        let Some(c) = choice else { return };
        self.st.sync_ask = None;
        match c {
            None => { self.st.sync.via = sync::Via::Off; self.st.dirty = true; self.st.say("Sync is off."); }
            Some(first) => {
                if first == sync::First::UseShared {
                    let path = data_dir().join(format!("before-sync-{}.json", now_secs()));
                    // sealed: an opened locked section never reaches this file in plain text
                    let notes = self.sealed().unwrap_or_else(|| self.store.clone());
                    match serde_json::to_vec(&notes).map_err(|e| e.to_string()).and_then(|b| crate::storage::write_atomic(&path, &b).map_err(|e| e.to_string())) {
                        Ok(()) => self.st.say(tf!("This device's notes were saved to {path}", path = path.display())),
                        Err(e) => { self.st.say(tf!("⚠ Couldn't back up this device's notes ({e}), so nothing was replaced.", e = e)); return; }
                    }
                }
                self.start_sync(Some(first));
            }
        }
    }

}

/// "Recordings 1.1 GB · Pictures 80 MB · …" for the files folder and the notes themselves (cached a few seconds).
fn storage_summary(dir: &std::path::Path) -> String {
    thread_local! { static CACHE: std::cell::RefCell<(f64, String)> = const { std::cell::RefCell::new((-100.0, String::new())) }; }
    if let Some(s) = CACHE.with(|c| { let c = c.borrow(); (now_f() - c.0 < 5.0).then(|| c.1.clone()) }) { return s; }
    let mut sizes = [0u64; 4]; // recordings, pictures, PDFs and files, notes
    for e in std::fs::read_dir(dir.join("files")).into_iter().flatten().flatten() {
        let len = e.metadata().map(|m| m.len()).unwrap_or(0);
        let name = e.file_name().to_string_lossy().to_lowercase();
        let k = if name.ends_with(".wav") || name.ends_with(".wav.part") { 0 } else if [".png", ".jpg", ".jpeg", ".webp"].iter().any(|x| name.ends_with(x)) { 1 } else { 2 };
        sizes[k] += len;
    }
    for sub in ["pages", "history"] { for e in std::fs::read_dir(dir.join(sub)).into_iter().flatten().flatten() { sizes[3] += e.metadata().map(|m| m.len()).unwrap_or(0); } }
    let mb = |b: u64| if b >= 1 << 30 { format!("{:.1} GB", b as f64 / (1u64 << 30) as f64) } else { format!("{:.0} MB", (b as f64 / (1u64 << 20) as f64).ceil()) };
    let s = tf!("Recordings {a} · Pictures {b} · PDFs and files {c} · Notes and history {d}", a = mb(sizes[0]), b = mb(sizes[1]), c = mb(sizes[2]), d = mb(sizes[3]));
    CACHE.with(|c| *c.borrow_mut() = (now_f(), s.clone()));
    s
}
