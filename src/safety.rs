//! Keeping notes safe: page history, restoring a backup, importing Markdown, and choosing between
//! the two versions of a page that was changed on two devices at once.
use crate::chrome::warn_color;
use crate::state::ActiveModal;
use crate::*;
use eframe::egui::{Align, Layout, RichText, ScrollArea};
use std::path::PathBuf;

impl NotesApp {
    /// Open the history of the page on screen.
    pub(crate) fn show_history(&mut self) {
        let id = self.page().id;
        let versions = if self.st.persist { storage::history(&data_dir(), id) } else { vec![] };
        self.st.modals.push(ActiveModal::History { page: id, versions, sel: 0 });
    }

    pub(crate) fn history_window(&mut self, ctx: &egui::Context, id: u64, versions: &[(u64, Page)], sel: &mut usize) -> bool {
        let (mut close, mut restore, mut as_copy) = (false, None, None);
        let narrow = ctx.content_rect().width() < 600.0;
        let max_modal_h = (ctx.content_rect().height() - 60.0 - self.st.bars[0] - self.st.bars[1]).max(180.0);
        let max_h = (max_modal_h - 60.0).clamp(130.0, 520.0);
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(if narrow { 14 } else { 20 })).corner_radius(18);
        let modal = egui::Modal::new(egui::Id::unique("history")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ctx, 780.0));
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            ui.spacing_mut().scroll.bar_width = 5.0;
            ScrollArea::vertical().id_salt("history-outer").max_height(max_modal_h).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Page history")).font(rich::serif(22.0)).color(ui.visuals().strong_text_color()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("✕").size(15.0)).frame_when_inactive(false)).tip("Close (Esc)").clicked() { close = true; }
                    });
                });
                ui.add_space(6.0);
                if versions.is_empty() {
                    ui.label(RichText::new(tr("No older versions yet. Leaf Notes keeps the previous version each time a page changes, at most every 10 minutes.")).weak());
                    return;
                }
                *sel = (*sel).min(versions.len() - 1);
                let render_list = |ui: &mut egui::Ui, sel: &mut usize, h: f32| {
                    ScrollArea::vertical().id_salt("history-list").max_height(h).show(ui, |ui| {
                        for (i, (t, p)) in versions.iter().enumerate() {
                            let words = p.plain_text().split_whitespace().count();
                            let text = tf!("{when}\n{words} words · {strokes} ink strokes", when = chrome::format_date(*t, true), words = words, strokes = p.strokes.len());
                            if ui.selectable_label(i == *sel, text).clicked() { *sel = i; }
                        }
                    });
                };
                let render_preview = |ui: &mut egui::Ui, sel: usize, h: f32, restore: &mut Option<usize>, as_copy: &mut Option<usize>| {
                    let p = &versions[sel].1;
                    ui.label(RichText::new(if p.title.is_empty() { tr("Untitled page") } else { &p.title }).size(17.0).strong());
                    ScrollArea::vertical().id_salt("history-preview").max_height(h).show(ui, |ui| {
                        if !p.strokes.is_empty() {
                            let (lo, hi) = p.strokes.iter().flat_map(|s| s.points.iter()).fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), q| ([lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])]));
                            let w = ui.available_width().clamp(80.0, 480.0);
                            let k = (w / (hi[0] - lo[0] + 20.0)).min(1.0);
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, ((hi[1] - lo[1] + 20.0) * k).clamp(32.0, 320.0)), egui::Sense::hover());
                            let painter = ui.painter_at(rect);
                            let dark = ui.visuals().dark_mode;
                            painter.rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
                            for s in &p.strokes {
                                let pts: Vec<Pos2> = s.points.iter().map(|q| rect.min + Vec2::new((q[0] - lo[0] + 10.0) * k, (q[1] - lo[1] + 10.0) * k)).collect();
                                // Chalk ink in dark mode so dark pen strokes remain legible in history preview.
                                let col = if dark && !s.highlighter && s.color[0].max(s.color[1]).max(s.color[2]) < 90 {
                                    Color32::from_rgba_unmultiplied(230, 234, 232, s.color[3])
                                } else {
                                    Color32::from_rgba_unmultiplied(s.color[0], s.color[1], s.color[2], s.color[3])
                                };
                                painter.add(egui::Shape::line(pts, EStroke::new((s.width * k).max(1.0), col)));
                            }
                        }
                        ui.label(p.plain_text());
                        ui.add_space(6.0);
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(tr("↺  Restore this version")).clicked() { *restore = Some(sel); }
                        if ui.button(tr("Restore as a new page")).clicked() { *as_copy = Some(sel); }
                    });
                    ui.label(RichText::new(tr("The page as it is now goes to Undo, so you can switch back.")).size(11.5).weak());
                };
                if narrow {
                    render_list(ui, sel, (max_h * 0.35).max(80.0));
                    ui.separator();
                    render_preview(ui, *sel, (max_h * 0.45).max(100.0), &mut restore, &mut as_copy);
                } else {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(240.0);
                            render_list(ui, sel, (max_h - 40.0).max(120.0));
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            render_preview(ui, *sel, (max_h - 120.0).max(110.0), &mut restore, &mut as_copy);
                        });
                    });
                }
            });
        });
        if let Some(i) = as_copy {
            if let Some((a, b, c)) = self.locate(id) {
                let mut p = versions[i].1.clone();
                p.id = new_id();
                p.other_of = None;
                p.title = tf!("{title} (from {when})", title = p.title, when = chrome::format_date(versions[i].0, false));
                for b in &mut p.blocks { b.id = new_id(); }
                for s in &mut p.strokes { s.id = new_id(); }
                for k in &mut p.cards { k.id = new_id(); }
                for r in &mut p.recordings { r.id = new_id(); }
                self.store.notebooks[a].sections[b].pages.insert(c + 1, p);
                self.go_to(a, b, c + 1);
                self.st.say("The old version is now a page of its own, next to the current one.");
            }
            return false;
        }
        if let Some(i) = restore {
            let old = versions[i].1.clone();
            if let Some(cur) = self.find_page(id).cloned() {
                self.st.snap(&cur);
                if let Some(p) = self.find_page(id) { *p = Page { id, other_of: cur.other_of, ..old }; }
                self.st.dirty = true;
                self.st.say(tf!("Restored the version from {when}.", when = chrome::format_date(versions[i].0, true)));
            }
            return false;
        }
        !(close || modal.should_close())
    }

    /// What's in the backup the person picked, and whether to add it or replace everything with it.
    pub(crate) fn restore_window(&mut self, ctx: &egui::Context, path: PathBuf, mut parsed: Option<Store>) -> Option<(PathBuf, Option<Store>)> {
        if parsed.is_none() {
            match std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Store>(&b).ok()) {
                Some(b) => parsed = Some(b),
                None => { self.restore_backup(&path); return None; }
            }
        }
        let backup = parsed.as_ref().unwrap();
        let (notebooks, pages): (usize, usize) = (backup.notebooks.len(), backup.notebooks.iter().flat_map(|n| &n.sections).map(|s| s.pages.len()).sum());
        let mut choice = None;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
        let m = egui::Modal::new(egui::Id::unique("restore-ask")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 440.0));
            ui.label(RichText::new(tr("Restore a backup")).font(rich::serif(20.0)));
            ui.label(tf!("This backup has {n} notebook(s) with {p} page(s).", n = notebooks, p = pages));
            if self.st.sync_st.settings.on() {
                ui.label(RichText::new(tr("Sync is on: replacing everything also replaces the notes on your other devices.")).color(warn_color(ui)));
            }
            ui.add_space(8.0);
            if ui.button(tr("Add them as extra notebooks")).clicked() { choice = Some(1); }
            if ui.button(tr("Replace everything with the backup")).tip("What's here now is saved to a file in the notes folder first").clicked() { choice = Some(2); }
            if ui.button(tr("Cancel")).clicked() { choice = Some(0); }
        });
        if m.should_close() { choice = Some(0); }
        match choice {
            Some(1) => { self.restore_backup(&path); None }
            Some(2) => {
                let backup = parsed.take()?;
                let keep = data_dir().join(format!("before-restore-{}.json", now_secs()));
                let now = self.sealed().unwrap_or_else(|| self.store.clone());
                match serde_json::to_vec(&now).map_err(|e| e.to_string()).and_then(|b| crate::storage::write_atomic(&keep, &b).map_err(|e| e.to_string())) {
                    Err(e) => self.st.say(tf!("⚠ Couldn't save the current notes first ({e}), so nothing was replaced.", e = e)),
                    Ok(()) => {
                        self.store = backup;
                        self.st.locks.keys.clear();
                        self.st.undo.clear();
                        self.st.redo.clear();
                        crate::sync::dedupe(&mut self.store);
                        crate::model::rekey_study(&mut self.store);
                        if self.store.notebooks.is_empty() { self.store = sample(); }
                        self.clamp();
                        self.st.full_save = true;
                        self.st.dirty = true;
                        self.st.cache.derived_dirty = true;
                        self.st.say(tf!("Restored the backup. The notes from before are in {path}", path = keep.display()));
                    }
                }
                None
            }
            Some(_) => None,
            None => Some((path, parsed)),
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
        crate::sync::dedupe(&mut self.store);
        self.store.trash.extend(backup.trash);
        for (k, v) in backup.study { self.store.study.entry(k).or_insert(v); }
        crate::model::rekey_study(&mut self.store);
        self.st.dirty = true;
        self.st.cache.derived_dirty = true;
        self.st.say(tf!("Restored {n} notebook(s) from the backup.", n = added));
    }

    /// Markdown files become pages in this section; a folder becomes a new section of its .md files.
    pub(crate) fn import_markdown(&mut self, paths: Vec<std::path::PathBuf>) {
        let width = self.st.p.note_width;
        let read = |p: &std::path::Path| -> Option<Page> {
            if std::fs::metadata(p).ok()?.len() > 5_000_000 { return None; }
            let md = std::fs::read_to_string(p).ok()?;
            let title = p.file_stem()?.to_string_lossy().to_string();
            Some(page_from_markdown(&title, &md, width))
        };
        let is_md = |p: &std::path::Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown") || e.eq_ignore_ascii_case("txt"));
        let (mut pages, mut sections) = (0, 0);
        let mut target_at: Option<(usize, usize, usize)> = None;
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
                let sec_idx = self.store.notebooks[self.nb].sections.len();
                self.store.notebooks[self.nb].sections.push(Section { id: new_id(), name, color, pages: new, lock: None, locked_files: vec![] });
                if target_at.is_none() { target_at = Some((self.nb, sec_idx, 0)); }
            } else if let Some(p) = read(&path).filter(|_| !self.section().sealed()) {
                let pg_idx = self.section().pages.len();
                self.section().pages.push(p);
                target_at = Some((self.nb, self.sec, pg_idx));
                pages += 1;
            }
        }
        if let Some((a, b, c)) = target_at {
            self.go_to(a, b, c);
        }
        self.st.dirty = true;
        self.st.cache.derived_dirty = true;
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
        let dark = ui.visuals().dark_mode;
        let bg = if dark { Color32::from_rgb(72, 52, 18) } else { Color32::from_rgb(254, 243, 199) };
        let fg = if dark { Color32::from_rgb(254, 226, 150) } else { Color32::from_rgb(120, 70, 0) };
        egui::Frame::new().fill(bg).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tr("⚠ This page was changed on two devices at the same time, so both versions were kept.")).color(fg));
                go = ui.button(tr("Compare side by side")).clicked();
            });
        });
        ui.add_space(6.0);
        go
    }

    pub(crate) fn compare_window(&mut self, ctx: &egui::Context, orig: u64, copy: u64) -> bool {
        let get = |id: u64| self.store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).find(|p| p.id == id).cloned();
        let (Some(a), Some(b)) = (get(orig), get(copy)) else { return false };
        let (mut close, mut choice) = (false, None);
        let narrow = ctx.content_rect().width() < 620.0;
        let max_modal_h = (ctx.content_rect().height() - 60.0 - self.st.bars[0] - self.st.bars[1]).max(180.0);
        let max_h = (max_modal_h - 140.0).clamp(90.0, 380.0);
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(if narrow { 14 } else { 20 })).corner_radius(18);
        let modal = egui::Modal::new(egui::Id::unique("compare")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ctx, 900.0));
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            ui.spacing_mut().scroll.bar_width = 5.0;
            ScrollArea::vertical().id_salt("compare-outer").max_height(max_modal_h).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Two versions of this page")).font(rich::serif(22.0)).color(ui.visuals().strong_text_color()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("✕").size(15.0)).frame_when_inactive(false)).tip("Close (Esc)").clicked() { close = true; }
                    });
                });
                ui.add_space(6.0);
                let mut render_col = |ui: &mut egui::Ui, k: usize, p: &Page, label: &str, h: f32| {
                    ui.label(RichText::new(label).size(12.0).weak());
                    ui.label(RichText::new(&p.title).size(16.0).strong());
                    ui.label(RichText::new(tf!("{words} words · {strokes} ink strokes", words = p.plain_text().split_whitespace().count(), strokes = p.strokes.len())).size(11.5).weak());
                    ScrollArea::vertical().id_salt(("compare", k)).max_height(h).show(ui, |ui| { ui.label(p.plain_text()); });
                    if ui.button(tr("Keep only this version")).clicked() { choice = Some(k); }
                };
                if narrow {
                    render_col(ui, 0, &a, tr("This device"), (max_h * 0.45).max(80.0));
                    ui.separator();
                    render_col(ui, 1, &b, tr("The other device"), (max_h * 0.45).max(80.0));
                } else {
                    ui.columns(2, |cols| {
                        render_col(&mut cols[0], 0, &a, tr("This device"), max_h);
                        render_col(&mut cols[1], 1, &b, tr("The other device"), max_h);
                    });
                }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.button(tr("Keep both as separate pages")).clicked() { choice = Some(2); }
                    ui.label(RichText::new(tr("The version you don't keep goes to Recently deleted.")).size(11.5).weak());
                });
            });
        });
        match choice {
            Some(0) => { self.delete(copy); false }
            Some(1) => {
                if let Some(cur) = self.find_page(orig).cloned() {
                    self.st.snap(&cur);
                    // Preserve remote version's title (stripping conflict suffix) and outline level.
                    let clean_title = b.title
                        .trim_end_matches(" (other device)")
                        .trim_end_matches(" (anderes Gerät)")
                        .trim_end_matches(" (autre appareil)")
                        .to_string();
                    if let Some(p) = self.find_page(orig) {
                        *p = Page { id: orig, title: clean_title, level: b.level, other_of: None, ..b.clone() };
                    }
                }
                self.delete(copy);
                if let Some((x, y, z)) = self.locate(orig) { self.go_to(x, y, z); }
                false
            }
            Some(_) => {
                if let Some(p) = self.find_page(copy) { p.other_of = None; }
                self.st.dirty = true;
                self.st.cache.derived_dirty = true;
                false
            }
            None => !(close || modal.should_close()),
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
        std::fs::write(dir.join("backup.json"), serde_json::to_vec(&app.store).unwrap()).unwrap();
        app.restore_backup(&dir.join("backup.json"));
        assert_eq!(app.store.notebooks.len(), before * 2);
        let ids: Vec<u64> = app.store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).map(|p| p.id).collect();
        assert_eq!(ids.len(), ids.iter().collect::<std::collections::HashSet<_>>().len(), "no duplicate page ids");
        assert!(app.store.notebooks[before].name.contains("restored"));
        std::fs::write(dir.join("junk.json"), b"[1,2]").unwrap();
        app.restore_backup(&dir.join("junk.json"));
        assert_eq!(app.store.notebooks.len(), before * 2);
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
