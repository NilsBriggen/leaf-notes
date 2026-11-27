//! Unified modal stack dispatcher and dialogs (switcher, study, mic, ink review, trash, shortcuts, prompts).

use super::{danger_color, format_date, one_line, shuffle, Study, StudyUndo, SwitchHit, COMMANDS};
use crate::state::ActiveModal;
use crate::*;
use eframe::egui::{Align, Align2, FontId, Layout, Rect, RichText, Sense};

impl NotesApp {
    /// Cards due again, and up to 20 never studied.
    pub(crate) fn due_cards(&self) -> Vec<Card> {
        let now = now_secs();
        let all = self.store.notebooks.iter().flat_map(|n| n.sections.iter()).flat_map(|s| s.pages.iter()).flat_map(|p| p.deck());
        let (mut due, mut fresh) = (vec![], vec![]);
        for c in all {
            match self.store.study.get(&c.key) {
                Some(s) if s.due <= now => due.push(c),
                None if fresh.len() < 20 => fresh.push(c),
                _ => {}
            }
        }
        due.extend(fresh);
        due
    }

    pub(crate) fn start_review(&mut self) {
        let cards = self.due_cards();
        if cards.is_empty() {
            self.st.say("Nothing due today — nice! Cards come back here once you've studied them.");
            return;
        }
        let total = cards.len();
        let mut q: Vec<usize> = (0..total).collect();
        shuffle(&mut q);
        self.st.modals.push(ActiveModal::Study(Study {
            title: tr("Due today").into(),
            queue: q,
            cards,
            total,
            shown: false,
            tally: [0; 3],
            missed: vec![],
            undo: None,
        }));
    }

    /// Study the open deck: cards that are due or new first, then the rest.
    pub(crate) fn start_deck(&mut self) {
        let p = &self.store.notebooks[self.nb].sections[self.sec].pages[self.pg];
        let cards = p.deck();
        if cards.is_empty() {
            self.st.say("Add a card with both a front and a back first.");
            return;
        }
        let title = if p.title.is_empty() { tr("Untitled deck").into() } else { p.title.clone() };
        let now = now_secs();
        let (mut due, mut later): (Vec<usize>, Vec<usize>) = (0..cards.len()).partition(|&i| self.store.study.get(&cards[i].key).is_none_or(|s| s.due <= now));
        shuffle(&mut due);
        shuffle(&mut later);
        due.append(&mut later);
        let total = due.len();
        self.st.modals.push(ActiveModal::Study(Study {
            title,
            cards,
            queue: due,
            total,
            shown: false,
            tally: [0; 3],
            missed: vec![],
            undo: None,
        }));
    }

