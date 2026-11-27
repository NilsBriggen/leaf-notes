//! Full-text search, tag filtering, and cross-notebook open tasks panels.

use super::{danger_color, due_day, local_offset, one_line, want_focus, SearchHit};
use crate::*;
use eframe::egui::{Align, FontId, Layout, RichText, Sense};

/// Map a byte range `(start, end)` in `text.to_lowercase()` back to valid UTF-8 byte offsets in `text`.
/// Handles characters whose lowercase expansion changes byte length (e.g. `İ` -> `i\u{307}`, `ẞ` -> `ß`).
pub(crate) fn map_lower_span(text: &str, low_start: usize, low_end: usize) -> (usize, usize) {
    if text.is_ascii() {
        return (low_start.min(text.len()), low_end.min(text.len()));
    }
    let mut orig_start = None;
    let mut orig_end = None;
    let mut low_pos = 0usize;
    for (orig_pos, ch) in text.char_indices() {
        let len: usize = ch.to_lowercase().map(|c| c.len_utf8()).sum();
        if orig_start.is_none() && low_pos + len > low_start {
            orig_start = Some(orig_pos);
        }
        low_pos += len;
        if orig_end.is_none() && low_pos >= low_end {
            orig_end = Some(orig_pos + ch.len_utf8());
            break;
        }
    }
    let s = orig_start.unwrap_or(text.len());
    let e = orig_end.unwrap_or(text.len()).max(s);
    (s, e)
}

/// Convert raw page text (including inline Markdown & LaTeX) into a readable single-line search snippet.
pub(crate) fn clean_snippet(t: &str) -> String {
    let mut s = t.replace('\n', " ");
    for (cmd, sym) in [
        (r"\sum", "∑"), (r"\int", "∫"), (r"\infty", "∞"), (r"\pi", "π"),
        (r"\alpha", "α"), (r"\beta", "β"), (r"\gamma", "γ"), (r"\theta", "θ"),
        (r"\omega", "ω"), (r"\lambda", "λ"), (r"\sigma", "σ"), (r"\Delta", "Δ"),
        (r"\cdot", "·"), (r"\times", "×"), (r"\pm", "±"), (r"\le", "≤"), (r"\ge", "≥"),
        (r"\ne", "≠"), (r"\approx", "≈"), (r"\to", "→"), (r"\Rightarrow", "⇒"),
        (r"\frac", " "), (r"\sqrt", "√"), (r"\left", ""), (r"\right", ""),
        (r"\begin{cases}", " "), (r"\end{cases}", " "), (r"\begin{pmatrix}", " "), (r"\end{pmatrix}", " "),
    ] {
        if s.contains(cmd) {
            s = s.replace(cmd, sym);
        }
    }
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        let mapped = match ch {
            '\\' | '{' | '}' | '^' | '_' | '$' | '*' | '=' | '`' | '#' => ' ',
            c => c,
        };
        if mapped.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(mapped);
            prev_space = false;
        }
    }
    out
}

pub(crate) type OpenTask = ((usize, usize, usize), String, u64, usize, String, bool);

impl NotesApp {
    /// Every open to-do in every notebook: checklist items and "[ ]" lines in text notes.
    pub(crate) fn open_tasks(&self) -> Vec<OpenTask> {
        let mut out = vec![];
        for (a, n) in self.store.notebooks.iter().enumerate() {
            for (b, s) in n.sections.iter().enumerate() {
                for (c, p) in s.pages.iter().enumerate() {
                    let title = if p.title.trim().is_empty() { tr("Untitled page").to_string() } else { p.title.clone() };
                    for bl in &p.blocks {
                        match &bl.kind {
                            Kind::Checklist { items } => {
                                for (i, (d, t)) in items.iter().enumerate() {
                                    if !t.trim().is_empty() && !d { out.push(((a, b, c), title.clone(), bl.id, i, t.clone(), *d)); }
                                }
                            }
                            Kind::Text { text, .. } => {
                                for (i, l) in text.lines().enumerate() {
                                    if let (rich::LineKind::Check(false), plen) = rich::classify(l) {
                                        if !l[plen..].trim().is_empty() { out.push(((a, b, c), title.clone(), bl.id, i, l[plen..].to_string(), false)); }
                                    }
                                }
                            }
                            _ => {}
                        }
                        if bl.tag == Tag::Todo {
                            out.push(((a, b, c), title.clone(), bl.id, usize::MAX, bl.kind.summary().lines().next().unwrap_or("").to_string(), false));
                        }
                    }
                }
            }
        }
        out
    }

