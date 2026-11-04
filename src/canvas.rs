//! The page: title, paper, free-placed notes and the ink layer.

use crate::chrome::format_date;
use crate::*;
use eframe::egui::{Align2, FontId, Rect, RichText, Sense};

pub(crate) enum PageAction { Export, Copy, Delete, Duplicate, Go(usize, usize, usize), ToggleFav, Study, Pdf(bool) }

/// Whole-page PDF export in progress: the page is shown tile by tile at a fixed zoom and each
/// tile is photographed (so the PDF looks exactly like the page, maths and all).
pub(crate) struct Capture {
    pub page: u64,
    pub content: Rect,
    pub k: u32,
    pub tiles: Vec<image::RgbaImage>,
    pub wait: u8,
    pub rx: Option<std::sync::mpsc::Receiver<std::sync::Arc<egui::ColorImage>>>,
    pub path: std::path::PathBuf,
    pub title: String,
    pub keep: (bool, bool, bool),
    pub view: Option<Rect>,
}

impl Capture {
    /// Tile size in page units for a viewport of `avail` points: as wide as the content, up to 2× zoom.
    fn scale(&self, avail: Vec2) -> f32 { (avail.x / self.content.width()).clamp(0.4, 2.0) }
    pub fn done(&self) -> bool { self.wait == u8::MAX }
}

enum BlockAction { None, Changed, Delete, Duplicate }

pub(crate) fn page_view(ui: &mut egui::Ui, page: &mut Page, st: &mut St, crumb: &str, backlinks: &[(String, (usize, usize, usize))], fav: bool, study: &std::collections::HashMap<String, CardState>) -> Option<PageAction> {
    let mut action = None;
    // The page is a sheet of paper lying on the desk.
    let edge = |shown: bool| if shown { 0 } else { 14 };
    crate::card(ui).inner_margin(egui::Margin { left: 40, right: 12, top: 20, bottom: 0 })
        .outer_margin(egui::Margin { left: edge(st.p.show_nav), right: edge(st.p.show_ai), top: 0, bottom: 12 }).show(ui, |ui| {
        ui.label(RichText::new(crumb.to_uppercase()).size(10.5).color(accent(ui)).extra_letter_spacing(1.2));
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 60.0).clamp(160.0, 680.0);
            let r = ui.add(egui::TextEdit::singleline(&mut page.title).id(egui::Id::unique("page-title")).font(rich::serif(31.0))
                .text_color(ui.visuals().strong_text_color()).frame(egui::Frame::NONE).hint_text("Untitled page").desired_width(w).margin(egui::Margin::ZERO));
            if st.focus_id == Some(r.id) { r.request_focus(); st.focus_id = None; }
            if r.changed() { st.dirty = true; }
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && page.blocks.is_empty() {
                // Enter in an empty page's title drops straight into the first note
                let b = block(24.0, 8.0, st.p.note_width, Kind::Text { text: String::new(), heading: 0 });
                st.focus_id = Some(egui::Id::unique(("ed", b.id)));
                st.editing = Some(b.id);
                page.blocks.push(b);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let m = ui.add(egui::Button::new(RichText::new("⋯").size(18.0)).frame_when_inactive(false)).tip("Page actions");
                // outline: every heading on the page, click to jump there
                let mut heads: Vec<(f32, String)> = page.blocks.iter().flat_map(|b| match &b.kind {
                    Kind::Text { text, heading } if *heading > 0 => vec![(b.pos[1], text.lines().next().unwrap_or("").to_string())],
                    Kind::Text { text, .. } => text.lines().enumerate().filter(|(_, l)| matches!(rich::classify(l).0, rich::LineKind::Heading(_)))
                        .map(|(i, l)| (b.pos[1] + i as f32 * 22.0, l.trim_start_matches(['#', ' ']).to_string())).collect(),
                    _ => vec![],
                }).filter(|(_, t)| !t.trim().is_empty()).collect();
                heads.sort_by(|a, b| a.0.total_cmp(&b.0));
                if heads.len() >= 2 {
                    let o = ui.add(egui::Button::new(RichText::new("☰").size(16.0)).frame_when_inactive(false)).tip("Outline — jump to a heading");
                    egui::Popup::menu(&o).show(|ui| {
                        ui.label(RichText::new("ON THIS PAGE").size(10.5).weak());
                        for (y, t) in &heads {
                            if ui.button(t).clicked() {
                                let v = st.views.get(&page.id).copied().unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0)));
                                st.views.insert(page.id, v.translate(Vec2::new(-v.left(), y - 30.0 - v.top())));
                                ui.close();
                            }
                        }
                    });
                }
                let star = ui.add(egui::Button::new(RichText::new(if fav { "★" } else { "☆" }).size(18.0).color(if fav { Color32::from_rgb(234, 179, 8) } else { ui.visuals().weak_text_color() })).frame_when_inactive(false))
                    .tip(if fav { "Remove from favourites" } else { "Add to favourites" });
                if star.clicked() { action = Some(PageAction::ToggleFav); }
                egui::Popup::menu(&m).show(|ui| {
                    if ui.button("⧉  Duplicate page").clicked() { action = Some(PageAction::Duplicate); ui.close(); }
                    if ui.button("⎘  Copy as Markdown").clicked() { action = Some(PageAction::Copy); ui.close(); }
                    if crate::export::printout_source(page, &|id| st.heights.get(&id).copied()).is_some() {
                        if ui.button("⇩  Export printout with notes (PDF)…").tip("The original PDF pages, with your ink and typed notes on top").clicked() { action = Some(PageAction::Pdf(false)); ui.close(); }
                        if ui.button("⇩  Export whole page as PDF…").clicked() { action = Some(PageAction::Pdf(true)); ui.close(); }
                    } else if ui.button("⇩  Export as PDF…").tip("Just the part of the page you wrote on, with the title, on A4 pages").clicked() { action = Some(PageAction::Pdf(true)); ui.close(); }
                    if ui.button("⇪  Export as Markdown…").clicked() { action = Some(PageAction::Export); ui.close(); }
                    if page.kind == PageKind::Deck && ui.button("▶  Study this deck").clicked() { action = Some(PageAction::Study); ui.close(); }
                    ui.separator();
                    if ui.button(RichText::new("🗑  Delete page").color(Color32::from_rgb(200, 60, 50))).clicked() { action = Some(PageAction::Delete); ui.close(); }
                });
            });
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format_date(page.created, true)).size(12.0).weak());
            if !backlinks.is_empty() {
                ui.add_space(14.0);
                ui.label(RichText::new("↩ Linked from").size(12.0).weak());
                for (title, at) in backlinks {
                    let t = if title.trim().is_empty() { "Untitled page" } else { title.as_str() };
                    if ui.add(egui::Button::new(RichText::new(t).size(12.0).color(accent(ui))).corner_radius(10).min_size(Vec2::new(0.0, 22.0))).clicked() { action = Some(PageAction::Go(at.0, at.1, at.2)); }
                }
            }
        });
        if page.kind == PageKind::Deck {
            ui.add_space(14.0);
            if deck_view(ui, page, st, study) { action = Some(PageAction::Study); }
            return;
        }
        audio_bar(ui, page, st);
        ui.add_space(14.0);
        // A zoomable, pannable canvas: pinch or Ctrl+scroll zooms, scrolling or dragging empty paper pans.
        let avail = ui.available_size();
        let outer = Rect::from_min_size(ui.cursor().min, avail);
        let mut view = st.views.get(&page.id).copied().unwrap_or(Rect::from_min_size(Pos2::ZERO, avail));
        if let Some(c) = st.capture.as_mut().filter(|c| c.page == page.id && !c.done()) {
            // show tile k exactly, then photograph it once it has settled for a few frames
            let s = c.scale(avail);
            let th = avail.y / s;
            view = Rect::from_min_size(Pos2::new(c.content.left(), c.content.top() + c.k as f32 * th), avail / s);
            st.pan = Vec2::ZERO;
            st.zoom_req = None;
            let ctx = ui.ctx().clone();
            if let Some(img) = c.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
                let ppp = ctx.pixels_per_point();
                let remaining = (c.content.bottom() - view.top()) * s;
                let crop = Rect::from_min_size(outer.min, Vec2::new(outer.width(), outer.height().min(remaining))) ;
                let (x0, y0) = ((crop.left() * ppp) as u32, (crop.top() * ppp) as u32);
                let (w, h) = (((crop.width() * ppp) as u32).min(img.size[0] as u32 - x0), ((crop.height() * ppp) as u32).min(img.size[1] as u32 - y0));
                let full = image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, img.as_raw().to_vec());
                if let Some(full) = full { c.tiles.push(image::imageops::crop_imm(&full, x0, y0, w, h).to_image()); }
                c.rx = None;
                c.wait = 0;
                c.k += 1;
                if c.content.top() + c.k as f32 * th >= c.content.bottom() { c.wait = u8::MAX; }
            } else if c.rx.is_none() {
                c.wait += 1;
                if c.wait > 3 {
                    let (tx, rx) = std::sync::mpsc::channel();
                    ctx.request_screenshot(move |img| { let _ = tx.send(img); });
                    c.rx = Some(rx);
                }
            }
            ctx.request_repaint();
        }
        if st.pan != Vec2::ZERO { view = view.translate(-std::mem::take(&mut st.pan)); }
        if let Some(z) = st.zoom_req.take() {
            let cur = avail.x / view.width().max(1.0);
            let target = match z {
                ZoomReq::By(f) => cur * f,
                ZoomReq::To(t) => t,
                ZoomReq::FitWidth => {
                    let right = page.blocks.iter().map(|b| b.pos[0] + b.width).chain(page.strokes.iter().flat_map(|s| s.points.iter().map(|p| p[0]))).fold(400.0, f32::max);
                    view = Rect::from_min_size(Pos2::new(0.0, view.top()), view.size());
                    avail.x / (right + 40.0)
                }
            }.clamp(0.3, 4.0);
            view = Rect::from_min_size(view.min, avail / target);
        }
        egui::Scene::new().zoom_range(0.3..=4.0).max_inner_size(Vec2::splat(30000.0)).show(ui, &mut view, |ui| canvas(ui, page, st));
        // keep the paper from drifting away from the top-left corner
        if view.top() < -80.0 { view = view.translate(Vec2::new(0.0, -80.0 - view.top())); }
        if view.left() < -200.0 { view = view.translate(Vec2::new(-200.0 - view.left(), 0.0)); }
        st.zoom_now = avail.x / view.width().max(1.0);
        st.views.insert(page.id, view);
    });
    action
}