    /// Render the topmost modal on the unified modal stack.
    pub(crate) fn dialogs(&mut self, ctx: &egui::Context) {
        if !self.st.modals.has_switcher() {
            self.st.cache.switch_cache = None;
        }
        let Some(top) = self.st.modals.pop() else { return };
        match top {
            ActiveModal::Settings { mut tab } => {
                let keep = self.settings_window(ctx, &mut tab);
                // Push back underneath any sub-modal that Settings may have opened this frame.
                if keep {
                    let opened_child = self.st.modals.pop();
                    self.st.modals.push(ActiveModal::Settings { tab });
                    if let Some(child) = opened_child {
                        self.st.modals.push(child);
                    }
                }
            }
            ActiveModal::SyncAsk { pages } => {
                if self.sync_ask_window(ctx, pages) {
                    self.st.modals.push(ActiveModal::SyncAsk { pages });
                }
            }
            ActiveModal::History { page, versions, mut sel } => {
                if self.history_window(ctx, page, &versions, &mut sel) {
                    self.st.modals.push(ActiveModal::History { page, versions, sel });
                }
            }
            ActiveModal::Compare { orig, copy } => {
                if self.compare_window(ctx, orig, copy) {
                    self.st.modals.push(ActiveModal::Compare { orig, copy });
                }
            }
            ActiveModal::LockPrompt(lp) => {
                if let Some(lp) = self.lock_window(ctx, lp) {
                    self.st.modals.push(ActiveModal::LockPrompt(lp));
                }
            }
            ActiveModal::Tour { step, fresh } => {
                if let Some((next_step, next_fresh)) = self.tour_window(ctx, step, fresh) {
                    self.st.modals.push(ActiveModal::Tour { step: next_step, fresh: next_fresh });
                }
            }
            ActiveModal::News => {
                if self.news_window(ctx) {
                    self.st.modals.push(ActiveModal::News);
                }
            }
            ActiveModal::Help => {
                if self.help_window(ctx) {
                    let opened_child = self.st.modals.pop();
                    self.st.modals.push(ActiveModal::Help);
                    if let Some(child) = opened_child {
                        self.st.modals.push(child);
                    }
                }
            }
            ActiveModal::Crash { path, text } => {
                if self.crash_window(ctx, &path, &text) {
                    self.st.modals.push(ActiveModal::Crash { path, text });
                }
            }
            ActiveModal::Switcher { query, sel } => {
                if let Some((q, s)) = self.switcher_window(ctx, query, sel) {
                    self.st.modals.push(ActiveModal::Switcher { query: q, sel: s });
                }
            }
            ActiveModal::Study(sd) => {
                if let Some(sd) = self.study_window(ctx, sd) {
                    self.st.modals.push(ActiveModal::Study(sd));
                }
            }
            ActiveModal::MicPrompt => {
                if self.mic_window(ctx) {
                    self.st.modals.push(ActiveModal::MicPrompt);
                }
            }
            ActiveModal::RestoreAsk { path, parsed } => {
                if let Some((p, parsed)) = self.restore_window(ctx, path, parsed) {
                    self.st.modals.push(ActiveModal::RestoreAsk { path: p, parsed });
                }
            }
            ActiveModal::InkReview(r) => {
                if let Some(r) = self.ink_review_window(ctx, r) {
                    self.st.modals.push(ActiveModal::InkReview(r));
                }
            }
            ActiveModal::TranscribeAsk { rec_id } => {
                if self.transcribe_ask_window(ctx, rec_id) {
                    self.st.modals.push(ActiveModal::TranscribeAsk { rec_id });
                }
            }
            ActiveModal::DropPdf { path } => {
                if self.drop_pdf_window(ctx, &path) {
                    self.st.modals.push(ActiveModal::DropPdf { path });
                }
            }
            ActiveModal::Trash => {
                if self.trash_window(ctx) {
                    self.st.modals.push(ActiveModal::Trash);
                }
            }
            ActiveModal::Shortcuts => {
                if self.shortcuts_window(ctx) {
                    self.st.modals.push(ActiveModal::Shortcuts);
                }
            }
            ActiveModal::PathPrompt { how, path } => {
                if let Some(path) = self.path_prompt_window(ctx, how, path) {
                    self.st.modals.push(ActiveModal::PathPrompt { how, path });
                }
            }
        }
    }