    pub(crate) fn tasks_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("All tasks")).font(rich::serif(18.0)).color(ui.visuals().strong_text_color()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new("✕").frame_when_inactive(false)).tip("Back to pages").clicked() { self.st.nav.tasks = false; }
            });
        });
        ui.label(RichText::new(tr("Open to-dos from every notebook. Tick them off here.")).size(11.5).weak());
        ui.separator();
        let mut tasks = self.open_tasks();
        tasks.sort_by_key(|t| due_day(&t.4).unwrap_or(i64::MAX));
        let today = (now_secs() as i64 + local_offset(now_secs())).div_euclid(86_400);
        let mut tick = None;
        let mut go = None;
        egui::ScrollArea::vertical().id_salt("tasks-scroll").auto_shrink([false; 2]).show(ui, |ui| {
            if tasks.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(tr("Nothing to do 🎉\nAdd a checklist, or start a line with [ ].")).weak()));
            }
            let mut last_page = None;
            for (at, title, bid, idx, text, _) in &tasks {
                if last_page != Some(*at) {
                    ui.add_space(6.0);
                    if ui.add(egui::Label::new(RichText::new(title).size(12.0).color(accent(ui))).sense(Sense::click())).tip("Open this page").clicked() { go = Some(*at); }
                    last_page = Some(*at);
                }
                ui.horizontal(|ui| {
                    let mut d = false;
                    if ui.checkbox(&mut d, "").changed() { tick = Some((*at, *bid, *idx)); }
                    if let Some(day) = due_day(text) {
                        let (label, col) = if day < today { (tr("overdue"), danger_color(ui)) } else if day == today { (tr("today"), accent(ui)) } else { ("", ui.visuals().weak_text_color()) };
                        if !label.is_empty() { ui.label(RichText::new(label).size(11.0).color(col)); }
                    }
                    if ui.add(egui::Label::new(RichText::new(text).size(13.5)).wrap().sense(Sense::click())).tip("Open this page").clicked() { go = Some(*at); }
                });
            }
        });
        if let Some(((a, b, c), bid, idx)) = tick {
            let page = &mut self.store.notebooks[a].sections[b].pages[c];
            let snap = page.clone();
            if let Some(bl) = page.blocks.iter_mut().find(|bl| bl.id == bid) {
                match &mut bl.kind {
                    _ if idx == usize::MAX => bl.tag = Tag::Done,
                    Kind::Checklist { items } => if let Some(it) = items.get_mut(idx) { it.0 = true; },
                    Kind::Text { text, .. } => {
                        let lines: Vec<String> = text.lines().enumerate().map(|(i, l)| if i == idx { l.replacen("[ ]", "[x]", 1) } else { l.to_string() }).collect();
                        *text = lines.join("\n");
                    }
                    _ => {}
                }
            }
            let here = snap.id == self.page().id;
            self.st.snap(&snap);
            self.st.say(if here { "Done ✓ (Undo brings it back)" } else { "Done ✓" });
        }
        if let Some((a, b, c)) = go {
            self.go_to(a, b, c);
            self.st.nav.tasks = false;
        }
    }

    pub(crate) fn search_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr(if self.st.nav.tag_filter.is_some() && self.st.nav.search.trim().is_empty() && !self.st.nav.search_open { "Tags" } else { "Search" })).size(15.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new("✕").frame_when_inactive(false)).tip("Close (Esc)").clicked() {
                    self.st.nav.search.clear();
                    self.st.nav.tag_filter = None;
                    self.st.nav.search_open = false;
                }
            });
        });
        // On phones and narrow windows where the title-bar search bar collapses to 🔍, show an inline search box here.
        if self.st.nav.search_icon_mode || self.st.nav.search_open {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.st.nav.search)
                    .id(egui::Id::unique("search-panel-input"))
                    .hint_text(tr("Search everything"))
                    .desired_width(f32::INFINITY),
            );
            want_focus(&mut self.st, &r);
            ui.add_space(4.0);
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            for (t, label) in [
                (None, tr("All")),
                (Some(Tag::None), tr("Any tag")),
                (Some(Tag::Todo), tr("☐ To Do")),
                (Some(Tag::Important), tr("★ Important")),
                (Some(Tag::Question), tr("? Question")),
            ] {
                if ui.add(egui::Button::new(RichText::new(label).size(12.0)).selected(self.st.nav.tag_filter == t).corner_radius(10)).clicked() {
                    self.st.nav.tag_filter = t;
                }
            }
        });
        // Wrapped scope chips so German / French labels never clip on narrow panels.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            for (k, label) in [(0u8, "Everywhere"), (1, "This notebook"), (2, "This section")] {
                if ui.add(egui::Button::new(RichText::new(tr(label)).size(11.5)).selected(self.st.nav.search_scope == k).corner_radius(10)).clicked() {
                    self.st.nav.search_scope = k;
                    self.st.cache.search_cache = None;
                }
            }
        });
        ui.separator();
        let q = self.st.nav.search.trim().to_lowercase();
        let filter = self.st.nav.tag_filter;
        let (scope, here) = (self.st.nav.search_scope, (self.nb, self.sec));
        let in_scope = move |ni: usize, si: usize| match scope { 1 => ni == here.0, 2 => (ni, si) == here, _ => true };
        let acc = accent(ui);
        let mark = if ui.visuals().dark_mode { Color32::from_rgba_unmultiplied(234, 179, 8, 125) } else { Color32::from_rgb(255, 236, 140) };
        let mut go = None;
        let mut toggle = None;
        let mut found = 0;

        let fresh = self.st.cache.search_cache.as_ref().is_some_and(|(cq, cscope, at, _)| *cq == q && *cscope == scope && now_f() - at < 1.0);
        if filter.is_none() && !fresh {
            let mut hits: Vec<SearchHit> = vec![];
            for (ni, n) in self.store.notebooks.iter().enumerate() {
                for (si, s) in n.sections.iter().enumerate().filter(|(si, _)| in_scope(ni, *si)) {
                    for (pi, p) in s.pages.iter().enumerate() {
                        let mut text = p.plain_text();
                        if let Some(extra) = self.st.index.text(p.id) { text.push('\n'); text.push_str(extra); }
                        let lower = text.to_lowercase();
                        let Some((low_at, low_end, exact)) = crate::index::fuzzy_find(&lower, &q) else { continue };
                        let (at, end) = map_lower_span(&text, low_at, low_end);
                        hits.push((!exact, (ni, si, pi), p.title.clone(), format!("{} › {}", n.name, s.name), text, at, end));
                    }
                }
            }
            hits.sort_by_key(|h| h.0);
            self.st.cache.search_cache = Some((q.clone(), scope, now_f(), hits));
        }
        // Take the cache temporarily to borrow without cloning all hit strings every frame.
        let taken_cache = if filter.is_none() { self.st.cache.search_cache.take() } else { None };
        let empty_hits: Vec<SearchHit> = vec![];
        let hits: &[SearchHit] = taken_cache.as_ref().map_or(&empty_hits, |c| &c.3);

        if filter.is_none() && !q.is_empty() {
            ui.label(RichText::new(crate::i18n::fill(tr(if hits.len() == 1 { "{n} page" } else { "{n} pages" }), &[("n", &hits.len())])).size(11.0).weak());
        }
        egui::ScrollArea::vertical().id_salt("search-scroll").auto_shrink([false; 2]).show(ui, |ui| {
            if let Some(f) = filter {
                for (ni, n) in self.store.notebooks.iter().enumerate() {
                    for (si, s) in n.sections.iter().enumerate().filter(|(si, _)| in_scope(ni, *si)) {
                        for (pi, p) in s.pages.iter().enumerate() {
                            for b in &p.blocks {
                                let tag_ok = match f {
                                    Tag::None => b.tag != Tag::None,
                                    Tag::Todo => matches!(b.tag, Tag::Todo | Tag::Done),
                                    t => b.tag == t,
                                };
                                let text = b.kind.summary();
                                if !tag_ok || (!q.is_empty() && !text.to_lowercase().contains(&q)) { continue; }
                                found += 1;
                                ui.horizontal(|ui| {
                                    match b.tag {
                                        Tag::Todo | Tag::Done => {
                                            let mut d = b.tag == Tag::Done;
                                            if ui.checkbox(&mut d, "").changed() { toggle = Some((p.id, b.id)); }
                                        }
                                        Tag::Important => { ui.label(RichText::new("★").color(Color32::from_rgb(234, 179, 8))); }
                                        Tag::Question => { ui.label(RichText::new("?").strong().color(Color32::from_rgb(124, 58, 237))); }
                                        Tag::None => {}
                                    }
                                    let label = if text.chars().count() > 60 { format!("{}…", text.chars().take(60).collect::<String>()) } else { text.clone() };
                                    let r = ui.add(egui::Label::new({
                                        let t = RichText::new(label).size(13.0);
                                        if b.tag == Tag::Done { t.strikethrough() } else { t }
                                    }).sense(Sense::click()).wrap());
                                    if r.tip(format!("{} › {} › {}", n.name, s.name, p.title)).clicked() { go = Some((ni, si, pi)); }
                                });
                            }
                        }
                    }
                }
            }
            for (_, at_page, title, path, text, at, end) in hits {
                let (at, end) = ((*at).min(text.len()), (*end).min(text.len()));
                found += 1;
                let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::click());
                if r.hovered() { ui.painter().rect_filled(rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill); }
                let painter = ui.painter_at(rect.shrink(4.0));
                let strong = ui.visuals().strong_text_color();
                painter.galley(rect.min + Vec2::new(8.0, 6.0), one_line(&painter, if title.is_empty() { tr("Untitled page") } else { title.as_str() }, FontId::proportional(14.0), strong, rect.width() - 16.0), strong);
                painter.galley(rect.min + Vec2::new(8.0, 24.0), one_line(&painter, path, FontId::proportional(10.5), acc, rect.width() - 16.0), acc);
                let mut start = text.get(..at).unwrap_or("").char_indices().rev().nth(24).map_or(0, |(i, _)| i);
                if start > 0 {
                    // Snap forward to a nearby word boundary so we don't start mid-word when room allows.
                    if let Some(ws) = text.get(start..at).and_then(|s| s.find(char::is_whitespace)).filter(|&ws| ws > 0 && ws <= 10) {
                        start += ws;
                    }
                }
                let tail_end = text.get(end..).and_then(|t| t.char_indices().nth(40)).map_or(text.len(), |(i, _)| end + i);
                if text.is_char_boundary(start) && text.is_char_boundary(at) && text.is_char_boundary(end) && text.is_char_boundary(tail_end) {
                    let mut job = egui::text::LayoutJob::default();
                    let f = |c: Color32, bg: Color32| egui::TextFormat { font_id: FontId::proportional(11.5), color: c, background: bg, ..Default::default() };
                    let weak = ui.visuals().weak_text_color();
                    let before = clean_snippet(&text[start..at]);
                    let before = if start > 0 { format!("…{}", before.trim_start()) } else { before.trim_start().to_string() };
                    job.append(&before, 0.0, f(weak, Color32::TRANSPARENT));
                    job.append(&text[at..end].replace('\n', " "), 0.0, f(ui.visuals().text_color(), mark));
                    job.append(&clean_snippet(&text[end..tail_end]), 0.0, f(weak, Color32::TRANSPARENT));
                    job.wrap.max_width = rect.width() - 16.0;
                    job.wrap.max_rows = 1;
                    let g = painter.layout_job(job);
                    painter.galley(rect.min + Vec2::new(8.0, 39.0), g, weak);
                }
                if r.clicked() { go = Some(*at_page); }
            }
            if found == 0 {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(tr(if filter.is_some() { "No tagged notes yet.\nTag a note from the Home tab." } else { "No matches." })).weak()));
            }
        });
        if let Some(c) = taken_cache {
            self.st.cache.search_cache = Some(c);
        }
        if let Some((pid, bid)) = toggle {
            if let Some(b) = self.find_page(pid).and_then(|p| p.blocks.iter_mut().find(|b| b.id == bid)) {
                b.tag = if b.tag == Tag::Done { Tag::Todo } else { Tag::Done };
            }
            self.st.dirty = true;
            self.st.cache.derived_dirty = true;
        }
        if let Some((a, b, c)) = go {
            self.go_to(a, b, c);
            if filter.is_none() {
                self.st.canvas.find_text = Some(q.clone()).filter(|q| !q.is_empty());
                self.st.nav.search.clear();
                self.st.nav.search_open = false;
            }
        }
    }
}