fn canvas(ui: &mut egui::Ui, page: &mut Page, st: &mut St) {
    let mut extent = Vec2::new(900.0, 700.0);
    for b in &page.blocks { extent = extent.max(Vec2::new(b.pos[0] + b.width, b.pos[1] + st.heights.get(&b.id).copied().unwrap_or(40.0)) + Vec2::splat(320.0)); }
    for s in &page.strokes { for p in &s.points { extent = extent.max(Vec2::new(p[0], p[1]) + Vec2::splat(320.0)); } }
    let view = st.views.get(&page.id).copied().unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 900.0)));
    let size = extent.max(view.max.to_vec2() + Vec2::splat(200.0));
    let (rect, bg) = ui.allocate_exact_size(size, Sense::click());
    let o = rect.min;
    let dark = ui.visuals().dark_mode;
    let line = if dark { Color32::from_gray(52) } else { Color32::from_rgb(220, 232, 240) };
    match page.paper {
        Paper::Plain => {}
        Paper::Ruled => {
            let mut y = o.y + 36.0;
            while y < rect.bottom() { ui.painter().hline(rect.x_range(), y, EStroke::new(1.0, line)); y += 28.0; }
            ui.painter().vline(o.x + 14.0, rect.y_range(), EStroke::new(1.0, if dark { Color32::from_rgb(90, 50, 50) } else { Color32::from_rgb(248, 200, 196) }));
        }
        Paper::Grid => {
            let mut y = o.y; while y < rect.bottom() { ui.painter().hline(rect.x_range(), y, EStroke::new(1.0, line)); y += 24.0; }
            let mut x = o.x; while x < rect.right() { ui.painter().vline(x, rect.y_range(), EStroke::new(1.0, line)); x += 24.0; }
        }
        // ruled, with a cue column on the left for questions and key words
        Paper::Cornell => {
            let mut y = o.y + 36.0;
            while y < rect.bottom() { ui.painter().hline(rect.x_range(), y, EStroke::new(1.0, line)); y += 28.0; }
            ui.painter().vline(o.x + 210.0, rect.y_range(), EStroke::new(1.5, if dark { Color32::from_rgb(90, 50, 50) } else { Color32::from_rgb(248, 200, 196) }));
        }
        // five-line staves
        Paper::Music => {
            let clip = ui.clip_rect();
            let staff = if dark { Color32::from_gray(78) } else { Color32::from_rgb(178, 196, 210) };
            let mut y = o.y + 48.0;
            while y < rect.bottom() {
                if y > clip.top() - 100.0 && y < clip.bottom() + 100.0 {
                    for k in 0..5 { ui.painter().hline(rect.x_range(), y + k as f32 * 9.0, EStroke::new(1.0, staff)); }
                }
                y += 100.0;
            }
        }
        Paper::Dots => {
            let clip = ui.clip_rect();
            let mut y = o.y + 12.0;
            while y < rect.bottom() {
                if y > clip.top() - 24.0 && y < clip.bottom() + 24.0 {
                    let mut x = o.x + 12.0;
                    while x < rect.right() { ui.painter().circle_filled(Pos2::new(x, y), 1.3, line.gamma_multiply(1.6)); x += 24.0; }
                }
                y += 24.0;
            }
        }
    }
    // Clicking empty paper starts a new note there, like writing anywhere on a page.
    // With a pen tool selected, only inputs allowed to draw make ink; the mouse (or a finger) keeps working as usual.
    // With finger drawing off, a stylus touching the page while typing picks up the pen in hand.
    // (only on devices whose fingers report no pressure, otherwise every tap would start drawing)
    let auto_pen = st.tool == Tool::Type && !st.p.draw_finger && st.p.pen_detect;
    if auto_pen && st.input == Input::Pen && ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(rect) {
        st.tool = Tool::Pen;
        st.editing = None;
        st.say("✏ Picked up your pen. Choose Type in the toolbar to type again.");
    }
    let makes_ink = matches!(st.tool, Tool::Pen | Tool::Highlighter | Tool::Shape(_));
    let allowed = match st.input { Input::Pen => true, Input::Finger => st.p.draw_finger, Input::Mouse => st.p.draw_mouse };
    // the ink layer also waits under the notes while typing, so that first stylus stroke draws straight away
    st.ink_pass = (makes_ink && !allowed) || st.tool == Tool::Type && auto_pen;
    // (the ink layer stays under the notes so a stylus touching empty paper still draws straight away)
    let paper_click = if st.ink_pass { ink_input(ui, rect, page, st) } else { None };
    let clicked = bg.clicked() || paper_click.is_some();
    let click_pos = paper_click.or_else(|| bg.interact_pointer_pos());
    let typing = st.tool == Tool::Type || st.ink_pass;
    if clicked && typing && st.listen && !page.recordings.is_empty() {
        // tap ink to hear what was said while it was written
        if let Some(p) = click_pos {
            let q = p - o.to_vec2();
            let hit = page.strokes.iter().filter(|s| s.points.iter().any(|v| (v[0] - q.x).hypot(v[1] - q.y) < 12.0)).map(|s| s.t).next();
            if let Some(t) = hit { jump_to(page, st, t); }
        }
    } else if clicked && typing && st.p.click_to_type {
        if let Some(p) = click_pos {
            let b = block((p.x - o.x - 4.0).max(0.0), (p.y - o.y - 16.0).max(0.0), st.p.note_width, Kind::Text { text: String::new(), heading: 0 });
            st.focus_id = Some(egui::Id::unique(("ed", b.id)));
            st.editing = Some(b.id);
            page.blocks.push(b);
        }
    }
    if page.blocks.is_empty() && page.strokes.is_empty() {
        ui.painter().text(o + Vec2::new(28.0, 24.0), Align2::LEFT_TOP, "Click anywhere to type, or pick the pen to write.", FontId::proportional(14.0), ui.visuals().weak_text_color());
    }
    let later = playback_moment(page, st);
    // Highlighter goes under the notes, like a real highlighter behind the words; pen ink goes on top.
    for s in page.strokes.iter().filter(|s| s.highlighter) { paint_stroke(&ui.painter_at(rect), s, o); }
    let mut remove = None;
    let mut dup = None;
    let mut changed = false;
    for b in page.blocks.iter_mut() {
        match block_ui(ui, b, st, o) {
            BlockAction::Delete => remove = Some(b.id),
            BlockAction::Duplicate => dup = Some(b.id),
            BlockAction::Changed => changed = true,
            BlockAction::None => {}
        }
    }
    if st.want_snap { st.want_snap = false; st.snap(page); }
    if let Some((after, kind)) = st.insert_below.take() {
        if let Some(b) = page.blocks.iter().find(|b| b.id == after) {
            let h = st.heights.get(&after).copied().unwrap_or(40.0);
            let nb = block(b.pos[0], b.pos[1] + h + 16.0, b.width, kind);
            st.focus_id = Some(egui::Id::unique(("ed", nb.id)));
            st.editing = Some(nb.id);
            st.snap(page);
            page.blocks.push(nb);
        }
    }
    if let Some(id) = remove {
        let empty_text = page.blocks.iter().any(|b| b.id == id && matches!(&b.kind, Kind::Text { text, .. } if text.trim().is_empty()));
        if !empty_text { st.snap(page); }
        page.blocks.retain(|b| b.id != id);
        changed = true;
    }
    if let Some(id) = dup {
        st.snap(page);
        if let Some(b) = page.blocks.iter().find(|b| b.id == id) {
            let h = st.heights.get(&id).copied().unwrap_or(40.0);
            let mut c = b.clone();
            c.id = new_id();
            c.pos[1] += h + 18.0;
            page.blocks.push(c);
        }
    }
    if changed { st.dirty = true; }

    // ink, drawn over the notes like ink on paper
    let painter = ui.painter_at(rect);
    for s in page.strokes.iter().filter(|s| !s.highlighter) {
        if later.as_ref().is_some_and(|f| f(s.t)) { let mut faded = s.clone(); faded.color[3] = (faded.color[3] as f32 * 0.22) as u8; paint_stroke(&painter, &faded, o); } else { paint_stroke(&painter, s, o); }
    }
    if let Some(s) = &st.current { paint_stroke(&painter, s, o); }
    if let Some(r) = selection_bounds(page, &st.selected).filter(|_| st.capture.is_none()) {
        let r = r.translate(o.to_vec2()).expand(6.0);
        let acc = accent(ui);
        for side in [[r.left_top(), r.right_top()], [r.right_top(), r.right_bottom()], [r.right_bottom(), r.left_bottom()], [r.left_bottom(), r.left_top()]] {
            painter.add(egui::Shape::dashed_line(&side, EStroke::new(1.2, acc), 5.0, 4.0));
        }
        painter.text(r.left_top() - Vec2::new(0.0, 4.0), Align2::LEFT_BOTTOM, format!("{} selected · drag to move, corner to resize, knob to rotate", st.selected.len()), FontId::proportional(11.0), acc);
    }
    if st.lasso.len() > 1 {
        painter.add(egui::Shape::dashed_line(&st.lasso.iter().map(|p| *p + o.to_vec2()).collect::<Vec<_>>(), EStroke::new(1.5, accent(ui)), 6.0, 4.0));
    }
    if st.tool != Tool::Type && !st.ink_pass { ink_input(ui, rect, page, st); }
    if st.tool == Tool::Lasso { selection_handles(ui, o, page, st); }
}

