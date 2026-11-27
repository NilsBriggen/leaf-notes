//! Navigation sidebars: notebook switcher, sections list, and pages list.

use super::{danger_color, focus_ring, format_date, nav_item, one_line, warn_color, DragPage};
use crate::state::ActiveModal;
use crate::*;
use eframe::egui::{Align, Align2, FontId, Layout, Rect, RichText, Sense};

impl NotesApp {
    pub(crate) fn nav(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_width();
        let phone = full < 560.0;
        let sec_w = if phone { 150.0 } else { 204.0 };
        egui::Panel::left("sections")
            .show_separator_line(false)
            .resizable(false)
            .exact_size(sec_w)
            .frame(egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin { left: 14, right: 10, top: 2, bottom: 12 }))
            .show(ui, |ui| {
                self.notebook_switcher(ui);
                ui.add_space(6.0);
                let jump = ui.add(
                    egui::Button::new(RichText::new(tr(if phone { "↗  Find a page" } else { "↗  Jump to a page…" })).size(13.0).color(ui.visuals().weak_text_color()))
                        .truncate()
                        .frame_when_inactive(false)
                        .min_size(Vec2::new(ui.available_width(), 30.0)),
                ).tip("Quick switcher (Ctrl+K)");
                if jump.clicked() { self.st.modals.push(ActiveModal::Switcher { query: String::new(), sel: 0 }); }
                let open_tasks = self.st.cache.open_tasks_n;
                let t = ui.add(
                    egui::Button::new(RichText::new(tf!("☑  All tasks{x}", x = if open_tasks > 0 { format!("  ·  {open_tasks}") } else { String::new() })).size(13.0).color(if self.st.nav.tasks { accent(ui) } else { ui.visuals().weak_text_color() }))
                        .truncate()
                        .frame_when_inactive(false)
                        .selected(self.st.nav.tasks)
                        .min_size(Vec2::new(ui.available_width(), 30.0)),
                ).tip("Every open to-do from every notebook");
                if t.clicked() { self.st.nav.tasks ^= true; }
                let favs: Vec<(u64, String, (usize, usize, usize))> = self.st.p.favorites.iter().filter_map(|id| self.locate(*id).map(|at| {
                    let p = &self.store.notebooks[at.0].sections[at.1].pages[at.2];
                    (p.id, if p.title.trim().is_empty() { tr("Untitled page").to_string() } else { p.title.clone() }, at)
                })).collect();
                if !favs.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr("FAVOURITES")).size(10.5).weak());
                    for (id, title, at) in favs {
                        let cur = at == (self.nb, self.sec, self.pg);
                        let r = nav_item(ui, &mut self.st, 1, id, &format!("★  {title}"), None, cur, 0, None);
                        if r.clicked() { self.go_to(at.0, at.1, at.2); }
                        r.context_menu(|ui| {
                            if ui.button(tr("Remove from favourites")).clicked() {
                                self.st.p.favorites.retain(|f| *f != id);
                                self.st.dirty = true;
                                ui.close();
                            }
                        });
                    }
                }
                let conflicts: Vec<(usize, usize, usize)> = self.store.notebooks.iter().enumerate()
                    .flat_map(|(a, n)| n.sections.iter().enumerate().flat_map(move |(b, s)| s.pages.iter().enumerate().filter(|(_, p)| p.other_of.is_some()).map(move |(c, _)| (a, b, c))))
                    .collect();
                if let Some(&(a, b, c)) = conflicts.first() {
                    if ui.add(
                        egui::Button::new(RichText::new(tf!("⚠  Sync conflicts · {n}", n = conflicts.len())).size(13.0).color(warn_color(ui)))
                            .truncate()
                            .frame_when_inactive(false)
                            .min_size(Vec2::new(ui.available_width(), 30.0)),
                    ).tip("Pages changed on two devices at once: open one to compare and keep a version").clicked() {
                        self.go_to(a, b, c);
                    }
                }
                let here = self.store.notebooks[self.nb].sections[self.sec].pages.get(self.pg).map(|p| p.id);
                let recent: Vec<(u64, String, (usize, usize, usize))> = self.st.p.recent.iter().filter(|id| Some(**id) != here && !self.st.p.favorites.contains(id)).filter_map(|id| self.locate(*id).map(|at| {
                    let p = &self.store.notebooks[at.0].sections[at.1].pages[at.2];
                    (p.id, if p.title.trim().is_empty() { tr("Untitled page").to_string() } else { p.title.clone() }, at)
                })).take(4).collect();
                if !recent.is_empty() && !self.st.nav.narrow {
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr("RECENT")).size(10.5).weak());
                    for (id, title, at) in recent {
                        if nav_item(ui, &mut self.st, 1, id, &format!("🕓  {title}"), None, false, 0, None).clicked() { self.go_to(at.0, at.1, at.2); }
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("SECTIONS")).size(10.5).weak());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("+").size(15.0).color(accent(ui))).frame_when_inactive(false)).tip("New section").clicked() { self.add_section(); }
                    });
                });
                let nb_count = self.store.notebooks.len();
                let names: Vec<(u64, String)> = self.store.notebooks.iter().map(|n| (n.id, n.name.clone())).collect();
                let nb = &mut self.store.notebooks[self.nb];
                let (mut clicked, mut recolor, mut swap, mut move_to, mut lock_act) = (None, None, None, None, None);
                let mut dup_sec: Option<usize> = None;
                let mut drop_in: Option<(u64, usize, u64)> = None;
                let count = nb.sections.len();
                egui::ScrollArea::vertical().id_salt("sections-scroll").auto_shrink([false, true]).show(ui, |ui| {
                    for (i, s) in nb.sections.iter().enumerate() {
                        let r = nav_item(ui, &mut self.st, 0, s.id, &s.name, Some(c3(s.color)), i == self.sec, 0, None);
                        if r.clicked() { clicked = Some(i); }
                        if !s.sealed() {
                            if r.dnd_hover_payload::<DragPage>().is_some_and(|x| x.0 != s.id) {
                                ui.painter().rect_stroke(r.rect, 8.0, EStroke::new(2.0, accent(ui)), egui::StrokeKind::Inside);
                            }
                            if let Some(x) = r.dnd_release_payload::<DragPage>() {
                                if x.0 != s.id { drop_in = Some((x.0, x.1, s.id)); }
                            }
                        }
                        r.context_menu(|ui| {
                            if ui.button(tr("Rename")).clicked() { self.st.nav.rename = Some((0, s.id, s.name.clone())); ui.close(); }
                            ui.horizontal(|ui| {
                                for c in PALETTE {
                                    let (rect, cr) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                                    ui.painter().rect_filled(rect, 4.0, c3(c));
                                    if cr.clicked() { recolor = Some((i, c)); ui.close(); }
                                }
                            });
                            if i > 0 && ui.button(tr("Move up")).clicked() { swap = Some((i, i - 1)); ui.close(); }
                            if i + 1 < count && ui.button(tr("Move down")).clicked() { swap = Some((i, i + 1)); ui.close(); }
                            if s.lock.is_none() && ui.button(tr("Duplicate section")).clicked() { dup_sec = Some(i); ui.close(); }
                            if nb_count > 1 {
                                ui.menu_button(tr("Move to notebook"), |ui| {
                                    for (id, name) in &names {
                                        if *id != nb.id && ui.button(name).clicked() { move_to = Some((i, *id)); ui.close(); }
                                    }
                                });
                            }
                            ui.separator();
                            match (&s.lock, self.st.locks.keys.contains_key(&s.id)) {
                                (None, _) => if ui.button(tr("🔒  Lock with a password…")).clicked() { lock_act = Some((s.id, 0)); ui.close(); },
                                (Some(_), true) => {
                                    if ui.button(tr("🔒  Lock now")).clicked() { lock_act = Some((s.id, 1)); ui.close(); }
                                    if ui.button(tr("Remove lock")).clicked() { lock_act = Some((s.id, 2)); ui.close(); }
                                }
                                (Some(_), false) => {}
                            }
                            if ui.button(RichText::new(tr("Delete section")).color(danger_color(ui))).clicked() {
                                self.st.nav.rename = None;
                                recolor = None;
                                move_to = None;
                                swap = None;
                                clicked = None;
                                self.st.nav.pending_delete = Some(s.id);
                                ui.close();
                            }
                        });
                    }
                });
                if let Some((i, c)) = recolor { nb.sections[i].color = c; self.st.dirty = true; }
                if let Some((from_sec, i, to_sec)) = drop_in {
                    let page = self.store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).find(|s| s.id == from_sec).filter(|s| i < s.pages.len()).map(|s| s.pages.remove(i));
                    if let Some(p) = page {
                        let title = p.title.clone();
                        let pid = p.id;
                        if let Some(s) = self.store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).find(|s| s.id == to_sec) {
                            if s.lock.is_some() {
                                let _ = std::fs::remove_file(data_dir().join("history").join(format!("{pid}.jsonl")));
                                let _ = std::fs::remove_file(data_dir().join("pages").join(format!("{pid}.json")));
                                self.st.index.forget(&[pid], self.st.persist);
                            }
                            s.pages.push(p);
                        }
                        self.clamp();
                        self.st.dirty = true;
                        self.st.cache.derived_dirty = true;
                        self.st.say(tf!("Moved “{title}”.", title = title));
                    }
                }
                let nb = &mut self.store.notebooks[self.nb];
                if let Some(i) = dup_sec {
                    let mut s = nb.sections[i].clone();
                    s.id = new_id();
                    s.name = tf!("{name} (copy)", name = s.name);
                    for p in &mut s.pages {
                        p.id = new_id();
                        for b in &mut p.blocks { b.id = new_id(); }
                        for k in &mut p.strokes { k.id = new_id(); }
                        for c in &mut p.cards { c.id = new_id(); }
                        for r in &mut p.recordings { r.id = new_id(); }
                    }
                    nb.sections.insert(i + 1, s);
                    self.st.dirty = true;
                    self.st.cache.derived_dirty = true;
                }
                match lock_act {
                    Some((id, 0)) => self.st.modals.push(ActiveModal::LockPrompt(locks::LockPrompt::for_section(id))),
                    Some((id, 1)) => { self.relock(id); self.st.say("Section locked."); }
                    Some((id, _)) => { self.remove_lock(id); self.st.say("The lock was removed; these pages are ordinary pages again."); }
                    None => {}
                }
                let nb = &mut self.store.notebooks[self.nb];
                if let Some((a, b)) = swap {
                    nb.sections.swap(a, b);
                    if self.sec == a { self.sec = b } else if self.sec == b { self.sec = a }
                    self.st.dirty = true;
                }
                if let Some((i, to)) = move_to {
                    let s = nb.sections.remove(i);
                    if let Some(n) = self.store.notebooks.iter_mut().find(|n| n.id == to) { n.sections.push(s); }
                    self.st.dirty = true;
                    self.st.cache.derived_dirty = true;
                    self.st.say("Section moved.");
                }
                if let Some(i) = clicked {
                    let nb = self.nb;
                    self.open_section(nb, i);
                    self.st.nav.search.clear();
                    self.st.nav.tag_filter = None;
                }
            });
        self.take_pending_delete();
        self.clamp();
        let sec_color = c3(self.store.notebooks[self.nb].sections[self.sec].color);
        let pages = egui::Panel::left("pages").show_separator_line(false);
        let pages = if phone {
            pages.resizable(false).exact_size((full - sec_w - 6.0).max(140.0))
        } else {
            pages.resizable(true).default_size(236.0).size_range(170.0..=400.0)
        };
        pages
            .frame(crate::card(ui).inner_margin(egui::Margin { left: 10, right: 10, top: 12, bottom: 10 }).outer_margin(egui::Margin { left: 0, right: 6, top: 0, bottom: 12 }))
            .show(ui, |ui| {
                if self.st.nav.tasks { self.tasks_panel(ui); return; }
                if !self.st.nav.search.trim().is_empty() || self.st.nav.tag_filter.is_some() { self.search_panel(ui); return; }
                let name = self.store.notebooks[self.nb].sections[self.sec].name.clone();
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 5.0, sec_color);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let t = ui.add(egui::Button::new("▾").frame_when_inactive(false)).tip("New page on a background, or a flashcard deck");
                        egui::Popup::menu(&t).show(|ui| {
                            for (p, icon, name) in PAPERS {
                                if ui.button(tf!("{icon}  {name}", icon = icon, name = tr(name))).clicked() { self.new_page(Some(p)); ui.close(); }
                            }
                            ui.separator();
                            for (i, (name, ..)) in crate::TEMPLATES.iter().enumerate() {
                                if ui.button(format!("▦  {}", tr(name))).clicked() { self.new_page_template(i); ui.close(); }
                            }
                            ui.separator();
                            if ui.button(tr("▤  Flashcard deck")).clicked() { self.new_deck("New deck", vec![]); ui.close(); }
                        });
                        if ui.add(egui::Button::new(RichText::new(tr("+ Page")).color(accent(ui))).frame_when_inactive(false)).tip("New page (Ctrl+N)").clicked() {
                            self.new_page(None);
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(name).font(rich::serif(18.0)).color(ui.visuals().strong_text_color())).truncate());
                        });
                    });
                });
                ui.add_space(2.0);
                let r = ui.cursor();
                ui.painter().line_segment([Pos2::new(r.left() + 2.0, r.top()), Pos2::new(r.left() + 40.0, r.top())], EStroke::new(3.0, sec_color));
                ui.painter().hline(r.left() + 44.0..=r.right(), r.top(), EStroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
                ui.add_space(6.0);
                self.page_list(ui);
            });
        self.take_pending_delete();
    }

    fn take_pending_delete(&mut self) {
        if let Some(id) = self.st.nav.pending_delete.take() {
            self.delete(id);
        }
    }

    fn notebook_switcher(&mut self, ui: &mut egui::Ui) {
        let n = &self.store.notebooks[self.nb];
        let (id, name, color) = (n.id, n.name.clone(), c3(n.color));
        if self.st.nav.rename.as_ref().is_some_and(|r| r.0 == 0 && r.1 == id) {
            nav_item(ui, &mut self.st, 0, id, &name, None, true, 0, None);
            return;
        }
        let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
        let v = ui.visuals();
        ui.painter().rect_filled(rect, 8.0, if r.hovered() { v.widgets.hovered.weak_bg_fill } else { v.extreme_bg_color });
        ui.painter().rect_stroke(rect, 8.0, v.widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
        let badge = Rect::from_min_size(rect.min + Vec2::new(8.0, 8.0), Vec2::splat(24.0));
        ui.painter().rect_filled(badge, 6.0, color);
        ui.painter().text(badge.center(), Align2::CENTER_CENTER, name.chars().next().unwrap_or('N').to_uppercase().to_string(), FontId::proportional(14.0), Color32::WHITE);
        let p = ui.painter_at(rect.shrink(2.0));
        // Ellipsize notebook title cleanly so it never overlaps the right-aligned chevron.
        let text_left = badge.right() + 8.0;
        let text_w = (rect.right() - 24.0 - text_left).max(16.0);
        let g = one_line(&p, &name, FontId::proportional(14.5), v.strong_text_color(), text_w);
        p.galley(Pos2::new(text_left, rect.center().y - g.size().y / 2.0), g, v.strong_text_color());
        p.text(Pos2::new(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, "⏷", FontId::proportional(12.0), v.weak_text_color());
        focus_ring(ui, &r, rect, 8.0);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, &name));
        if r.double_clicked() { self.st.nav.rename = Some((0, id, name.clone())); }
        let r = r.tip("Switch notebook");
        let mut pick = None;
        let mut action = 0;
        egui::Popup::menu(&r).show(|ui| {
            ui.set_min_width(200.0);
            ui.label(RichText::new(tr("NOTEBOOKS")).size(10.5).weak());
            for (i, n) in self.store.notebooks.iter().enumerate() {
                let r = nav_item(ui, &mut self.st, 3, n.id, &n.name, Some(c3(n.color)), i == self.nb, 0, None);
                if r.clicked() { pick = Some(i); ui.close(); }
            }
            ui.separator();
            if ui.button(tr("+  New notebook")).clicked() { action = 1; ui.close(); }
            if ui.button(tr("✎  Rename this notebook")).clicked() { action = 2; ui.close(); }
            if ui.button(tr("🗑  Recently deleted")).clicked() { action = 3; ui.close(); }
            ui.horizontal(|ui| {
                for c in PALETTE {
                    let (rect, cr) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                    ui.painter().rect_filled(rect, 4.0, c3(c));
                    if cr.clicked() { action = 10 + PALETTE.iter().position(|x| *x == c).unwrap_or(0); ui.close(); }
                }
            });
            if self.nb > 0 && ui.button(tr("Move up")).clicked() { action = 5; ui.close(); }
            if self.nb + 1 < self.store.notebooks.len() && ui.button(tr("Move down")).clicked() { action = 6; ui.close(); }
            if self.store.notebooks.len() > 1 && ui.button(RichText::new(tr("Delete this notebook")).color(danger_color(ui))).clicked() {
                action = 4;
                ui.close();
            }
        });
        if let Some(i) = pick { self.open_section(i, 0); }
        match action {
            1 => {
                let c = PALETTE[self.store.notebooks.len() % PALETTE.len()];
                let n = Notebook { id: new_id(), name: tr("New notebook").into(), color: c, sections: vec![] };
                self.st.nav.rename = Some((0, n.id, n.name.clone()));
                self.store.notebooks.push(n);
                self.go_to(self.store.notebooks.len() - 1, 0, 0);
            }
            2 => self.st.nav.rename = Some((0, id, name)),
            3 => self.st.modals.push(ActiveModal::Trash),
            4 => self.delete(id),
            5 | 6 => {
                let (a, b) = if action == 5 { (self.nb, self.nb - 1) } else { (self.nb, self.nb + 1) };
                self.store.notebooks.swap(a, b);
                self.nb = b;
                self.st.dirty = true;
            }
            c if c >= 10 => {
                self.store.notebooks[self.nb].color = PALETTE[(c - 10).min(PALETTE.len() - 1)];
                self.st.dirty = true;
            }
            _ => {}
        }
    }

    fn add_section(&mut self) {
        let nb = &mut self.store.notebooks[self.nb];
        let c = PALETTE[(nb.sections.len() + 1) % PALETTE.len()];
        let s = Section { id: new_id(), name: tr("New section").into(), color: c, pages: vec![], lock: None, locked_files: vec![] };
        self.st.nav.rename = Some((0, s.id, s.name.clone()));
        nb.sections.push(s);
        let (n, i) = (self.nb, nb.sections.len() - 1);
        self.go_to(n, i, 0);
    }

    fn page_list(&mut self, ui: &mut egui::Ui) {
        let targets: Vec<(u64, String)> = self.store.notebooks.iter()
            .flat_map(|n| n.sections.iter().filter(|s| !s.sealed()).map(move |s| (s.id, format!("{} › {}", n.name, s.name))))
            .collect();
        let (nb, sec) = (self.nb, self.sec);
        let sec_id = self.store.notebooks[nb].sections[sec].id;
        let mut drop_to: Option<(usize, usize)> = None;
        let mut clicked = None;
        let mut act: Option<(usize, u8)> = None;
        let mut move_to = None;
        egui::ScrollArea::vertical().id_salt("pages-scroll").auto_shrink([false; 2]).show(ui, |ui| {
            let section = &self.store.notebooks[nb].sections[sec];
            let n = section.pages.len();
            for (i, p) in section.pages.iter().enumerate() {
                let title = if p.title.trim().is_empty() { tr("Untitled page") } else { &p.title };
                let first = p.blocks.iter().filter(|b| matches!(&b.kind, Kind::Text { heading: 0, .. } | Kind::Checklist { .. })).min_by(|a, b| a.pos[1].total_cmp(&b.pos[1]));
                let preview = first.map(|b| b.kind.summary().replace(['*', '=', '$', '`'], "")).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| format_date(p.created, false));
                let preview = if p.kind == PageKind::Deck {
                    let now = now_secs();
                    let (due, new) = p.deck().iter().fold((0, 0), |(d, n), c| match self.store.study.get(&c.key) {
                        None => (d, n + 1),
                        Some(s) if s.due <= now => (d + 1, n),
                        _ => (d, n),
                    });
                    if due + new > 0 { tf!("▤ {due} due · {new} new · {n} cards", due = due, new = new, n = p.cards.len()) }
                    else { tf!("▤ Flashcards · {n} cards", n = p.cards.len()) }
                } else if p.kind == PageKind::Locked {
                    tr("Encrypted").to_string()
                } else {
                    preview
                };
                let sub = Some(if preview.chars().count() > 48 { format!("{}…", preview.chars().take(48).collect::<String>()) } else { preview });
                let r = nav_item(ui, &mut self.st, 2, p.id, title, None, i == self.pg, p.level, sub);
                if r.clicked() { clicked = Some(i); }
                if i == self.pg && self.st.nav.reveal { r.scroll_to_me(None); self.st.nav.reveal = false; }
                if self.st.canvas.input == Input::Mouse && p.kind != PageKind::Locked && self.st.nav.rename.is_none() {
                    let d = r.interact(Sense::drag());
                    d.dnd_set_drag_payload(DragPage(sec_id, i));
                    let last = |from: usize| i + 1 == n && from < i;
                    if let Some(x) = d.dnd_hover_payload::<DragPage>().filter(|x| x.0 == sec_id && x.1 != i) {
                        ui.painter().hline(r.rect.x_range(), if last(x.1) { r.rect.bottom() } else { r.rect.top() }, EStroke::new(2.0, accent(ui)));
                    }
                    if let Some(x) = d.dnd_release_payload::<DragPage>() {
                        if x.0 == sec_id && x.1 != i { drop_to = Some((x.1, if last(x.1) { n } else { i })); }
                    }
                }
                if p.kind == PageKind::Locked { continue; }
                r.context_menu(|ui| {
                    if ui.button(tr("Rename")).clicked() { self.st.nav.rename = Some((2, p.id, title.to_string())); ui.close(); }
                    if ui.button(tr("Duplicate")).clicked() { act = Some((i, 4)); ui.close(); }
                    if ui.button(tr(if p.level == 0 { "Make subpage" } else { "Promote to page" })).clicked() { act = Some((i, 1)); ui.close(); }
                    if i > 0 && ui.button(tr("Move up")).clicked() { act = Some((i, 2)); ui.close(); }
                    if i + 1 < n && ui.button(tr("Move down")).clicked() { act = Some((i, 3)); ui.close(); }
                    ui.menu_button(tr("Move to section"), |ui| {
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            for (id, label) in &targets {
                                if *id != section.id && ui.button(label).clicked() { move_to = Some((i, *id)); ui.close(); }
                            }
                        });
                    });
                    if ui.button(tr("Copy as Markdown")).clicked() { act = Some((i, 5)); ui.close(); }
                    if ui.button(tr("Copy link")).tip("Copies [[the page's name]], to paste into another page").clicked() { ui.ctx().copy_text(format!("[[{title}]]")); ui.close(); }
                    let fav = self.st.p.favorites.contains(&p.id);
                    if ui.button(tr(if fav { "Remove from favourites" } else { "Add to favourites" })).clicked() { act = Some((i, 6)); ui.close(); }
                    if ui.button(tr("Page history…")).clicked() { act = Some((i, 7)); ui.close(); }
                    ui.separator();
                    if ui.button(RichText::new(tr("Delete page")).color(danger_color(ui))).clicked() {
                        self.st.nav.pending_delete = Some(p.id);
                        ui.close();
                    }
                });
            }
        });
        let pages = &mut self.store.notebooks[nb].sections[sec].pages;
        match act {
            Some((i, 1)) => { pages[i].level = if pages[i].level == 0 { 1 } else { 0 }; self.st.dirty = true; }
            Some((i, 2)) => { pages.swap(i, i - 1); if self.pg == i { self.pg -= 1 } else if self.pg == i - 1 { self.pg += 1 } self.st.dirty = true; }
            Some((i, 3)) => { pages.swap(i, i + 1); if self.pg == i { self.pg += 1 } else if self.pg == i + 1 { self.pg -= 1 } self.st.dirty = true; }
            Some((i, 4)) => self.duplicate_page(nb, sec, i),
            Some((i, 5)) => { let md = pages[i].plain_text(); ui.ctx().copy_text(md); self.st.say("Copied as Markdown."); }
            Some((i, 6)) => {
                let id = pages[i].id;
                if self.st.p.favorites.contains(&id) { self.st.p.favorites.retain(|f| *f != id); } else { self.st.p.favorites.push(id); }
                self.st.dirty = true;
            }
            Some((i, 7)) => { self.go_to(nb, sec, i); self.show_history(); }
            _ => {}
        }
        if let Some((from, to)) = drop_to.filter(|(f, _)| *f < self.store.notebooks[nb].sections[sec].pages.len()) {
            let pages = &mut self.store.notebooks[nb].sections[sec].pages;
            let open = pages.get(self.pg).map(|p| p.id);
            crate::model::move_pages(pages, from, to);
            if let Some(id) = open {
                if let Some(k) = pages.iter().position(|p| p.id == id) { self.pg = k; }
            }
            self.st.dirty = true;
        }
        if let Some((i, to)) = move_to {
            let p = self.store.notebooks[nb].sections[sec].pages.remove(i);
            let title = p.title.clone();
            let pid = p.id;
            if let Some(s) = self.store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).find(|s| s.id == to) {
                if s.lock.is_some() {
                    let _ = std::fs::remove_file(data_dir().join("history").join(format!("{pid}.jsonl")));
                    let _ = std::fs::remove_file(data_dir().join("pages").join(format!("{pid}.json")));
                    self.st.index.forget(&[pid], self.st.persist);
                }
                s.pages.push(p);
            }
            self.st.dirty = true;
            self.st.cache.derived_dirty = true;
            self.st.say(tf!("Moved “{title}”.", title = title));
            self.clamp();
        }
        if let Some(i) = clicked { self.go_to(nb, sec, i); }
    }
}