    fn mic_window(&mut self, ctx: &egui::Context) -> bool {
        let (mut allow, mut no) = (false, false);
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(24)).corner_radius(18);
        let modal = egui::Modal::new(egui::Id::unique("mic")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 440.0));
            let (r, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
            let (c, acc) = (r.center(), accent(ui));
            ui.painter().circle_filled(c, 28.0, ui.visuals().selection.bg_fill);
            ui.painter().rect_filled(Rect::from_center_size(c - Vec2::new(0.0, 5.0), Vec2::new(13.0, 22.0)), 6.5, acc);
            let arc: Vec<Pos2> = (0..=16).map(|i| { let t = std::f32::consts::PI * i as f32 / 16.0; c + Vec2::new(-11.0 * t.cos(), -2.0 + 11.0 * t.sin()) }).collect();
            ui.painter().add(egui::Shape::line(arc, EStroke::new(2.2, acc)));
            ui.painter().line_segment([c + Vec2::new(0.0, 9.0), c + Vec2::new(0.0, 15.0)], EStroke::new(2.2, acc));
            ui.painter().line_segment([c + Vec2::new(-6.0, 15.0), c + Vec2::new(6.0, 15.0)], EStroke::new(2.2, acc));
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Allow the microphone?")).font(rich::serif(23.0)).color(ui.visuals().strong_text_color()));
            ui.add_space(4.0);
            ui.label(tr("Leaf Notes records only while the red Record button is on. Each recording is saved with its page, on this device only, and is linked to the notes you write so you can tap a note to hear that moment again."));
            #[cfg(target_os = "android")]
            ui.label(RichText::new(tr("Android will ask you to confirm next.")).weak());
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add(egui::Button::new(RichText::new(tr("Allow and record")).color(Color32::WHITE)).fill(accent_base()).min_size(Vec2::new(150.0, 34.0))).clicked() { allow = true; }
                if ui.add(egui::Button::new(tr("Not now")).min_size(Vec2::new(100.0, 34.0))).clicked() { no = true; }
            });
        });
        if allow {
            self.st.p.mic_ok = true;
            self.st.dirty = true;
            self.st.audio.rec_toggle = true;
            return false;
        }
        !(no || modal.should_close())
    }

    fn study_window(&mut self, ctx: &egui::Context, mut sd: Study) -> Option<Study> {
        let mut close = false;
        let mut rate: Option<usize> = None;
        let (space, k1, k2, k3) = ctx.input_mut(|i| (
            i.consume_key(Modifiers::NONE, Key::Space),
            i.consume_key(Modifiers::NONE, Key::Num1),
            i.consume_key(Modifiers::NONE, Key::Num2),
            i.consume_key(Modifiers::NONE, Key::Num3),
        ));
        let (enter, back) = ctx.input_mut(|i| (
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Backspace) || i.consume_key(Modifiers::COMMAND, Key::Z),
        ));
        if (space || enter) && !sd.shown { sd.shown = true; } else if enter && sd.shown { rate = Some(1); }
        if sd.shown {
            if k1 { rate = Some(0) } else if k2 { rate = Some(1) } else if k3 { rate = Some(2) }
        }
        let mut back = back;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
        let max_card_h = (ctx.content_rect().height() - 280.0 - self.st.bars[0] - self.st.bars[1]).max(140.0);
        let modal = egui::Modal::new(egui::Id::unique("study")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 600.0));
            let acc = accent(ui);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Study")).font(rich::serif(24.0)).color(ui.visuals().strong_text_color()));
                ui.add(egui::Label::new(RichText::new(format!("·  {}", sd.title)).size(14.0).weak()).truncate());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("✕").size(15.0)).frame_when_inactive(false)).clicked() { close = true; }
                    if sd.undo.is_some() && ui.add(egui::Button::new(tr("↶ Undo")).frame_when_inactive(false)).tip("Take the last rating back (Backspace)").clicked() { back = true; }
                });
            });
            let done = sd.total - sd.queue.len();
            let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
            ui.painter().rect_filled(bar, 3.0, ui.visuals().widgets.inactive.weak_bg_fill);
            ui.painter().rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * done as f32 / sd.total.max(1) as f32, 6.0)), 3.0, acc);
            ui.add_space(12.0);
            let colors = rich::Colors::from(ui, acc);
            match sd.queue.first().copied() {
                None => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(20.0);
                        ui.label(RichText::new(tr("All done 🎉")).font(rich::serif(26.0)));
                        ui.label(RichText::new(tf!("{total} cards · {x} again · {x2} good · {x3} easy", total = sd.total, x = sd.tally[0], x2 = sd.tally[1], x3 = sd.tally[2])).weak());
                        let next = sd.cards.iter().filter_map(|c| self.store.study.get(&c.key)).map(|s| s.due).filter(|d| *d > now_secs()).min();
                        if let Some(d) = next {
                            ui.label(RichText::new(tf!("Next review: {when}", when = format_date(d, false))).weak());
                        }
                        ui.add_space(14.0);
                        ui.horizontal_wrapped(|ui| {
                            if !sd.missed.is_empty() && ui.add(egui::Button::new(RichText::new(tf!("Practise the {n} missed", n = sd.missed.len())).color(Color32::WHITE)).fill(accent_base()).min_size(Vec2::new(110.0, 32.0))).clicked() {
                                sd.queue = std::mem::take(&mut sd.missed);
                                sd.total = sd.queue.len();
                                sd.tally = [0; 3];
                                sd.shown = false;
                            }
                            if ui.add(egui::Button::new(tr("Close")).min_size(Vec2::new(100.0, 32.0))).clicked() { close = true; }
                        });
                        ui.add_space(10.0);
                    });
                }
                Some(i) => {
                    let card = sd.cards[i].clone();
                    ui.label(RichText::new(tf!("Card {x} of {total}", x = done + 1, total = sd.total)).size(11.5).weak());
                    egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(16).inner_margin(egui::Margin::same(20)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::vertical().id_salt(("study-card", i)).max_height(max_card_h).show(ui, |ui| {
                            ui.set_min_height(140.0f32.min(max_card_h));
                            let _ = rich::view(ui, &card.front, 20.0, 0, &colors);
                            if sd.shown {
                                ui.add_space(10.0);
                                ui.separator();
                                ui.add_space(10.0);
                                let _ = rich::view(ui, &card.back, 16.0, 0, &colors);
                            }
                        });
                    });
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if !sd.shown {
                            if ui.add(egui::Button::new(RichText::new(tr(if self.st.nav.touch_ui { "Show answer" } else { "Show answer   Space" })).color(Color32::WHITE)).fill(accent_base()).min_size(Vec2::new(ui.available_width(), 38.0))).clicked() {
                                sd.shown = true;
                            }
                        } else {
                            let gap = ui.spacing().item_spacing.x;
                            let w = ((ui.available_width() - 2.0 * gap) / 3.0).max(60.0);
                            let t = self.st.nav.touch_ui;
                            for (k, (label, col)) in [
                                (tr(if t { "Again" } else { "Again   1" }), Color32::from_rgb(214, 82, 62)),
                                (tr(if t { "Good" } else { "Good   2" }), accent_base()),
                                (tr(if t { "Easy" } else { "Easy   3" }), Color32::from_rgb(21, 128, 61)),
                            ].into_iter().enumerate() {
                                if ui.add(egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(col).min_size(Vec2::new(w, 38.0))).clicked() {
                                    rate = Some(k);
                                }
                            }
                        }
                    });
                }
            }
        });
        if back {
            if let Some(u) = sd.undo.take() {
                match u.before {
                    Some(b) => { self.store.study.insert(u.key, b); }
                    None => { self.store.study.remove(&u.key); }
                }
                sd.queue = u.queue;
                sd.missed = u.missed;
                sd.tally[u.rating] = sd.tally[u.rating].saturating_sub(1);
                sd.shown = true;
                rate = None;
                self.st.dirty = true;
                self.st.cache.derived_dirty = true;
            }
        }
        if let (Some(k), Some(i)) = (rate, sd.queue.first().copied()) {
            let key = sd.cards[i].key.clone();
            sd.undo = Some(StudyUndo { key: key.clone(), before: self.store.study.get(&key).copied(), rating: k, queue: sd.queue.clone(), missed: sd.missed.clone() });
            let state = self.store.study.entry(key).or_default();
            sd.queue.remove(0);
            match k {
                0 => {
                    state.bx = state.bx.saturating_sub(2);
                    state.due = now_secs() + 600;
                    let at = sd.queue.len().min(3);
                    sd.queue.insert(at, i);
                    if !sd.missed.contains(&i) { sd.missed.push(i); }
                }
                n => {
                    state.bx = (state.bx + n as u8).min(5);
                    state.due = now_secs() + STUDY_DAYS[state.bx as usize] * 86_400;
                }
            }
            sd.tally[k] += 1;
            sd.shown = false;
            self.st.dirty = true;
            self.st.cache.derived_dirty = true;
        }
        if !close && !modal.should_close() { Some(sd) } else { None }
    }

    fn switcher_window(&mut self, ctx: &egui::Context, mut q: String, mut sel: usize) -> Option<(String, usize)> {
        let ql = q.trim().to_lowercase();
        let cached = self.st.cache.switch_cache.take().filter(|(cq, _)| *cq == ql).map(|(_, h)| h);
        let mut hits: Vec<SwitchHit> = cached.clone().unwrap_or_default();
        let (cmd_only, cq) = match ql.strip_prefix('>') {
            Some(r) => (true, r.trim().to_string()),
            None => (false, ql.clone()),
        };
        if cached.is_none() {
            for (i, name) in COMMANDS.iter().enumerate() {
                let n = tr(name).to_lowercase();
                if (cmd_only && (cq.is_empty() || n.contains(&cq))) || (!cmd_only && cq.len() >= 2 && n.contains(&cq)) {
                    hits.push((if cmd_only { 5.0 } else { 0.4 }, format!("› {}", tr(name)), tr("Command").to_string(), (usize::MAX, i, 0)));
                }
            }
            if !cmd_only {
                for (a, n) in self.store.notebooks.iter().enumerate() {
                    for (b, s) in n.sections.iter().enumerate() {
                        for (c, p) in s.pages.iter().enumerate() {
                            let title = if p.title.trim().is_empty() { tr("Untitled page").to_string() } else { p.title.clone() };
                            let path = format!("{} › {}", n.name, s.name);
                            let score = if ql.is_empty() {
                                let r = self.st.p.recent.iter().position(|id| *id == p.id);
                                match r {
                                    Some(i) => 100.0 - i as f32,
                                    None if self.st.p.favorites.contains(&p.id) => 50.0,
                                    None => 0.0,
                                }
                            } else {
                                let t = title.to_lowercase();
                                if t.starts_with(&ql) { 3.0 }
                                else if ql.split_whitespace().all(|w| t.contains(w)) { 2.0 }
                                else if ql.split_whitespace().all(|w| path.to_lowercase().contains(w) || t.contains(w)) { 1.0 }
                                else if crate::index::fuzzy_find(&t, &ql).is_some() { 0.8 }
                                else if p.plain_text().to_lowercase().contains(&ql) { 0.5 }
                                else { continue }
                            };
                            hits.push((score, title, path, (a, b, c)));
                        }
                    }
                }
            }
            hits.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
            hits.truncate(12);
            if !ql.is_empty() && !cmd_only && !hits.iter().any(|h| h.1.trim().eq_ignore_ascii_case(q.trim())) {
                hits.push((0.0, tf!("+  Create a page called “{trim}”", trim = q.trim()), String::new(), (usize::MAX - 1, 0, 0)));
            }
        }
        self.st.cache.switch_cache = Some((ql.clone(), hits.clone()));
        let (down, up, enter) = ctx.input_mut(|i| (
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::Enter),
        ));
        if down { sel = (sel + 1).min(hits.len().saturating_sub(1)); }
        if up { sel = sel.saturating_sub(1); }
        let mut go = if enter { hits.get(sel).map(|h| h.3) } else { None };
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16)).corner_radius(16);
        let id = egui::Id::unique("switcher");
        let top = self.st.bars[0] + if self.st.nav.narrow { 12.0 } else { (ctx.content_rect().height() * 0.12).min(120.0) };
        let room = ctx.content_rect().height() - top - self.st.bars[1] - 140.0;
        let area = egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, Vec2::new(0.0, top));
        let modal = egui::Modal::new(id).area(area).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 520.0));
            let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text(tr("Jump to a page… (> for commands)")).font(FontId::proportional(18.0)).desired_width(f32::INFINITY).frame(egui::Frame::NONE));
            if !r.has_focus() { r.request_focus(); }
            if r.changed() { sel = 0; }
            ui.separator();
            ui.label(RichText::new(tr(if cmd_only { "COMMANDS" } else { "PAGES" })).size(10.5).weak());
            egui::ScrollArea::vertical().max_height(room.max(132.0)).show(ui, |ui| {
                for (i, (_, title, path, at)) in hits.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                    if i == sel || resp.hovered() {
                        ui.painter().rect_filled(rect, 8.0, if i == sel { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.hovered.weak_bg_fill });
                    }
                    let (strong, weak) = (ui.visuals().strong_text_color(), ui.visuals().weak_text_color());
                    let g = one_line(ui.painter(), title, FontId::proportional(14.5), strong, rect.width() - 24.0);
                    ui.painter().galley(rect.min + Vec2::new(12.0, 13.0 - g.size().y / 2.0), g, strong);
                    let g = one_line(ui.painter(), path, FontId::proportional(11.5), weak, rect.width() - 24.0);
                    ui.painter().galley(rect.min + Vec2::new(12.0, 31.0 - g.size().y / 2.0), g, weak);
                    if resp.clicked() { go = Some(*at); }
                    if i == sel && (down || up) { ui.scroll_to_rect(rect, None); }
                }
            });
            if !self.st.nav.touch_ui {
                ui.label(RichText::new(tr("↑↓ to choose · Enter to open · Esc to close")).size(10.5).weak());
            }
        });
        match go {
            Some((usize::MAX, i, _)) => { self.run_command(i); return None; }
            Some((x, ..)) if x == usize::MAX - 1 => { let t = q.trim().to_string(); self.open_title(&t); return None; }
            Some((a, b, c)) => { self.go_to(a, b, c); return None; }
            None => {}
        }
        if !modal.should_close() { Some((q, sel)) } else { None }
    }

    fn run_command(&mut self, i: usize) {
        match i {
            0 => { self.new_page(None); }
            1 => self.new_deck("New deck", vec![]),
            2 => self.start_review(),
            3 => { self.st.nav.tasks = true; self.st.p.show_nav = true; }
            4 => self.st.modals.open_settings(0),
            5 => self.st.modals.push(ActiveModal::Trash),
            6 => self.show_history(),
            7 => self.st.modals.push(ActiveModal::Shortcuts),
            8 => { self.st.p.dark ^= true; self.st.p.follow_system = false; self.st.dirty = true; self.st.theme_dirty = true; }
            9 => self.export_pdf(true),
            10 => self.start_lecture(false),
            _ => {}
        }
    }

    fn ink_review_window(&mut self, ctx: &egui::Context, mut r: InkReview) -> Option<InkReview> {
        let mut done = None;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(20)).corner_radius(16);
        let m = egui::Modal::new(egui::Id::unique("ink-review")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 460.0));
            ui.label(RichText::new(tr(if r.math { "Read as maths" } else { "Read as text" })).font(rich::serif(20.0)));
            ui.label(RichText::new(tr("Correct anything misread, then choose what to do.")).weak());
            ui.add(egui::TextEdit::multiline(&mut r.text).desired_rows(4).desired_width(f32::INFINITY).font(if r.math { egui::TextStyle::Monospace } else { egui::TextStyle::Body }));
            if r.math && !r.text.trim().is_empty() { math::math_label(ui, &r.text, 20.0); }
            ui.horizontal_wrapped(|ui| {
                if ui.add(egui::Button::new(RichText::new(tr("Replace the ink")).color(Color32::WHITE)).fill(accent_base())).clicked() { done = Some(Some(false)); }
                if ui.button(tr("Add it, keep the ink")).clicked() { done = Some(Some(true)); }
                if ui.button(tr("Cancel")).clicked() { done = Some(None); }
            });
        });
        match done {
            Some(Some(keep)) => { self.apply_ink(r, keep); None }
            Some(None) => None,
            None if m.should_close() => None,
            None => Some(r),
        }
    }

    fn transcribe_ask_window(&mut self, ctx: &egui::Context, id: u64) -> bool {
        let secs = self.store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).flat_map(|p| &p.recordings).find(|r| r.id == id).map_or(0.0, |r| r.secs);
        let host = self.st.ai.base_url.split('/').nth(2).unwrap_or("").to_string();
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
        let mut go = None;
        let m = egui::Modal::new(egui::Id::unique("transcribe-ask")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 420.0));
            ui.label(RichText::new(tr("Make a transcript?")).font(rich::serif(20.0)));
            ui.label(tf!("About {min} minutes of audio (≈{mb} MB) will be sent to {host} to be turned into text. Services charge for this by the minute.", min = format!("{:.0}", (secs / 60.0).ceil()), mb = format!("{:.0}", secs * 32_000.0 / 1e6), host = host));
            ui.horizontal_wrapped(|ui| {
                if ui.add(egui::Button::new(RichText::new(tr("Send it")).color(Color32::WHITE)).fill(accent_base())).clicked() { go = Some(true); }
                if ui.button(tr("Cancel")).clicked() { go = Some(false); }
            });
        });
        if m.should_close() { go = Some(false); }
        match go {
            Some(true) => { self.start_transcribe(id); false }
            Some(false) => false,
            None => true,
        }
    }

    fn drop_pdf_window(&mut self, ctx: &egui::Context, path: &std::path::Path) -> bool {
        let mut how = None;
        let mut cancel = false;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
        let m = egui::Modal::new(egui::Id::unique("drop-pdf")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 400.0));
            ui.label(RichText::new(path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).strong());
            ui.add_space(6.0);
            if ui.button(tr("Print its pages onto this page (to write on them)")).clicked() { how = Some(Ins::Printout); }
            if ui.button(tr("Attach it as a file")).clicked() { how = Some(Ins::Attach); }
            if ui.button(tr("Cancel")).clicked() { cancel = true; }
        });
        if let Some(h) = how {
            self.insert_file(path, h);
            return false;
        }
        !(cancel || m.should_close())
    }

    fn trash_window(&mut self, ctx: &egui::Context) -> bool {
        let mut close = false;
        let mut restore = None;
        let mut purge = None;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(20)).corner_radius(18);
        let max_h = (ctx.content_rect().height() - 220.0 - self.st.bars[0] - self.st.bars[1]).clamp(120.0, 420.0);
        let m = egui::Modal::new(egui::Id::unique("trash")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 460.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Recently deleted")).font(rich::serif(22.0)).color(ui.visuals().strong_text_color()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("✕").size(15.0)).frame_when_inactive(false)).tip("Close (Esc)").clicked() { close = true; }
                });
            });
            ui.add_space(6.0);
            if self.store.trash.is_empty() {
                ui.label(RichText::new(tr("Nothing here. Deleted pages, sections and notebooks wait here until you empty the bin.")).weak());
            }
            egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
                for (i, t) in self.store.trash.iter().enumerate().rev() {
                    ui.horizontal(|ui| {
                        // Place action buttons first in right-to-left layout so long labels truncate cleanly instead of pushing buttons off-screen.
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let sure_id = egui::Id::unique("purge-one-sure");
                            if ui.data(|d| d.get_temp::<usize>(sure_id)) == Some(i) {
                                if ui.small_button(tr("Cancel")).clicked() { ui.data_mut(|d| d.remove::<usize>(sure_id)); }
                                if ui.small_button(RichText::new(tr("Delete for good")).color(danger_color(ui))).clicked() {
                                    purge = Some(i);
                                    ui.data_mut(|d| d.remove::<usize>(sure_id));
                                }
                            } else {
                                if ui.small_button(tr("Delete forever")).clicked() { ui.data_mut(|d| d.insert_temp(sure_id, i)); }
                                if ui.small_button(tr("Restore")).clicked() {
                                    restore = Some(i);
                                    ui.data_mut(|d| d.remove::<usize>(sure_id));
                                }
                            }
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.vertical(|ui| {
                                    ui.add(egui::Label::new(&t.label).truncate());
                                    ui.add(egui::Label::new(RichText::new(tf!("Deleted {x}", x = format_date(t.deleted, true))).size(11.0).weak()).truncate());
                                });
                            });
                        });
                    });
                    ui.separator();
                }
            });
            let sure_id = egui::Id::unique("empty-bin-sure");
            let sure = ui.data(|d| d.get_temp::<bool>(sure_id)).unwrap_or(false);
            if !self.store.trash.is_empty() {
                ui.add_space(4.0);
                if !sure {
                    if ui.button(tr("Empty the bin")).clicked() { ui.data_mut(|d| d.insert_temp(sure_id, true)); }
                } else {
                    ui.label(RichText::new(tr("Everything here will be deleted for good. This can't be undone.")).color(danger_color(ui)));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(RichText::new(tr("Delete all for good")).color(danger_color(ui))).clicked() {
                            purge = Some(usize::MAX);
                            ui.data_mut(|d| d.remove::<bool>(sure_id));
                        }
                        if ui.button(tr("Cancel")).clicked() { ui.data_mut(|d| d.remove::<bool>(sure_id)); }
                    });
                }
            }
        });
        if let Some(i) = restore { self.restore(i); }
        match purge {
            Some(usize::MAX) => { let gone = std::mem::take(&mut self.store.trash); self.purge(gone); }
            Some(i) => { let gone = self.store.trash.remove(i); self.purge(vec![gone]); }
            None => {}
        }
        !(close || m.should_close())
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) -> bool {
        let mut close = false;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(20)).corner_radius(18);
        let max_h = (ctx.content_rect().height() - 180.0 - self.st.bars[0] - self.st.bars[1]).max(160.0);
        let m = egui::Modal::new(egui::Id::unique("shortcuts")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 580.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Keyboard shortcuts")).font(rich::serif(22.0)).color(ui.visuals().strong_text_color()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("✕").size(15.0)).frame_when_inactive(false)).tip("Close (Esc)").clicked() { close = true; }
                });
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
                egui::Grid::new("keys").num_columns(2).spacing([24.0, 6.0]).striped(true).show(ui, |ui| {
                    for (k, what) in [
                        ("Ctrl+N", "New page"), ("Ctrl+F", "Search all notes"), ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
                        ("Ctrl+B / Ctrl+I", "Bold / italic"), ("Ctrl+Shift+H", "Highlight"), ("Ctrl+M", "Inline math"), ("Ctrl+`", "Inline code"),
                        ("T  P  H  E  L", "Type, pen, highlighter, eraser, select ink"), ("Delete", "Delete selected ink"), ("Esc", "Clear selection and search"),
                        ("Ctrl + / Ctrl −", "Make everything larger or smaller"), ("Ctrl+K", "Jump to a page"), ("Two fingers", "Scroll while drawing (touch screens)"), ("Click empty paper", "Start a new note there"),
                        ("- , 1. , [ ] , #", "Start a list, checkbox or heading while typing"), ("/", "Insert menu while typing"), ("[[", "Link to another page while typing"),
                        ("Space · 1 2 3", "Flashcards: show the answer, then rate it"), ("Alt+← / Alt+→", "Back / forward between pages"),
                        ("Ctrl+D", "Duplicate this page"), ("F2", "Rename this page"), ("Ctrl+Shift+T", "All tasks"), ("Ctrl+Shift+R", "Review due flashcards"),
                        ("Ctrl+,", "Settings"), ("F1", "This list"),
                    ] {
                        ui.label(RichText::new(tr(k)).monospace());
                        ui.add(egui::Label::new(tr(what)).wrap());
                        ui.end_row();
                    }
                });
            });
        });
        !(close || m.should_close())
    }

    fn path_prompt_window(&mut self, ctx: &egui::Context, how: Ins, mut path: String) -> Option<String> {
        let mut go = false;
        let mut cancel = false;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(20)).corner_radius(18);
        let title = tr(match how {
            Ins::Picture => "Insert picture",
            Ins::Printout => "Insert printout",
            Ins::Attach => "Attach file",
        });
        let m = egui::Modal::new(egui::Id::unique("path-prompt")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 420.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).font(rich::serif(20.0)).color(ui.visuals().strong_text_color()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("✕").size(15.0)).frame_when_inactive(false)).tip("Close (Esc)").clicked() { cancel = true; }
                });
            });
            ui.add_space(6.0);
            ui.label(tr("File path on this device:"));
            let r = ui.add(egui::TextEdit::singleline(&mut path).desired_width(f32::INFINITY));
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) { go = true; }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(tr("Insert")).color(Color32::WHITE)).fill(accent_base())).clicked() { go = true; }
                if ui.button(tr("Cancel")).clicked() { cancel = true; }
            });
        });
        if go {
            self.insert_file(std::path::Path::new(&path), how);
            return None;
        }
        if cancel || m.should_close() { None } else { Some(path) }
    }
}