/// Handles on the selected ink: bottom-right corner resizes, the knob above rotates.
fn selection_handles(ui: &mut egui::Ui, o: Pos2, page: &mut Page, st: &mut St) {
    let Some(r) = selection_bounds(page, &st.selected) else { return };
    let r = r.expand(6.0);
    let acc = accent(ui);
    let (corner, knob) = (r.right_bottom(), r.center_top() - Vec2::new(0.0, 24.0));
    let painter = ui.painter();
    painter.line_segment([o + r.center_top().to_vec2(), o + knob.to_vec2()], EStroke::new(1.2, acc));
    for (c, round) in [(corner, false), (knob, true)] {
        let c = o + c.to_vec2();
        if round { painter.circle(c, 6.0, Color32::WHITE, EStroke::new(1.5, acc)); } else { painter.rect(Rect::from_center_size(c, Vec2::splat(11.0)), 2.0, Color32::WHITE, EStroke::new(1.5, acc), egui::StrokeKind::Middle); }
    }
    for (kind, c) in [(0u8, corner), (1u8, knob)] {
        let resp = ui.interact(Rect::from_center_size(o + c.to_vec2(), Vec2::splat(22.0)), ui.scope_id().with(("sel-handle", kind)), Sense::drag());
        if resp.hovered() || resp.dragged() { ui.ctx().set_cursor_icon(if kind == 0 { egui::CursorIcon::ResizeNwSe } else { egui::CursorIcon::Grab }); }
        if resp.drag_started() { st.snap(page); st.xform = Some((kind, if kind == 0 { r.left_top() + Vec2::splat(6.0) } else { r.center() })); }
        if resp.drag_stopped() { st.xform = None; }
        let (Some((k, pivot)), Some(pos)) = (st.xform, resp.interact_pointer_pos()) else { continue };
        if k != kind || !resp.dragged() { continue; }
        let p = pos - o.to_vec2();
        let prev = p - resp.drag_delta();
        let sel = st.selected.clone();
        if k == 0 {
            // uniform resize about the top-left corner
            let f = ((p - pivot).length() / (prev - pivot).length().max(1.0)).clamp(0.5, 2.0);
            if (prev - pivot).length() < 4.0 { continue; }
            for s in page.strokes.iter_mut().filter(|s| sel.contains(&s.id)) {
                for q in &mut s.points { q[0] = pivot.x + (q[0] - pivot.x) * f; q[1] = pivot.y + (q[1] - pivot.y) * f; }
                s.width = (s.width * f).clamp(0.3, 60.0);
            }
        } else {
            // rotate about the selection's centre
            let a = (p - pivot).angle() - (prev - pivot).angle();
            let (sn, cs) = a.sin_cos();
            for s in page.strokes.iter_mut().filter(|s| sel.contains(&s.id)) {
                for q in &mut s.points { let (dx, dy) = (q[0] - pivot.x, q[1] - pivot.y); q[0] = pivot.x + dx * cs - dy * sn; q[1] = pivot.y + dx * sn + dy * cs; }
            }
        }
        st.dirty = true;
    }
}

fn selection_bounds(page: &Page, sel: &[u64]) -> Option<Rect> {
    if sel.is_empty() { return None; }
    let pts: Vec<Pos2> = page.strokes.iter().filter(|s| sel.contains(&s.id)).flat_map(|s| s.points.iter().map(|p| Pos2::new(p[0], p[1]))).collect();
    (!pts.is_empty()).then(|| Rect::from_points(&pts))
}

pub(crate) fn paint_stroke(painter: &egui::Painter, s: &Stroke, o: Pos2) {
    let col = c4(s.color);
    let pts: Vec<Pos2> = s.points.iter().map(|p| Pos2::new(o.x + p[0], o.y + p[1])).collect();
    let w0 = seg_width(s, s.points[0][2], 1.0, 0.0);
    if pts.len() == 1 { painter.circle_filled(pts[0], w0 * 0.5, col); return; }
    let even = s.highlighter || s.pen == PenKind::Marker || (s.pen == PenKind::Ballpoint && s.points.iter().all(|p| (p[2] - s.points[0][2]).abs() < 0.05));
    if even {
        painter.add(egui::Shape::line(pts, EStroke::new(w0, col)));
        return;
    }
    if s.pen == PenKind::Calligraphy { paint_calligraphy(painter, s, &pts, col); return; }
    for (i, w) in pts.windows(2).enumerate() {
        let (dx, dy) = (w[1].x - w[0].x, w[1].y - w[0].y);
        let pw = seg_width(s, (s.points[i][2] + s.points[i + 1][2]) * 0.5, dx, dy);
        match s.pen {
            PenKind::Calligraphy => unreachable!("drawn whole by paint_calligraphy"),
            PenKind::Pencil => {
                // graphite grain: each bit of the line takes a little more or less colour
                let grain = 0.5 + 0.45 * (((i as u32).wrapping_mul(2_654_435_761) >> 13) & 255) as f32 / 255.0;
                let c = col.gamma_multiply(grain * 0.9);
                painter.line_segment([w[0], w[1]], EStroke::new(pw, c));
                painter.circle_filled(w[1], pw * 0.45, c);
            }
            _ => {
                painter.line_segment([w[0], w[1]], EStroke::new(pw, col));
                painter.circle_filled(w[1], pw * 0.5, col);
            }
        }
    }
}

/// A flat nib held at 45°, swept along the stroke: one solid shape (no seams between pieces),
/// with its two long edges traced by thin lines for smooth anti-aliased borders.
fn paint_calligraphy(painter: &egui::Painter, s: &Stroke, pts: &[Pos2], col: Color32) {
    let dir = Vec2::new(1.0, -1.0).normalized();
    let half = |i: usize| dir * (s.width * 0.9 * (0.6 + 0.4 * s.points[i][2].clamp(0.0, 1.0)));
    let upper: Vec<Pos2> = pts.iter().enumerate().map(|(i, p)| *p + half(i)).collect();
    let lower: Vec<Pos2> = pts.iter().enumerate().map(|(i, p)| *p - half(i)).collect();
    let mut mesh = egui::Mesh::default();
    for i in 0..pts.len() {
        mesh.colored_vertex(upper[i], col);
        mesh.colored_vertex(lower[i], col);
    }
    for i in 0..pts.len() as u32 - 1 {
        let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        mesh.add_triangle(a, b, c);
        mesh.add_triangle(b, d, c);
    }
    painter.add(egui::Shape::mesh(mesh));
    let edge = EStroke::new(1.0, col);
    painter.add(egui::Shape::line(upper.clone(), edge));
    painter.add(egui::Shape::line(lower.clone(), edge));
    painter.line_segment([upper[0], lower[0]], edge);
    painter.line_segment([*upper.last().unwrap(), *lower.last().unwrap()], edge);
}

/// The live part of smoothing: each new point trails the pen a little (stronger smoothing, more trail).
pub(crate) fn trail(last: [f32; 3], x: f32, y: f32, stabilizer: f32) -> (f32, f32) {
    let k = 1.0 - stabilizer.clamp(0.0, 1.0) * 0.75;
    (last[0] + (x - last[0]) * k, last[1] + (y - last[1]) * k)
}

/// The finishing part of smoothing, applied when the pen lifts (more passes for stronger smoothing).
pub(crate) fn smooth_passes(points: &mut [[f32; 3]], stabilizer: f32) {
    if points.len() < 3 { return; }
    for _ in 0..1 + (stabilizer * 3.0).round() as usize {
        let src = points.to_vec();
        for i in 1..src.len() - 1 { for k in 0..3 { points[i][k] = (src[i - 1][k] + 2.0 * src[i][k] + src[i + 1][k]) / 4.0; } }
    }
}

/// Line width for this pen at pressure `p`, moving in direction (dx, dy).
pub(crate) fn seg_width(s: &Stroke, p: f32, dx: f32, dy: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    if s.highlighter { return s.width; }
    match s.pen {
        PenKind::Fountain => s.width * (0.35 + 0.65 * p),
        PenKind::Ballpoint => s.width * (0.8 + 0.2 * p),
        PenKind::Pencil => s.width * (0.55 + 0.45 * p),
        PenKind::Calligraphy => s.width * 1.8 * (0.18 + 0.82 * (dy.atan2(dx) - std::f32::consts::FRAC_PI_4).sin().abs()),
        PenKind::Marker => s.width * 1.6,
    }
}

fn point_in_poly(p: Pos2, poly: &[Pos2]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x { inside = !inside; }
        j = i;
    }
    inside
}

