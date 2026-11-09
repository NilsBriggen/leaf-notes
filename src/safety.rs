//! Keeping notes safe: page history, restoring a backup, importing Markdown, and choosing between
//! the two versions of a page that was changed on two devices at once.
use crate::*;
use eframe::egui::{RichText, ScrollArea};

impl NotesApp {
    /// Open the history of the page on screen.
    pub(crate) fn show_history(&mut self) {
        let id = self.page().id;
        let versions = if self.st.persist { storage::history(&data_dir(), id) } else { vec![] };
        self.st.history = Some((id, versions, 0));
    }

    pub(crate) fn history_window(&mut self, ctx: &egui::Context) {
        let Some((id, versions, mut sel)) = self.st.history.take() else { return };
        let (mut open, mut restore) = (true, None);
        egui::Window::new(tr("Page history")).open(&mut open).collapsible(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).default_size([crate::fit_w(ctx, 780.0), 520.0f32.min(ctx.content_rect().height() - 60.0)]).max_width(crate::fit_w(ctx, 2000.0)).show(ctx, |ui| {
            if versions.is_empty() {
                ui.label(RichText::new(tr("No older versions yet. Leaf Notes keeps the previous version each time a page changes, at most every 10 minutes.")).weak());
                return;
            }
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(240.0);
                    ScrollArea::vertical().id_salt("history-list").max_height(440.0).show(ui, |ui| {
                        for (i, (t, p)) in versions.iter().enumerate() {
                            let words = p.plain_text().split_whitespace().count();
                            let text = tf!("{when}\n{words} words · {strokes} ink strokes", when = chrome::format_date(*t, true), words = words, strokes = p.strokes.len());
                            if ui.selectable_label(i == sel, text).clicked() { sel = i; }
                        }
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    let p = &versions[sel].1;
                    ui.label(RichText::new(if p.title.is_empty() { tr("Untitled page") } else { &p.title }).size(17.0).strong());
                    ScrollArea::vertical().id_salt("history-preview").max_height(380.0).show(ui, |ui| { ui.label(p.plain_text()); });
                    ui.add_space(6.0);
                    if ui.button(tr("↺  Restore this version")).clicked() { restore = Some(sel); }
                    ui.label(RichText::new(tr("The page as it is now goes to Undo, so you can switch back.")).size(11.5).weak());
                });
            });
        });
        if let Some(i) = restore {
            let old = versions[i].1.clone();
            if let Some(cur) = self.find_page(id).cloned() {
                self.st.snap(&cur);
                if let Some(p) = self.find_page(id) { *p = Page { id, other_of: cur.other_of, ..old }; }
                self.st.dirty = true;
                self.st.say(tf!("Restored the version from {when}.", when = chrome::format_date(versions[i].0, true)));
            }
            return;
        }
        if open { self.st.history = Some((id, versions, sel)); }
    }

    /// What's in the backup the person picked, and whether to add it or replace everything with it.
    pub(crate) fn restore_window(&mut self, ctx: &egui::Context) {
        let Some(path) = self.st.restore_ask.clone() else { return };
        let parsed = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Store>(&b).ok());
        let Some(backup) = parsed else { self.st.restore_ask = None; self.restore_backup(&path); return }; // says what's wrong
        let pages: usize = backup.notebooks.iter().flat_map(|n| &n.sections).map(|s| s.pages.len()).sum();
        let mut choice = None;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
        let m = egui::Modal::new(egui::Id::unique("restore-ask")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 440.0));
            ui.label(RichText::new(tr("Restore a backup")).font(rich::serif(20.0)));
            ui.label(tf!("This backup has {n} notebook(s) with {p} page(s).", n = backup.notebooks.len(), p = pages));
            ui.add_space(8.0);
            if ui.button(tr("Add them as extra notebooks")).clicked() { choice = Some(1); }
            if ui.button(tr("Replace everything with the backup")).tip("What's here now is saved to a file in the notes folder first").clicked() { choice = Some(2); }
            if ui.button(tr("Cancel")).clicked() { choice = Some(0); }
        });
        if m.should_close() { choice = Some(0); }
        match choice {
            Some(1) => { self.st.restore_ask = None; self.restore_backup(&path); }
            Some(2) => {
                self.st.restore_ask = None;
                // the notes as they are now, sealed, kept before anything is replaced
                let keep = data_dir().join(format!("before-restore-{}.json", now_secs()));
                let now = self.sealed().unwrap_or_else(|| self.store.clone());
                match serde_json::to_vec(&now).map_err(|e| e.to_string()).and_then(|b| crate::storage::write_atomic(&keep, &b).map_err(|e| e.to_string())) {
                    Err(e) => self.st.say(tf!("⚠ Couldn't save the current notes first ({e}), so nothing was replaced.", e = e)),
                    Ok(()) => {
                        self.store = backup;
                        self.st.keys.clear();
                        self.st.undo.clear();
                        self.st.redo.clear();
                        crate::sync::dedupe(&mut self.store);
                        if self.store.notebooks.is_empty() { self.store = sample(); }
                        self.clamp();
                        self.st.full_save = true;
                        self.st.dirty = true;
                        self.st.say(tf!("Restored the backup. The notes from before are in {path}", path = keep.display()));
                    }
                }
            }
            Some(_) => self.st.restore_ask = None,
            None => {}
        }
    }

    /// Add the notebooks of a backup file as extra notebooks; nothing that's there now is replaced.
    pub(crate) fn restore_backup(&mut self, path: &std::path::Path) {
        let parsed = std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<Store>(&b).map_err(|e| e.to_string()));
        let backup = match parsed { Ok(s) => s, Err(e) => { self.st.say(tf!("⚠ That file isn't a Leaf Notes backup ({e}).", e = e)); return; } };
        let names: Vec<String> = self.store.notebooks.iter().map(|n| n.name.clone()).collect();
        let mut added = 0;
        for mut n in backup.notebooks {
            if names.contains(&n.name) { n.name = tf!("{name} (restored)", name = n.name); }
            self.store.notebooks.push(n);
            added += 1;
        }
        // the backup's own copies get fresh ids (what's here keeps its own), and its Recently deleted comes along
        crate::sync::dedupe(&mut self.store);
        self.store.trash.extend(backup.trash);
        for (k, v) in backup.study { self.store.study.entry(k).or_insert(v); }
        self.st.dirty = true;
        self.st.say(tf!("Restored {n} notebook(s) from the backup.", n = added));
    }

    /// Markdown files become pages in this section; a folder becomes a new section of its .md files.
    pub(crate) fn import_markdown(&mut self, paths: Vec<std::path::PathBuf>) {
        let width = self.st.p.note_width;
        let read = |p: &std::path::Path| -> Option<Page> {
            // a notes file, not a book: huge text would stall the app
            if std::fs::metadata(p).ok()?.len() > 5_000_000 { return None; }
            let md = std::fs::read_to_string(p).ok()?;
            let title = p.file_stem()?.to_string_lossy().to_string();
            Some(page_from_markdown(&title, &md, width))
        };
        let is_md = |p: &std::path::Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown") || e.eq_ignore_ascii_case("txt"));
        let (mut pages, mut sections) = (0, 0);
        for path in paths {
            if path.is_dir() {
                let mut files: Vec<_> = std::fs::read_dir(&path).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| is_md(p)).collect();
                files.sort();
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let new: Vec<Page> = files.iter().filter_map(|f| read(f)).collect();
                if new.is_empty() { continue; }
                pages += new.len();
                sections += 1;
                let color = PALETTE[(self.store.notebooks[self.nb].sections.len()) % PALETTE.len()];
                self.store.notebooks[self.nb].sections.push(Section { id: new_id(), name, color, pages: new, lock: None });
            } else if let Some(p) = read(&path).filter(|_| !self.section().sealed()) {
                self.section().pages.push(p);
                pages += 1;
            }
        }
        self.st.dirty = true;
        self.st.say(if pages == 0 { tr("No Markdown files found there.").to_string() } else { tf!("Imported {pages} page(s) ({sections} new section(s)).", pages = pages, sections = sections) });
    }

    /// For the page on screen: (this device's page, the other device's copy) if it has a sync conflict.
    pub(crate) fn conflict_pair(&self) -> Option<(u64, u64)> {
        let all = || self.store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages);
        let page = &self.store.notebooks.get(self.nb)?.sections.get(self.sec)?.pages.get(self.pg)?;
        if let Some(orig) = page.other_of.filter(|o| all().any(|p| p.id == *o)) { return Some((orig, page.id)); }
        all().find(|p| p.other_of == Some(page.id)).map(|c| (page.id, c.id))
    }

    /// A note above the page; true when "Compare" was pressed.
    pub(crate) fn conflict_banner(ui: &mut egui::Ui) -> bool {
        let mut go = false;
        egui::Frame::new().fill(Color32::from_rgb(254, 243, 199)).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("⚠ This page was changed on two devices at the same time, so both versions were kept.")).color(Color32::from_rgb(120, 70, 0)));
                go = ui.button(tr("Compare side by side")).clicked();
            });
        });
        ui.add_space(6.0);
        go
    }

    pub(crate) fn compare_window(&mut self, ctx: &egui::Context) {
        let Some((orig, copy)) = self.st.compare else { return };
        let get = |id: u64| self.store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).find(|p| p.id == id).cloned();
        let (Some(a), Some(b)) = (get(orig), get(copy)) else { self.st.compare = None; return };
        let (mut open, mut choice) = (true, None);
        egui::Window::new(tr("Two versions of this page")).open(&mut open).collapsible(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).default_size([crate::fit_w(ctx, 900.0), 560.0f32.min(ctx.content_rect().height() - 60.0)]).max_width(crate::fit_w(ctx, 2000.0)).show(ctx, |ui| {
            ui.columns(2, |cols| {
                for (k, (p, label)) in [(&a, tr("This device")), (&b, tr("The other device"))].into_iter().enumerate() {
                    let ui = &mut cols[k];
                    ui.label(RichText::new(label).size(12.0).weak());
                    ui.label(RichText::new(&p.title).size(16.0).strong());
                    ui.label(RichText::new(tf!("{words} words · {strokes} ink strokes", words = p.plain_text().split_whitespace().count(), strokes = p.strokes.len())).size(11.5).weak());
                    ScrollArea::vertical().id_salt(("compare", k)).max_height(360.0).show(ui, |ui| { ui.label(p.plain_text()); });
                    if ui.button(tr("Keep only this version")).clicked() { choice = Some(k); }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(tr("Keep both as separate pages")).clicked() { choice = Some(2); }
                ui.label(RichText::new(tr("The version you don't keep goes to Recently deleted.")).size(11.5).weak());
            });
        });
        match choice {
            Some(0) => { self.delete(copy); self.st.compare = None; }
            Some(1) => {
                if let Some(cur) = self.find_page(orig).cloned() {
                    self.st.snap(&cur);
                    if let Some(p) = self.find_page(orig) { *p = Page { id: orig, title: cur.title.clone(), level: cur.level, other_of: None, ..b.clone() }; }
                }
                self.delete(copy);
                if let Some((x, y, z)) = self.locate(orig) { self.go_to(x, y, z); }
                self.st.compare = None;
            }
            Some(_) => { if let Some(p) = self.find_page(copy) { p.other_of = None; } self.st.dirty = true; self.st.compare = None; }
            None => if !open { self.st.compare = None; },
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The real app, without a window and without touching saved notes.
    pub(crate) fn app() -> NotesApp {
        let ctx = egui::Context::default();
        NotesApp::new(&eframe::CreationContext::_new_kittest(ctx), Some(std::env::temp_dir().join("ln-test.png")))
    }

    #[test]
    fn restores_backups_and_imports_markdown() {
        let dir = std::env::temp_dir().join(format!("ln-safety-{}", new_id()));
        std::fs::create_dir_all(dir.join("lectures")).unwrap();
        let mut app = app();
        let before = app.store.notebooks.len();
        // a backup of the same notes: added alongside, with fresh ids and "(restored)" names
        std::fs::write(dir.join("backup.json"), serde_json::to_vec(&app.store).unwrap()).unwrap();
        app.restore_backup(&dir.join("backup.json"));
        assert_eq!(app.store.notebooks.len(), before * 2);
        let ids: Vec<u64> = app.store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).map(|p| p.id).collect();
        assert_eq!(ids.len(), ids.iter().collect::<std::collections::HashSet<_>>().len(), "no duplicate page ids");
        assert!(app.store.notebooks[before].name.contains("restored"));
        // not a backup: nothing changes
        std::fs::write(dir.join("junk.json"), b"[1,2]").unwrap();
        app.restore_backup(&dir.join("junk.json"));
        assert_eq!(app.store.notebooks.len(), before * 2);
        // Markdown: a folder becomes a section, a file a page
        std::fs::write(dir.join("lectures/01 Intro.md"), "# Intro\n\nSome **notes**.\n\n- [ ] read").unwrap();
        std::fs::write(dir.join("lectures/02 Next.md"), "More").unwrap();
        std::fs::write(dir.join("single.md"), "Hello").unwrap();
        let sections = app.store.notebooks[app.nb].sections.len();
        app.import_markdown(vec![dir.join("lectures"), dir.join("single.md")]);
        let nb = &app.store.notebooks[app.nb];
        assert_eq!(nb.sections.len(), sections + 1);
        let new = nb.sections.last().unwrap();
        assert_eq!((new.name.as_str(), new.pages.len(), new.pages[0].title.as_str()), ("lectures", 2, "01 Intro"));
        assert!(new.pages[0].plain_text().contains("read"));
        assert!(nb.sections[app.sec].pages.iter().any(|p| p.title == "single"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