/// Pen, highlighter, shapes, eraser and lasso. In pass-through mode (a pen tool is selected but this input
/// doesn't draw) it only moves the page and reports a click on empty paper.
fn ink_input(ui: &mut egui::Ui, rect: Rect, page: &mut Page, st: &mut St) -> Option<Pos2> {
    // Two fingers scroll the page even while a pen tool is active.
    let from_global = ui.ctx().layer_transform_from_global(ui.layer_id()).unwrap_or_default();
    if let Some(mt) = ui.input(|i| i.multi_touch()) {
        st.current = None;
        st.lasso.clear();
        st.pan += mt.translation_delta * from_global.scaling;
        return None;
    }
    let resp = ui.interact(rect, ui.scope_id().with("ink"), if st.ink_pass { Sense::click_and_drag() } else { Sense::drag() });
    let o = rect.min;
    if st.ink_pass {
        // the mouse (or a finger) keeps working as usual: dragging moves the page, clicking works like typing mode
        if resp.dragged() { st.pan += resp.drag_delta(); }
        if resp.drag_started() && st.input == Input::Mouse && !st.mouse_hint && st.tool != Tool::Type {
            st.mouse_hint = true;
            st.say("Tip: your stylus draws and the mouse works as usual. To draw with the mouse too, switch it on in Settings ▸ Pen & input.");
        }
        return resp.clicked().then(|| resp.interact_pointer_pos()).flatten();
    }
    let shift = ui.input(|i| i.modifiers.shift);
    if let Some(pos) = resp.interact_pointer_pos() {
        let p = pos - o.to_vec2();
        if resp.drag_started() {
            match st.tool {
                Tool::Pen | Tool::Highlighter => {
                    let hl = st.tool == Tool::Highlighter;
                    let start = ui.input(|i| i.pointer.press_origin()).map_or(p, |q| from_global * q - o.to_vec2());
                    let pen = *st.pen();
                    st.snapped = false;
                    st.still = (start, ui.input(|i| i.time));
                    st.current = Some(Stroke { id: new_id(), points: vec![[start.x, start.y, if st.p.pressure { st.frame_force.unwrap_or(0.8) } else { 0.85 }]],
                        color: if hl { st.p.hl_color } else { pen.color }, width: if hl { 16.0 } else { pen.width }, highlighter: hl, pen: pen.kind, t: now_f() });
                }
                Tool::Eraser => st.snap(page),
                Tool::Lasso => {
                    if selection_bounds(page, &st.selected).is_some_and(|r| r.expand(8.0).contains(p)) { st.moving = true; st.snap(page); }
                    else { st.lasso = vec![p]; st.selected.clear(); }
                }
                Tool::Shape(_) => {
                    st.anchor = Some([p.x, p.y]);
                    let pen = *st.pen();
                    st.current = Some(Stroke { id: new_id(), points: vec![], color: pen.color, width: pen.width.max(1.5), highlighter: false, pen: PenKind::Ballpoint, t: now_f() });
                }
                Tool::Type => {}
            }
        }
        if resp.dragged() {
            match st.tool {
                Tool::Pen | Tool::Highlighter => if let Some(s) = &mut st.current {
                    // Draw, then hold still: the rough shape snaps to a perfect one.
                    let now = ui.input(|i| i.time);
                    if (p - st.still.0).length() > 3.0 / from_global.scaling.max(0.01) { st.still = (p, now); }
                    if !st.snapped && !s.highlighter && now - st.still.1 > 0.55 && s.points.len() > 8 {
                        if let Some((pts, kind)) = snap_shape(&s.points) {
                            s.points = pts;
                            st.snapped = true;
                            st.status = format!("Snapped to a {kind}."); st.status_age = 0.0;
                        }
                        st.still.1 = now;
                    }
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    let last = s.points.last().copied().unwrap_or([p.x, p.y, 0.8]);
                    let d = (last[0] - p.x).hypot(last[1] - p.y);
                    if d > 1.0 && !st.snapped {
                        // Real pen pressure when the device reports it; otherwise slow strokes are a bit bolder, like a fountain pen.
                        let pr = if !st.p.pressure { 0.85 } else { st.frame_force.unwrap_or_else(|| last[2] * 0.75 + (1.1 - d / 28.0).clamp(0.45, 1.0) * 0.25) };
                        // the stabilizer lets the line trail the pen slightly, which irons out shaky hands
                        let (x, y) = trail(last, p.x, p.y, st.p.stabilizer);
                        s.points.push([x, y, pr]);
                    }
                },
                Tool::Eraser => {
                    let hit = if st.p.erase_partial { erase_partial(&mut page.strokes, [p.x, p.y], 7.0) } else {
                        let before = page.strokes.len();
                        page.strokes.retain(|s| !s.points.iter().any(|q| (q[0] - p.x).hypot(q[1] - p.y) < 9.0 + s.width * 0.5));
                        page.strokes.len() != before
                    };
                    if hit { st.dirty = true; }
                }
                Tool::Lasso => if st.moving {
                    let d = resp.drag_delta();
                    for s in page.strokes.iter_mut().filter(|s| st.selected.contains(&s.id)) { for q in &mut s.points { q[0] += d.x; q[1] += d.y; } }
                } else { st.lasso.push(p) },
                Tool::Shape(shape) => if let (Some(a), Some(cur)) = (st.anchor, &mut st.current) {
                    let mut b = [p.x, p.y];
                    if shift {
                        // Shift keeps lines at 45° steps and boxes/circles even.
                        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                        if matches!(shape, Shape::Rect | Shape::Ellipse) { let m = dx.abs().max(dy.abs()); b = [a[0] + m * dx.signum(), a[1] + m * dy.signum()]; }
                        else { let ang = (dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4; let len = dx.hypot(dy); b = [a[0] + len * ang.cos(), a[1] + len * ang.sin()]; }
                    }
                    cur.points = shape_points(shape, a, b);
                },
                Tool::Type => {}
            }
        }
    }
    if resp.drag_stopped() {
        match st.tool {
            Tool::Pen | Tool::Highlighter | Tool::Shape(_) => if let Some(mut s) = st.current.take() {
                if s.highlighter && st.p.straighten_hl && s.points.len() > 1 {
                    // Highlighter strokes that are nearly straight snap straight, like a ruler.
                    let (a, b) = (s.points[0], *s.points.last().unwrap());
                    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                    let off = s.points.iter().map(|q| ((b[1] - a[1]) * q[0] - (b[0] - a[0]) * q[1] + b[0] * a[1] - b[1] * a[0]).abs() / len.max(1.0)).fold(0.0, f32::max);
                    if len > 40.0 && off < 6.0 { s.points = vec![a, b]; }
                } else if !matches!(st.tool, Tool::Shape(_)) && !st.snapped && s.points.len() > 3 {
                    // finish where the pen lifted, then smooth (more passes for a stronger stabilizer)
                    if let Some(end) = resp.interact_pointer_pos().or(ui.input(|i| i.pointer.latest_pos()).map(|q| from_global * q)) {
                        let e = end - o.to_vec2();
                        let last = *s.points.last().unwrap();
                        s.points.push([e.x, e.y, last[2]]);
                    }
                    smooth_passes(&mut s.points, st.p.stabilizer);
                }
                if !s.points.is_empty() { st.snap(page); page.strokes.push(s); }
                st.anchor = None;
            },
            Tool::Lasso => {
                if st.moving { st.moving = false; }
                else if st.lasso.len() > 2 {
                    let poly = std::mem::take(&mut st.lasso);
                    st.selected = page.strokes.iter().filter(|s| {
                        let inside = s.points.iter().filter(|q| point_in_poly(Pos2::new(q[0], q[1]), &poly)).count();
                        inside * 2 > s.points.len()
                    }).map(|s| s.id).collect();
                    st.say(if st.selected.is_empty() { "Nothing selected — draw a loop around the ink.".to_string() } else { format!("{} strokes selected — drag to move, or convert them.", st.selected.len()) });
                } else { st.selected.clear(); }
                st.lasso.clear();
            }
            _ => {}
        }
    }
    if let Some(h) = resp.hover_pos().filter(|_| resp.hovered() || resp.dragged()) {
        let painter = ui.painter_at(rect);
        match st.tool {
            Tool::Pen => { let pen = *st.pen(); ui.ctx().set_cursor_icon(egui::CursorIcon::None); painter.circle_filled(h, (pen.width * 0.5).max(1.5), c4(pen.color)); }
            Tool::Highlighter => { ui.ctx().set_cursor_icon(egui::CursorIcon::None); painter.rect_filled(Rect::from_center_size(h, Vec2::new(5.0, 16.0)), 1.0, c4(st.p.hl_color)); }
            Tool::Eraser => { ui.ctx().set_cursor_icon(egui::CursorIcon::None); painter.circle_stroke(h, 8.0, EStroke::new(1.2, ui.visuals().text_color())); }
            Tool::Lasso if selection_bounds(page, &st.selected).is_some_and(|r| r.expand(8.0).contains(h - o.to_vec2())) => ui.ctx().set_cursor_icon(egui::CursorIcon::Move),
            _ => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
        }
    }
    None
}

#[derive(Clone, Copy)]
enum BarIcon { Grip, Resize, More, Delete }

/// Small vector icons for the note toolbar, drawn centred in their button.
fn bar_icon(p: &egui::Painter, c: Pos2, icon: BarIcon, col: Color32) {
    let s = EStroke::new(1.6, col);
    match icon {
        BarIcon::Grip => for dx in [-3.0, 3.0] { for dy in [-5.0, 0.0, 5.0] { p.circle_filled(c + Vec2::new(dx, dy), 1.5, col); } },
        BarIcon::More => for dx in [-5.0, 0.0, 5.0] { p.circle_filled(c + Vec2::new(dx, 0.0), 1.7, col); },
        BarIcon::Delete => { p.line_segment([c + Vec2::new(-4.5, -4.5), c + Vec2::new(4.5, 4.5)], s); p.line_segment([c + Vec2::new(-4.5, 4.5), c + Vec2::new(4.5, -4.5)], s); }
        BarIcon::Resize => {
            p.line_segment([c + Vec2::new(-7.0, 0.0), c + Vec2::new(7.0, 0.0)], s);
            for d in [-1.0f32, 1.0] { let tip = c + Vec2::new(7.0 * d, 0.0); p.line_segment([tip, tip + Vec2::new(-3.5 * d, -3.5)], s); p.line_segment([tip, tip + Vec2::new(-3.5 * d, 3.5)], s); }
        }
    }
}

fn block_ui(ui: &mut egui::Ui, b: &mut Block, st: &mut St, o: Pos2) -> BlockAction {
    let mut act = BlockAction::None;
    let h = st.heights.get(&b.id).copied().unwrap_or(40.0);
    let outer = Rect::from_min_size(o + Vec2::new(b.pos[0], b.pos[1]), Vec2::new(b.width, h));
    // a small floating toolbar above the note's top-right corner: move · resize · more · delete
    let (bw, bh) = (30.0, 26.0);
    let mut bar = Rect::from_min_size(Pos2::new(outer.right() - 4.0 * bw - 6.0, outer.top() - bh - 6.0), Vec2::new(4.0 * bw + 6.0, bh));
    // notes at the top of the view keep their toolbar on screen
    let top = ui.clip_rect().top() + 2.0;
    if bar.top() < top { bar = bar.translate(Vec2::new(0.0, top - bar.top())); }
    // Background catcher so clicks inside a note don't create a new note behind it.
    let body = ui.interact(outer, ui.scope_id().with(("blockbg", b.id)), Sense::click());
    let typing = st.tool == Tool::Type || st.ink_pass;
    let active = st.editing == Some(b.id);
    let hovered = typing && st.capture.is_none() && ui.rect_contains_pointer(outer.union(bar).expand(8.0));
    let acc = accent(ui);
    let v = ui.visuals().clone();
    let mut menu_from: Option<egui::Response> = None;
    if (hovered || active) && st.capture.is_none() {
        ui.painter().rect_stroke(outer.expand(4.0), 8.0, EStroke::new(1.0, if active { acc.gamma_multiply(0.7) } else { v.widgets.noninteractive.bg_stroke.color }), egui::StrokeKind::Outside);
        ui.painter().rect(bar.translate(Vec2::new(0.0, 1.5)), 9.0, Color32::from_black_alpha(if v.dark_mode { 60 } else { 14 }), EStroke::NONE, egui::StrokeKind::Inside);
        ui.painter().rect(bar, 9.0, v.extreme_bg_color, v.widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
        let slot = |k: usize| Rect::from_min_size(Pos2::new(bar.left() + 3.0 + k as f32 * bw, bar.top() + 2.0), Vec2::new(bw, bh - 4.0));
        let button = |ui: &mut egui::Ui, k: usize, icon: BarIcon, salt: &str, sense: Sense, tip: &str| {
            let r = slot(k);
            let resp = ui.interact(r, ui.scope_id().with((salt, b.id)), sense);
            if resp.hovered() || resp.dragged() { ui.painter().rect_filled(r.shrink(1.0), 6.0, v.widgets.hovered.weak_bg_fill); }
            let col = if matches!(icon, BarIcon::Delete) && resp.hovered() { Color32::from_rgb(214, 70, 60) } else if resp.hovered() || resp.dragged() { acc } else { v.text_color() };
            bar_icon(ui.painter(), r.center(), icon, col);
            resp.tip(tip)
        };
        let g = button(ui, 0, BarIcon::Grip, "grip", Sense::drag(), "Drag to move");
        if g.dragged() { let d = g.drag_delta(); b.pos[0] = (b.pos[0] + d.x).max(0.0); b.pos[1] = (b.pos[1] + d.y).max(0.0); act = BlockAction::Changed; }
        if g.hovered() || g.dragged() { ui.ctx().set_cursor_icon(if g.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab }); }
        let rz = button(ui, 1, BarIcon::Resize, "rz", Sense::drag(), "Drag to make wider or narrower");
        if rz.dragged() { b.width = (b.width + rz.drag_delta().x).clamp(120.0, 1600.0); act = BlockAction::Changed; }
        if rz.hovered() || rz.dragged() { ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal); }
        let more = button(ui, 2, BarIcon::More, "more", Sense::click(), "More");
        let del = button(ui, 3, BarIcon::Delete, "del", Sense::click(), "Delete this note");
        if del.clicked() { return BlockAction::Delete; }
        menu_from = Some(more);
    }
    if let Some(m) = &menu_from { egui::Popup::menu(m).show(|ui| block_menu(ui, b, &mut act)); }
    body.context_menu(|ui| block_menu(ui, b, &mut act));
    if body.clicked() && st.listen && st.recordings_span.is_some_and(|(a, z)| b.t >= a && b.t <= z) {
        st.jump = Some(b.t);
    } else if body.clicked() {
        st.editing = Some(b.id);
        st.focus_id = Some(egui::Id::unique(("ed", b.id)));
        if matches!(b.kind, Kind::Text { .. }) { st.last_text = Some(b.id); }
    }
    let inner_rect = Rect::from_min_size(outer.min, Vec2::new(b.width, f32::INFINITY));
    let faded = st.fade_after.is_some_and(|(now, a, z)| b.t > now && b.t >= a && b.t <= z);
    let res = ui.scope_builder(egui::UiBuilder::new().max_rect(inner_rect).id_salt(("block", b.id)), |ui| {
        ui.set_width(b.width);
        if faded { ui.multiply_opacity(0.25); }
        ui.horizontal_top(|ui| {
            match b.tag {
                Tag::Todo | Tag::Done => {
                    let mut done = b.tag == Tag::Done;
                    if ui.checkbox(&mut done, "").tip("To Do").changed() { b.tag = if done { Tag::Done } else { Tag::Todo }; act = BlockAction::Changed; }
                }
                Tag::Important => { ui.label(RichText::new("★").size(16.0).color(Color32::from_rgb(234, 179, 8))); }
                Tag::Question => { ui.label(RichText::new("?").size(16.0).strong().color(Color32::from_rgb(124, 58, 237))); }
                Tag::None => {}
            }
            ui.vertical(|ui| if block_body(ui, b, st, hovered || active) { if matches!(act, BlockAction::None) { act = BlockAction::Changed; } });
        });
    });
    st.heights.insert(b.id, res.response.rect.height());
    if matches!(act, BlockAction::None) {
        if let Kind::Text { text, .. } = &b.kind {
            let fid = egui::Id::unique(("ed", b.id));
            if text.trim().is_empty() && st.editing != Some(b.id) && st.focus_id != Some(fid) && !ui.memory(|m| m.has_focus(fid)) { return BlockAction::Delete; }
        }
    }
    act
}

fn block_menu(ui: &mut egui::Ui, b: &mut Block, act: &mut BlockAction) {
    let textual = matches!(b.kind, Kind::Text { .. } | Kind::Math { .. } | Kind::Code { .. } | Kind::Checklist { .. });
    if ui.button("⧉  Duplicate").clicked() { *act = BlockAction::Duplicate; ui.close(); }
    match &b.kind {
        Kind::Image { file } => { if ui.button("↗  Open picture").clicked() { open_path(&data_dir().join("files").join(file)); ui.close(); } }
        Kind::File { file, .. } => { if ui.button("↗  Open file").clicked() { open_path(&data_dir().join("files").join(file)); ui.close(); } }
        Kind::Link { url, .. } => { if ui.button("⎘  Copy address").clicked() { ui.ctx().copy_text(url.clone()); ui.close(); } }
        Kind::Math { src } => { if ui.button("⎘  Copy LaTeX").clicked() { ui.ctx().copy_text(src.clone()); ui.close(); } }
        Kind::Table { .. } => { if ui.button("⎘  Copy as text").clicked() { ui.ctx().copy_text(b.kind.summary()); ui.close(); } }
        _ => { if ui.button("⎘  Copy text").clicked() { ui.ctx().copy_text(match &b.kind { Kind::Text { text, .. } => text.clone(), Kind::Code { code, .. } => code.clone(), k => k.summary() }); ui.close(); } }
    }
    ui.menu_button("🏷  Tag", |ui| {
        for (t, label) in [(Tag::Todo, "☐ To Do"), (Tag::Important, "★ Important"), (Tag::Question, "? Question"), (Tag::None, "No tag")] {
            if ui.button(label).clicked() { b.tag = t; *act = BlockAction::Changed; ui.close(); }
        }
    });
    // only notes made of text can sensibly become another kind of text note
    if textual {
        let conv: Option<Kind> = ui.menu_button("⇄  Turn into", |ui| {
            let mut out = None;
            let text = match &b.kind {
                Kind::Text { text, .. } => text.clone(),
                Kind::Math { src } => src.clone(),
                Kind::Code { code, .. } => code.clone(),
                Kind::Checklist { items } => items.iter().map(|i| i.1.clone()).collect::<Vec<_>>().join("\n"),
                other => other.summary(),
            };
            if !matches!(b.kind, Kind::Text { .. }) && ui.button("Text").clicked() { out = Some(Kind::Text { text: text.clone(), heading: 0 }); }
            if !matches!(b.kind, Kind::Math { .. }) && ui.button("Equation").clicked() { out = Some(Kind::Math { src: text.clone() }); }
            if !matches!(b.kind, Kind::Checklist { .. }) && ui.button("Checklist").clicked() {
                out = Some(Kind::Checklist { items: text.lines().filter(|l| !l.trim().is_empty()).map(|l| (false, l.trim_start_matches(['-', '*', ' ']).to_string())).collect() });
            }
            if !matches!(b.kind, Kind::Code { .. }) && ui.button("Code").clicked() { out = Some(Kind::Code { lang: "text".into(), code: text }); }
            if out.is_some() { ui.close(); }
            out
        }).inner.flatten();
        if let Some(k) = conv { b.kind = k; *act = BlockAction::Changed; }
    }
    ui.separator();
    if ui.button(RichText::new("🗑  Delete").color(Color32::from_rgb(200, 60, 50))).clicked() { *act = BlockAction::Delete; ui.close(); }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum PopKind { Slash, Link }

/// An open "/" menu or "[[" page list: which note, where it started (char index), the highlighted row.
#[derive(Clone, Debug)]
pub(crate) struct Popup { pub block: u64, pub start: usize, pub kind: PopKind, pub sel: usize }

enum Act { Prefix(&'static str), Insert(&'static str), Block(u8), Date, Link(String) }

/// What the person typed after "/" or "[[" so far.
fn popup_query(ctx: &egui::Context, id: egui::Id, text: &str, pp: &Popup) -> String {
    let cur = egui::text_edit::TextEditState::load(ctx, id).and_then(|s| s.cursor.char_range()).map_or(text.chars().count(), |r| r.primary.index.0);
    let from = pp.start + if pp.kind == PopKind::Slash { 1 } else { 2 };
    text.chars().skip(from).take(cur.saturating_sub(from)).collect()
}

fn popup_items(pp: &Popup, q: &str, titles: &[String]) -> Vec<(String, &'static str, Act)> {
    let ql = q.to_lowercase();
    match pp.kind {
        PopKind::Slash => {
            let all: Vec<(&str, &'static str, Act)> = vec![
                ("Heading 1", "H1", Act::Prefix("# ")), ("Heading 2", "H2", Act::Prefix("## ")), ("Heading 3", "H3", Act::Prefix("### ")),
                ("Bulleted list", "•", Act::Prefix("- ")), ("Numbered list", "1.", Act::Prefix("1. ")), ("To-do", "☐", Act::Prefix("[ ] ")),
                ("Quote", "❝", Act::Prefix("> ")), ("Inline math", "$x$", Act::Insert("$|$")), ("Link to a page", "↗", Act::Insert("[[|")),
                ("Date and time", "🕓", Act::Date), ("Equation", "∑", Act::Block(0)), ("Table", "▦", Act::Block(1)),
                ("Checklist", "☑", Act::Block(2)), ("Code", "{ }", Act::Block(3)),
            ];
            all.into_iter().filter(|(l, ..)| l.to_lowercase().contains(&ql)).map(|(l, i, a)| (l.to_string(), i, a)).collect()
        }
        PopKind::Link => {
            let mut v: Vec<(String, &'static str, Act)> = titles.iter().filter(|t| t.to_lowercase().contains(&ql)).take(7).map(|t| (t.clone(), "↗", Act::Link(t.clone()))).collect();
            let q = q.trim();
            if !q.is_empty() && !titles.iter().any(|t| t.eq_ignore_ascii_case(q)) { v.push((format!("New page “{q}”"), "+", Act::Link(q.to_string()))); }
            v
        }
    }
}

fn set_cursor(ctx: &egui::Context, id: egui::Id, c: usize) {
    use egui::text::{CCursor, CCursorRange};
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
    state.store(ctx, id);
}

/// Carry out row `i` of the open menu. Returns true if the text changed.
fn apply_popup(ctx: &egui::Context, id: egui::Id, block_id: u64, text: &mut String, i: usize, st: &mut St) -> bool {
    let Some(pp) = st.popup.take() else { return false };
    let q = popup_query(ctx, id, text, &pp);
    let items = popup_items(&pp, &q, &st.page_titles);
    let Some((_, _, act)) = items.into_iter().nth(i) else { return false };
    let byte = |t: &str, c: usize| t.char_indices().nth(c).map_or(t.len(), |(b, _)| b);
    // remove the "/query" (or the query after "[[")
    let from = if pp.kind == PopKind::Link { pp.start + 2 } else { pp.start };
    let to = pp.start + if pp.kind == PopKind::Slash { 1 } else { 2 } + q.chars().count();
    let (fb, tb) = (byte(text, from), byte(text, to));
    text.replace_range(fb..tb, "");
    let mut cur = from;
    match act {
        Act::Prefix(p) => { cur = rich::toggle_prefix(text, from, from, p).0; }
        Act::Insert(s) => {
            let clean = s.replace('|', "");
            text.insert_str(byte(text, from), &clean);
            cur = from + s.find('|').map_or(clean.chars().count(), |b| s[..b].chars().count());
            if s.starts_with("[[") { st.popup = Some(Popup { block: block_id, start: from, kind: PopKind::Link, sel: 0 }); }
        }
        Act::Date => { let d = crate::chrome::format_date(now_secs(), true); text.insert_str(byte(text, from), &d); cur = from + d.chars().count(); }
        Act::Block(k) => {
            let kind = match k {
                0 => Kind::Math { src: String::new() },
                1 => Kind::Table { rows: vec![vec![String::new(); 3]; 3] },
                2 => Kind::Checklist { items: vec![(false, String::new())] },
                _ => Kind::Code { lang: "text".into(), code: String::new() },
            };
            st.insert_below = Some((block_id, kind));
        }
        Act::Link(title) => { let ins = format!("{title}]]"); text.insert_str(byte(text, from), &ins); cur = from + ins.chars().count(); }
    }
    set_cursor(ctx, id, cur);
    if st.insert_below.is_none() { st.focus_id = Some(id); st.editing = Some(block_id); }
    true
}

/// Open, update, close and draw the "/" menu or "[[" page list. Returns a clicked row.
fn popup_ui(ui: &mut egui::Ui, ctx: &egui::Context, id: egui::Id, block_id: u64, text: &str, out: &egui::text_edit::TextEditOutput, st: &mut St) -> Option<usize> {
    let cur = out.cursor_range.map(|r| r.primary.index.0)?;
    let chars: Vec<char> = text.chars().collect();
    if out.response.changed() && st.popup.as_ref().is_none_or(|pp| pp.block != block_id) {
        // "/" at the start of a line or after a space opens the menu; "[[" opens the page list
        if cur >= 1 && chars[cur - 1] == '/' && (cur == 1 || chars[cur - 2].is_whitespace()) {
            st.popup = Some(Popup { block: block_id, start: cur - 1, kind: PopKind::Slash, sel: 0 });
        } else if cur >= 2 && chars[cur - 2] == '[' && chars[cur - 1] == '[' {
            st.popup = Some(Popup { block: block_id, start: cur - 2, kind: PopKind::Link, sel: 0 });
        }
    }
    let pp = st.popup.clone().filter(|pp| pp.block == block_id)?;
    let q = popup_query(ctx, id, text, &pp);
    let off = if pp.kind == PopKind::Slash { 1 } else { 2 };
    let items = popup_items(&pp, &q, &st.page_titles);
    // close when the cursor leaves, the trigger is deleted, or nothing matches a space-ended query
    let trigger_gone = chars.get(pp.start) != Some(&(if pp.kind == PopKind::Slash { '/' } else { '[' }));
    if cur < pp.start + off || trigger_gone || q.contains('\n') || (items.is_empty() && q.ends_with(' ')) || (!out.response.has_focus() && !out.response.lost_focus()) {
        st.popup = None;
        return None;
    }
    let sel = pp.sel.min(items.len().saturating_sub(1));
    let rect = out.galley.pos_from_cursor(egui::text::CCursor::new(cur));
    let to_global = ctx.layer_transform_to_global(ui.layer_id()).unwrap_or_default();
    let at = to_global * (out.galley_pos + rect.left_bottom().to_vec2() + Vec2::new(0.0, 6.0));
    let mut picked = None;
    egui::Area::new(egui::Id::unique(("popup", block_id))).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).inner_margin(6).show(ui, |ui| {
            ui.set_min_width(240.0);
            ui.label(RichText::new(if pp.kind == PopKind::Slash { "INSERT" } else { "LINK TO A PAGE" }).size(10.5).weak());
            if items.is_empty() { ui.label(RichText::new("No matches").weak()); }
            for (i, (label, icon, _)) in items.iter().enumerate() {
                let r = ui.add(egui::Button::new(RichText::new(format!("{icon}   {label}")).size(13.5)).selected(i == sel).frame_when_inactive(false).min_size(Vec2::new(236.0, 28.0)));
                if r.clicked() { picked = Some(i); }
            }
            ui.label(RichText::new("↑↓ to choose · Enter to insert · Esc to close").size(10.5).weak());
        });
    });
    picked
}

fn mmss(s: f32) -> String { let s = s.max(0.0) as u32; format!("{:02}:{:02}", s / 60, s % 60) }

/// Which recording on this page is "current": the one playing, else the newest.
fn current_recording(page: &Page, st: &St) -> Option<usize> {
    let playing = st.player.as_ref().and_then(|pl| page.recordings.iter().position(|r| r.id == pl.id));
    playing.or(page.recordings.len().checked_sub(1))
}

/// Make sure the player holds recording `i` of this page.
fn ensure_player(page: &Page, st: &mut St, i: usize) -> bool {
    let rec = &page.recordings[i];
    if st.player.as_ref().is_some_and(|p| p.id == rec.id) { return true; }
    match crate::audio::Player::open(&data_dir().join("files").join(&rec.file), rec.id) {
        Ok(p) => { st.player = Some(p); true }
        Err(e) => { st.say(format!("⚠ Can't play the recording: {e}")); false }
    }
}

/// Play from a few seconds before the moment `t` (Unix seconds) in whichever recording covers it.
fn jump_to(page: &Page, st: &mut St, t: f64) {
    let Some(i) = page.recordings.iter().position(|r| t >= r.start - 1.0 && t <= r.start + r.secs as f64 + 2.0) else { return };
    if ensure_player(page, st, i) {
        let p = st.player.as_ref().unwrap();
        p.seek((t - page.recordings[i].start) as f32 - 2.0);
        p.set_playing(true);
    }
}

/// During playback: a test for "written later than the current moment" (those are shown faded).
fn playback_moment(page: &Page, st: &mut St) -> Option<impl Fn(f64) -> bool> {
    let span = page.recordings.iter().fold(None, |acc: Option<(f64, f64)>, r| { let z = r.start + r.secs as f64; Some(acc.map_or((r.start, z), |(a, b)| (a.min(r.start), b.max(z)))) });
    st.recordings_span = span;
    st.fade_after = None;
    if let Some(t) = st.jump.take() { jump_to(page, st, t); }
    let pl = st.player.as_ref()?;
    let rec = page.recordings.iter().find(|r| r.id == pl.id)?;
    if !pl.playing() && pl.time() < 0.05 { st.fade_after = None; return None; }
    let now = rec.start + pl.time() as f64;
    let (a, z) = (rec.start, rec.start + rec.secs as f64);
    st.fade_after = Some((now, a, z));
    Some(move |t: f64| t > now && t >= a && t <= z)
}

/// Under the title: recording controls, or playback with a scrubber.
fn audio_bar(ui: &mut egui::Ui, page: &mut Page, st: &mut St) {
    let recording_here = st.recorder.as_ref().is_some_and(|r| r.1 == page.id);
    if page.recordings.is_empty() && !recording_here { return; }
    let red = Color32::from_rgb(220, 50, 47);
    ui.add_space(6.0);
    egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(12).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            if recording_here {
                let secs = st.recorder.as_ref().map_or(0.0, |r| (now_f() - r.3) as f32);
                ui.label(RichText::new("●").color(red));
                ui.label(RichText::new(format!("Recording  {}", mmss(secs))).color(red));
                if ui.button("■ Stop").clicked() { st.rec_toggle = true; }
                if page.recordings.is_empty() { return; }
                ui.separator();
            }
            let Some(mut i) = current_recording(page, st) else { return };
            let playing = st.player.as_ref().is_some_and(|p| p.id == page.recordings[i].id && p.playing());
            if ui.add(egui::Button::new(RichText::new(if playing { "⏸" } else { "▶" }).size(16.0)).min_size(Vec2::new(34.0, 28.0))).tip(if playing { "Pause" } else { "Play" }).clicked()
                && ensure_player(page, st, i) { st.player.as_ref().unwrap().set_playing(!playing); }
            if page.recordings.len() > 1 {
                let label = |k: usize, r: &Recording| format!("Recording {} · {}", k + 1, mmss(r.secs));
                egui::ComboBox::from_id_salt(("recs", page.id)).selected_text(label(i, &page.recordings[i])).show_ui(ui, |ui| {
                    for (k, r) in page.recordings.iter().enumerate() { if ui.selectable_label(k == i, label(k, r)).clicked() { i = k; st.player = None; let _ = ensure_player(page, st, k); } }
                });
            }
            let rec = &page.recordings[i];
            let mut t = st.player.as_ref().filter(|p| p.id == rec.id).map_or(0.0, |p| p.time());
            ui.spacing_mut().slider_width = (ui.available_width() - 250.0).clamp(80.0, 420.0);
            if ui.add(egui::Slider::new(&mut t, 0.0..=rec.secs.max(0.1)).show_value(false)).changed() && ensure_player(page, st, i) { st.player.as_ref().unwrap().seek(t); }
            ui.label(RichText::new(format!("{} / {}", mmss(t), mmss(rec.secs))).size(12.0).monospace().weak());
            let tip = "On: clicking a note or ink plays what was said while you wrote it";
            if ui.add(egui::Button::new(RichText::new("👆 Tap notes to jump").size(12.0)).selected(st.listen).corner_radius(10)).tip(tip).clicked() { st.listen ^= true; }
            if ui.add(egui::Button::new("🗑").frame_when_inactive(false)).tip("Remove this recording from the page (Undo brings it back)").clicked() {
                st.snap(page);
                let id = page.recordings[i].id;
                page.recordings.retain(|r| r.id != id);
                if st.player.as_ref().is_some_and(|p| p.id == id) { st.player = None; }
            }
        });
    });
}

/// The card editor of a flashcard deck. Returns true when "Study" is pressed.
fn deck_view(ui: &mut egui::Ui, page: &mut Page, st: &mut St, study: &std::collections::HashMap<String, CardState>) -> bool {
    let acc = accent(ui);
    let now = now_secs();
    let weak = ui.visuals().weak_text_color();
    let status = |c: &DeckCard| match study.get(&format!("card{}", c.id)) {
        None => ("new".to_string(), acc),
        Some(s) if s.due <= now => ("due".to_string(), Color32::from_rgb(214, 120, 40)),
        Some(s) => { let d = (s.due - now).div_ceil(86_400); (if d <= 1 { "next tomorrow".into() } else { format!("next in {d} days") }, weak) }
    };
    let ready = page.deck().len();
    let (due, new) = page.cards.iter().fold((0, 0), |(d, n), c| match status(c).0.as_str() { "due" => (d + 1, n), "new" if !c.front.trim().is_empty() && !c.back.trim().is_empty() => (d, n + 1), _ => (d, n) });
    let mut study_now = false;
    // summary and the main action
    ui.horizontal(|ui| {
        let chip = |ui: &mut egui::Ui, t: String, c: Color32| { egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(10).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| ui.label(RichText::new(t).size(12.5).color(c))); };
        chip(ui, format!("▤  {} cards", page.cards.len()), ui.visuals().text_color());
        if due > 0 { chip(ui, format!("{due} due"), Color32::from_rgb(214, 120, 40)); }
        if new > 0 { chip(ui, format!("{new} new"), acc); }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(12.0);
            let b = ui.add_enabled(ready > 0, egui::Button::new(RichText::new(format!("▶  Study{}", if due + new > 0 { format!(" ({})", due + new) } else { String::new() })).size(15.0).color(Color32::WHITE)).fill(acc).corner_radius(18).min_size(Vec2::new(150.0, 36.0)));
            if b.tip("Go through the cards; due and new ones come first").clicked() { study_now = true; }
        });
    });
    ui.add_space(10.0);
    let mut remove = None;
    let mut swap = None;
    let mut changed = false;
    let colors = rich::Colors::from(ui, acc);
    egui::ScrollArea::vertical().id_salt(("deck", page.id)).auto_shrink([false; 2]).show(ui, |ui| {
        ui.set_max_width(ui.available_width().min(900.0) - 12.0);
        let n = page.cards.len();
        for (i, c) in page.cards.iter_mut().enumerate() {
            let (label, col) = status(c);
            egui::Frame::new().fill(ui.visuals().extreme_bg_color).stroke(ui.visuals().widgets.noninteractive.bg_stroke).corner_radius(12)
                .inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}", i + 1)).size(13.0).weak());
                    ui.label(RichText::new(label).size(11.0).color(col));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new("🗑").frame_when_inactive(false)).tip("Delete this card").clicked() { remove = Some(i); }
                        if i + 1 < n && ui.add(egui::Button::new("↓").frame_when_inactive(false)).tip("Move down").clicked() { swap = Some((i, i + 1)); }
                        if i > 0 && ui.add(egui::Button::new("↑").frame_when_inactive(false)).tip("Move up").clicked() { swap = Some((i, i - 1)); }
                    });
                });
                ui.columns(2, |cols| {
                    for (k, (side, text, hint)) in [("FRONT", &mut c.front, "Question or term"), ("BACK", &mut c.back, "Answer")].into_iter().enumerate() {
                        let ui = &mut cols[k];
                        ui.label(RichText::new(side).size(10.5).weak());
                        let id = egui::Id::unique((if k == 0 { "cardf" } else { "cardb" }, c.id));
                        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                            let mut job = rich::edit_job(buf.as_str(), 15.0, 0, &colors);
                            job.wrap.max_width = wrap;
                            ui.fonts_mut(|f| f.layout_job(job))
                        };
                        let r = ui.add(egui::TextEdit::multiline(text).id(id).hint_text(hint).desired_rows(2).desired_width(f32::INFINITY).layouter(&mut layouter)
                            .background_color(ui.visuals().faint_bg_color).margin(egui::Margin::symmetric(8, 6)));
                        if st.focus_id == Some(id) { r.request_focus(); st.focus_id = None; }
                        changed |= r.changed();
                        // show how maths and formatting will look when studying
                        if text.contains('$') {
                            egui::Frame::new().inner_margin(egui::Margin { left: 6, right: 0, top: 4, bottom: 0 }).show(ui, |ui| { let _ = rich::view(ui, text, 14.0, 0, &colors); });
                        }
                    }
                });
            });
            ui.add_space(8.0);
        }
        if page.cards.is_empty() { ui.label(RichText::new("No cards yet.").weak()); }
        let add = ui.add(egui::Button::new(RichText::new("+  Add card").size(14.0).color(acc)).min_size(Vec2::new(ui.available_width(), 40.0)).corner_radius(12)
            .fill(ui.visuals().faint_bg_color).stroke(EStroke::new(1.0, acc.gamma_multiply(0.5))));
        if add.clicked() {
            let c = DeckCard::new("", "");
            st.focus_id = Some(egui::Id::unique(("cardf", c.id)));
            st.snap(page);
            page.cards.push(c);
        }
        ui.add_space(6.0);
        ui.label(RichText::new("Tip: Tab jumps from front to back. Use $x^2$ for maths and **bold** for emphasis.").size(11.5).weak());
        ui.add_space(30.0);
    });
    if let Some(i) = remove { st.snap(page); page.cards.remove(i); }
    if let Some((a, b)) = swap { st.snap(page); page.cards.swap(a, b); }
    if changed { st.dirty = true; }
    study_now
}

const MATH_PALETTE: &[(&str, &str)] = &[
    ("a⁄b", r"\frac{}{}"), ("√", r"\sqrt{}"), ("xⁿ", "^{}"), ("xₙ", "_{}"), ("∑", r"\sum_{i=1}^{n} "), ("∫", r"\int_{a}^{b} "), ("lim", r"\lim_{x \to 0} "),
    ("( )", r"\left( \right)"), ("[⋮]", r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"), ("{≡", r"\begin{cases}  & \\  & \end{cases}"),
    ("x̂", r"\hat{}"), ("x⃗", r"\vec{}"), ("x̄", r"\bar{}"), ("α", r"\alpha "), ("β", r"\beta "), ("γ", r"\gamma "), ("δ", r"\delta "), ("θ", r"\theta "),
    ("λ", r"\lambda "), ("μ", r"\mu "), ("π", r"\pi "), ("σ", r"\sigma "), ("ω", r"\omega "), ("Δ", r"\Delta "), ("Ω", r"\Omega "),
    ("≤", r"\le "), ("≥", r"\ge "), ("≠", r"\ne "), ("≈", r"\approx "), ("±", r"\pm "), ("×", r"\times "), ("·", r"\cdot "), ("→", r"\to "),
    ("⇒", r"\Rightarrow "), ("∞", r"\infty "), ("∂", r"\partial "), ("∇", r"\nabla "), ("∈", r"\in "), ("ℝ", r"\mathbb{R}"),
];

/// Replace the selection a..b with `snippet`; the cursor lands in its first `{}` (or after it).
fn insert_snippet(text: &mut String, a: usize, b: usize, snippet: &str) -> (usize, usize) {
    let byte = |ci: usize| text.char_indices().nth(ci).map_or(text.len(), |(i, _)| i);
    let (ab, bb) = (byte(a), byte(b));
    text.replace_range(ab..bb, snippet);
    let c = snippet.find("{}").map_or(snippet.chars().count(), |i| snippet[..i].chars().count() + 1);
    (a + c, a + c)
}

/// Draws a note's content; returns true when the person changed it.
fn block_body(ui: &mut egui::Ui, b: &mut Block, st: &mut St, active: bool) -> bool {
    let w = ui.available_width();
    let id = egui::Id::unique(("ed", b.id));
    let ctx = ui.ctx().clone();
    let acc = accent(ui);
    let mut changed = false;
    let focus = |r: &egui::Response, st: &mut St| { if st.focus_id == Some(r.id) { r.request_focus(); st.focus_id = None; } };
    match &mut b.kind {
        Kind::Text { text, heading } => {
            let has_focus = ui.memory(|m| m.has_focus(id));
            // keyboard formatting while typing
            if has_focus {
                let k = |m: Modifiers, key: Key| ui.input_mut(|i| i.consume_key(m, key));
                if k(Modifiers::COMMAND, Key::B) { st.fmt = Some(Fmt::Wrap("**")); }
                if k(Modifiers::COMMAND, Key::I) { st.fmt = Some(Fmt::Wrap("*")); }
                if k(Modifiers::COMMAND | Modifiers::SHIFT, Key::H) { st.fmt = Some(Fmt::Wrap("==")); }
                if k(Modifiers::COMMAND, Key::M) { st.fmt = Some(Fmt::Wrap("$")); }
                if k(Modifiers::COMMAND, Key::Backtick) { st.fmt = Some(Fmt::Wrap("`")); }
            }
            if st.fmt.is_some() && Some(b.id) == st.editing.or(st.last_text) {
                match st.fmt.take().unwrap() {
                    Fmt::Wrap(m) => rich::edit_at_cursor(&ctx, id, text, |t, a, b| rich::toggle_wrap(t, a, b, m)),
                    Fmt::Prefix(p) => rich::edit_at_cursor(&ctx, id, text, |t, a, b| rich::toggle_prefix(t, a, b, p)),
                    Fmt::Heading(h) => *heading = h,
                }
                changed = true;
                st.editing = Some(b.id);
                st.focus_id = Some(id);
            }
            let colors = rich::Colors::from(ui, acc);
            let hd = *heading;
            let size = st.p.note_size;
            // keys for an open "/" menu or "[[" page list go to the menu, not the text
            let mut choose = None;
            if let Some(pp) = st.popup.clone().filter(|pp| pp.block == b.id && has_focus) {
                let k = |key: Key| ui.input_mut(|i| i.consume_key(Modifiers::NONE, key));
                let n = popup_items(&pp, &popup_query(&ctx, id, text, &pp), &st.page_titles).len();
                if k(Key::ArrowDown) { st.popup.as_mut().unwrap().sel = (pp.sel + 1) % n.max(1); }
                if k(Key::ArrowUp) { st.popup.as_mut().unwrap().sel = (pp.sel + n.max(1) - 1) % n.max(1); }
                if (k(Key::Enter) || k(Key::Tab)) && n > 0 { choose = Some(pp.sel.min(n - 1)); }
            }
            if let Some(i) = choose { changed |= apply_popup(&ctx, id, b.id, text, i, st); }
            if st.editing == Some(b.id) || st.focus_id == Some(id) || has_focus {
                let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                    let mut job = rich::edit_job(buf.as_str(), size, hd, &colors);
                    job.wrap.max_width = wrap;
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let out = egui::TextEdit::multiline(text).id(id).frame(egui::Frame::NONE).desired_rows(1).desired_width(w)
                    .hint_text("Type here…  **bold**, - list, $x^2$").layouter(&mut layouter).show(ui);
                let r = &out.response;
                focus(r, st);
                if let Some(i) = popup_ui(ui, &ctx, id, b.id, text, &out, st) { changed |= apply_popup(&ctx, id, b.id, text, i, st); }
                let r = &out.response;
                if r.gained_focus() { st.editing = Some(b.id); st.last_text = Some(b.id); st.want_snap = true; }
                if r.has_focus() { st.last_text = Some(b.id); }
                if r.changed() {
                    changed = true;
                    if st.p.auto_lists && ui.input(|i| i.key_pressed(Key::Enter)) {
                        if let Some(c) = out.cursor_range.and_then(|cr| rich::continue_list(text, cr.primary.index.0)) {
                            use egui::text::{CCursor, CCursorRange};
                            let mut state = out.state.clone();
                            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
                            state.store(&ctx, id);
                        }
                    }
                }
                if r.lost_focus() && st.editing == Some(b.id) && st.fmt.is_none() && st.focus_id != Some(id) { st.editing = None; }
            } else {
                let (clicked, link) = rich::view(ui, text, size, hd, &colors);
                if let Some(l) = link { st.open_link = Some(l); } else if clicked {
                    st.editing = Some(b.id);
                    st.focus_id = Some(id);
                    st.last_text = Some(b.id);
                }
            }
        }
        Kind::Math { src } => {
            if st.editing == Some(b.id) {
                egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(8).inner_margin(10).show(ui, |ui| {
                    ui.set_width(w - 20.0);
                    if src.trim().is_empty() { ui.label(RichText::new("Type LaTeX below, or tap the symbols.").weak()); } else { math::math_label(ui, src, 22.0); }
                    ui.add_space(6.0);
                    let r = ui.add(egui::TextEdit::multiline(src).id(id).code_editor().desired_rows(1).desired_width(f32::INFINITY).hint_text(r"e.g. \frac{a}{b} or x^2 + y^2 = r^2"));
                    focus(&r, st);
                    changed |= r.changed();
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                        for (label, snip) in MATH_PALETTE {
                            if ui.add(egui::Button::new(RichText::new(*label).size(13.0)).min_size(Vec2::new(28.0, 24.0))).tip(*snip).clicked() {
                                rich::edit_at_cursor(&ctx, id, src, |t, a, b| insert_snippet(t, a, b, snip));
                                st.focus_id = Some(id);
                                changed = true;
                            }
                        }
                    });
                    ui.add_space(4.0);
                    if ui.add(egui::Button::new(RichText::new("Done").color(Color32::WHITE)).fill(acc)).clicked() { st.editing = None; }
                });
            } else if src.trim().is_empty() {
                if ui.add(egui::Label::new(RichText::new("∑  Empty equation — click to write one").weak()).sense(Sense::click())).clicked() { st.editing = Some(b.id); st.focus_id = Some(id); }
            } else if math::math_label(ui, src, 22.0).tip("Click to edit").clicked() {
                st.editing = Some(b.id);
                st.focus_id = Some(id);
            }
        }
        Kind::Checklist { items } => {
            if st.focus_id == Some(id) { st.focus_id = Some(egui::Id::unique(("chk", b.id, items.len().saturating_sub(1)))); }
            let mut remove = None;
            let mut new_item: Option<(usize, String)> = None;
            let n = items.len();
            ui.spacing_mut().item_spacing.y = 8.0;
            for (i, (done, t)) in items.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    changed |= ui.checkbox(done, "").changed();
                    // Multi-line so long items wrap; Enter still makes a new item.
                    let mut te = egui::TextEdit::multiline(t).id(egui::Id::unique(("chk", b.id, i))).frame(egui::Frame::NONE).desired_rows(1).desired_width(w - 40.0).hint_text("To do…");
                    if *done { te = te.text_color(ui.visuals().weak_text_color()); }
                    let r = ui.add(te);
                    focus(&r, st);
                    changed |= r.changed();
                    if let Some(nl) = t.find('\n') {
                        let rest = t[nl + 1..].replace('\n', " ");
                        t.truncate(nl);
                        new_item = Some((i, rest));
                    }
                    if r.has_focus() && t.is_empty() && n > 1 && ui.input(|i| i.key_pressed(Key::Backspace)) { remove = Some(i); }
                });
            }
            if let Some((i, rest)) = new_item { items.insert(i + 1, (false, rest)); st.focus_id = Some(egui::Id::unique(("chk", b.id, i + 1))); changed = true; }
            if let Some(i) = remove { items.remove(i); st.focus_id = Some(egui::Id::unique(("chk", b.id, i.saturating_sub(1)))); changed = true; }
            let done = items.iter().filter(|i| i.0).count();
            ui.horizontal(|ui| {
                if items.len() > 1 {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(80.0, 4.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, ui.visuals().widgets.inactive.weak_bg_fill);
                    ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width() * done as f32 / items.len() as f32, r.height())), 2.0, acc);
                    ui.label(RichText::new(format!("{done} of {} done", items.len())).size(11.0).weak());
                }
                if active && ui.small_button("+ item").clicked() { items.push((false, String::new())); st.focus_id = Some(egui::Id::unique(("chk", b.id, items.len() - 1))); changed = true; }
            });
        }
        Kind::Table { rows } => {
            let cols = rows.first().map(|r| r.len()).unwrap_or(1).max(1);
            // Columns as wide as their longest cell, scaled down together if the note is narrower.
            let bold = rich::HAS_BOLD.load(std::sync::atomic::Ordering::Relaxed);
            let head_font = if bold { FontId::new(14.0, egui::FontFamily::Name("bold".into())) } else { FontId::proportional(14.0) };
            let mut colw: Vec<f32> = (0..cols).map(|c| rows.iter().enumerate().filter_map(|(ri, r)| r.get(c).map(|t| (ri, t)))
                .map(|(ri, t)| ui.painter().layout_no_wrap(t.clone(), if ri == 0 { head_font.clone() } else { FontId::proportional(14.0) }, Color32::WHITE).size().x + 20.0)
                .fold(48.0, f32::max)).collect();
            let total: f32 = colw.iter().sum::<f32>() + cols as f32 * 10.0 + 12.0;
            if total > w { for c in &mut colw { *c = (*c * w / total).max(36.0); } }
            egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).corner_radius(6).inner_margin(6).show(ui, |ui| {
                egui::Grid::new(("tbl", b.id)).striped(true).spacing([10.0, 6.0]).show(ui, |ui| {
                    for (ri, row) in rows.iter_mut().enumerate() {
                        for (ci, cell) in row.iter_mut().enumerate() {
                            let mut te = egui::TextEdit::singleline(cell).desired_width(colw[ci.min(cols - 1)]).clip_text(false).frame(egui::Frame::NONE).id(egui::Id::unique(("cell", b.id, ri, ci)));
                            if ri == 0 { te = te.font(head_font.clone()).text_color(acc); }
                            let r = ui.add(te);
                            if ri == 0 && ci == 0 && st.focus_id == Some(id) { r.request_focus(); st.focus_id = None; }
                            changed |= r.changed();
                        }
                        ui.end_row();
                    }
                });
            });
            if active {
                ui.horizontal_wrapped(|ui| { // wraps inside narrow notes
                    if ui.small_button("+ row").clicked() { rows.push(vec![String::new(); cols]); changed = true; }
                    if ui.small_button("+ column").clicked() { for r in rows.iter_mut() { r.push(String::new()); } changed = true; }
                    if rows.len() > 1 && ui.small_button("− row").clicked() { rows.pop(); changed = true; }
                    if cols > 1 && ui.small_button("− column").clicked() { for r in rows.iter_mut() { r.pop(); } changed = true; }
                });
            }
        }
        Kind::Code { lang, code } => {
            let dark = ui.visuals().dark_mode;
            egui::Frame::new().fill(if dark { Color32::from_rgb(20, 24, 25) } else { Color32::from_rgb(244, 247, 248) }).stroke(ui.visuals().widgets.noninteractive.bg_stroke).corner_radius(8).inner_margin(8).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("{ }").size(11.0).color(acc));
                    changed |= ui.add(egui::TextEdit::singleline(lang).desired_width(90.0).font(egui::TextStyle::Small).frame(egui::Frame::NONE)).changed();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.small_button("⧉ Copy").clicked() { ui.ctx().copy_text(code.clone()); } });
                });
                let r = ui.add(egui::TextEdit::multiline(code).id(id).code_editor().desired_rows(2).desired_width(w - 16.0).frame(egui::Frame::NONE));
                focus(&r, st);
                changed |= r.changed();
            });
        }
        Kind::Image { file } => {
            let tex = st.textures.entry(file.clone()).or_insert_with(|| {
                let mut img = image::open(data_dir().join("files").join(&*file)).ok()?;
                // graphics chips (especially phones) can't show huge pictures; 4096 px is safe everywhere
                if img.width().max(img.height()) > 4096 { img = img.resize(4096, 4096, image::imageops::FilterType::Triangle); }
                let img = img.to_rgba8();
                let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                Some(ui.ctx().load_texture(file.clone(), ci, egui::TextureOptions::LINEAR))
            });
            match tex {
                Some(t) => {
                    let s = t.size_vec2();
                    let size = Vec2::new(w, w * s.y / s.x.max(1.0));
                    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                    ui.painter().image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                None => { ui.label(RichText::new("⚠ This picture's file is missing").weak()); }
            }
        }
        Kind::File { file, name, size } => {
            egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(8).inner_margin(8).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("📄").size(24.0));
                    ui.vertical(|ui| { ui.label(RichText::new(&*name).strong()); ui.label(RichText::new(format!("{:.1} KB", *size as f64 / 1024.0)).small().weak()); });
                    if ui.button("Open").clicked() { open_path(&data_dir().join("files").join(&*file)); }
                });
            });
        }
        Kind::Link { url, title } => {
            if st.editing == Some(b.id) {
                egui::Grid::new(("link", b.id)).num_columns(2).show(ui, |ui| {
                    ui.label("Address");
                    let r = ui.add(egui::TextEdit::singleline(url).id(id).desired_width(w - 80.0));
                    focus(&r, st);
                    changed |= r.changed();
                    ui.end_row();
                    ui.label("Title");
                    changed |= ui.add(egui::TextEdit::singleline(title).hint_text("optional").desired_width(w - 80.0)).changed();
                    ui.end_row();
                });
                if ui.add(egui::Button::new(RichText::new("Done").color(Color32::WHITE)).fill(acc)).clicked() { st.editing = None; }
            } else {
                egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(8).inner_margin(8).show(ui, |ui| {
                    ui.set_width(w - 16.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🔗").size(18.0));
                        ui.vertical(|ui| {
                            ui.hyperlink_to(RichText::new(if title.is_empty() { url.as_str() } else { title.as_str() }).size(14.0), url.as_str());
                            if !title.is_empty() { ui.label(RichText::new(url.as_str()).size(11.0).weak()); }
                        });
                    });
                });
            }
        }
    }
    changed
}

pub(crate) fn open_path(p: &std::path::Path) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(not(any(target_os = "linux", windows)))]
    let _ = p; // shortcut: Android needs an intent via JNI to open attachments; add when phones need it.
}

#[cfg(test)]
mod tests {
    #[test]
    fn lasso_and_snippets() {
        use eframe::egui::pos2;
        let sq = [pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0), pos2(0.0, 10.0)];
        assert!(super::point_in_poly(pos2(5.0, 5.0), &sq));
        assert!(!super::point_in_poly(pos2(15.0, 5.0), &sq));
        let mut t = String::from("x + ");
        assert_eq!(super::insert_snippet(&mut t, 4, 4, r"\frac{}{}"), (10, 10));
        assert_eq!(t, r"x + \frac{}{}");
    }
}
