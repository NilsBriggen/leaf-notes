//! The page: title, paper, free-placed notes, ink layer, deck editor, and audio bar.

mod audio_bar;
mod blocks;
mod deck;
mod ink;
mod popup;

pub(crate) use audio_bar::{audio_bar, jump_to, playback_moment};
pub(crate) use blocks::{block_ui, BlockAction};
pub(crate) use deck::deck_view;
pub(crate) use ink::{
    ink_input, paint_stroke, seg_width, selection_bounds, selection_handles, smooth_passes, trail,
};
pub(crate) use popup::Popup;

use crate::chrome::format_date;
use crate::*;
use eframe::egui::{Align2, FontId, Rect, RichText, Sense};

pub(crate) enum PageAction {
    Export,
    Copy,
    Delete,
    Duplicate,
    History,
    Unlock(String),
    Go(usize, usize, usize),
    Nav(bool),
    ToggleFav,
    Study,
    Pdf(bool),
    Retitled(String),
}

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
    /// the page zoom before the capture, put back afterwards
    pub scale: Option<f32>,
}

impl Capture {
    /// Tile size in page units for a viewport of `avail` points: as wide as the content, up to 2× zoom.
    pub(crate) fn scale(&self, avail: Vec2) -> f32 {
        (avail.x / self.content.width()).clamp(0.4, 2.0)
    }
    pub fn done(&self) -> bool {
        self.wait == u8::MAX
    }
}

/// True if `file` is a rendered PDF printout page (`<stem>-page<N>.png`).
pub(crate) fn printout_page_num(file: &str) -> Option<u32> {
    file.strip_suffix(".png")
        .and_then(|f| f.rsplit_once("-page"))
        .and_then(|(_, n)| n.parse::<u32>().ok())
}

pub(crate) fn page_view(
    ui: &mut egui::Ui,
    page: &mut Page,
    st: &mut St,
    crumb: &str,
    backlinks: &[(String, (usize, usize, usize))],
    fav: bool,
    study: &std::collections::HashMap<String, CardState>,
) -> Option<PageAction> {
    let mut action = None;
    let edge = |shown: bool| if shown { 0 } else { 14 };
    let touch = st.nav.touch_ui;
    let ml: i8 = if st.nav.narrow { 20 } else { 40 };
    crate::card(ui)
        .inner_margin(egui::Margin { left: ml, right: 12, top: if touch { 8 } else { 10 }, bottom: 0 })
        .outer_margin(egui::Margin { left: edge(st.p.show_nav), right: edge(st.p.show_ai), top: 0, bottom: 6 })
        .show(ui, |ui| {
            let head_top = ui.cursor().top();
            let head = !(st.canvas.typing_kb && page.kind == PageKind::Notes);
            if head && (!touch || page.kind == PageKind::Locked) {
                ui.horizontal_wrapped(|ui| {
                    if !st.nav.nav_back.is_empty()
                        && ui.add(egui::Button::new(RichText::new("◂").size(12.0)).frame_when_inactive(false)).tip("Back to the page before (Alt+←)").clicked()
                    {
                        action = Some(PageAction::Nav(true));
                    }
                    if !st.nav.nav_fwd.is_empty()
                        && ui.add(egui::Button::new(RichText::new("▸").size(12.0)).frame_when_inactive(false)).tip("Forward (Alt+→)").clicked()
                    {
                        action = Some(PageAction::Nav(false));
                    }
                    ui.label(RichText::new(crumb.to_uppercase()).size(10.5).color(accent(ui)).extra_letter_spacing(1.2));
                    if page.kind == PageKind::Locked {
                        return;
                    }
                    ui.label(RichText::new(format!("·  {}", format_date(page.created, true))).size(11.0).weak());
                    if !backlinks.is_empty() {
                        ui.add_space(10.0);
                        ui.label(RichText::new(tr("↩ Linked from")).size(11.0).weak());
                        for (title, at) in backlinks {
                            let t = if title.trim().is_empty() { tr("Untitled page") } else { title.as_str() };
                            if ui.add(egui::Button::new(RichText::new(t).size(11.0).color(accent(ui))).corner_radius(9).min_size(Vec2::new(0.0, 18.0))).clicked() {
                                action = Some(PageAction::Go(at.0, at.1, at.2));
                            }
                        }
                    }
                });
            }
            if head && touch && page.kind != PageKind::Locked && !backlinks.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tr("↩ Linked from")).size(11.0).weak());
                    for (title, at) in backlinks {
                        let t = if title.trim().is_empty() { tr("Untitled page") } else { title.as_str() };
                        if ui.add(egui::Button::new(RichText::new(t).size(11.0).color(accent(ui))).corner_radius(9)).clicked() {
                            action = Some(PageAction::Go(at.0, at.1, at.2));
                        }
                    }
                });
            }
            if page.kind == PageKind::Locked {
                if let Some(pw) = locked_view(ui, st) {
                    action = Some(PageAction::Unlock(pw));
                }
                ui.set_min_size(ui.available_size());
                return;
            }
            if head {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 110.0).clamp(160.0, 680.0);
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut page.title)
                            .desired_rows(1)
                            .id(egui::Id::unique("page-title"))
                            .font(rich::serif(if touch { 20.0 } else { 24.0 }))
                            .text_color(ui.visuals().strong_text_color())
                            .frame(egui::Frame::NONE)
                            .hint_text(RichText::new(tr("Untitled page")).font(rich::serif(if touch { 20.0 } else { 24.0 })))
                            .desired_width(w)
                            .margin(egui::Margin::ZERO),
                    );
                    if st.nav.focus_id == Some(r.id) {
                        r.request_focus();
                        st.nav.focus_id = None;
                    }
                    if r.changed() {
                        st.dirty = true;
                        st.cache.derived_dirty = true;
                    }
                    if r.gained_focus() {
                        st.nav.title_was = Some(page.title.clone());
                    }
                    if r.lost_focus() {
                        if let Some(old) = st.nav.title_was.take().filter(|o| o.trim() != page.title.trim()) {
                            action = Some(PageAction::Retitled(old));
                        }
                    }
                    let enter = page.title.contains('\n');
                    if enter {
                        page.title.retain(|c| c != '\n' && c != '\r');
                        r.surrender_focus();
                    }
                    if enter && page.blocks.is_empty() {
                        let b = block(24.0, 8.0, st.p.note_width, Kind::Text { text: String::new(), heading: 0 });
                        st.nav.focus_id = Some(egui::Id::unique(("ed", b.id)));
                        st.canvas.editing = Some(b.id);
                        page.blocks.push(b);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let m = ui.add(egui::Button::new(RichText::new("⋯").size(18.0)).frame_when_inactive(false)).tip("Page actions");
                        let mut heads: Vec<(f32, String)> = page
                            .blocks
                            .iter()
                            .flat_map(|b| match &b.kind {
                                Kind::Text { text, heading } if *heading > 0 => vec![(b.pos[1], text.lines().next().unwrap_or("").to_string())],
                                Kind::Text { text, .. } => text
                                    .lines()
                                    .enumerate()
                                    .filter(|(_, l)| matches!(rich::classify(l).0, rich::LineKind::Heading(_)))
                                    .map(|(i, l)| (b.pos[1] + i as f32 * 22.0, l.trim_start_matches(['#', ' ']).to_string()))
                                    .collect(),
                                Kind::Image { file } => printout_page_num(file)
                                    .map(|n| vec![(b.pos[1] + 30.0, tf!("Page {n}", n = n))])
                                    .unwrap_or_default(),
                                _ => vec![],
                            })
                            .filter(|(_, t)| !t.trim().is_empty())
                            .collect();
                        heads.sort_by(|a, b| a.0.total_cmp(&b.0));
                        if heads.len() >= 2 {
                            let o = ui.add(egui::Button::new(RichText::new("☰").size(16.0)).frame_when_inactive(false)).tip("Outline — jump to a heading");
                            egui::Popup::menu(&o).show(|ui| {
                                ui.label(RichText::new(tr("ON THIS PAGE")).size(10.5).weak());
                                for (y, t) in &heads {
                                    if ui.button(t).clicked() {
                                        let v = st.canvas.views.get(&page.id).copied().unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0)));
                                        st.canvas.views.insert(page.id, v.translate(Vec2::new(-v.left(), y - 30.0 - v.top())));
                                        ui.close();
                                    }
                                }
                            });
                        }
                        let star = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(if fav { "★" } else { "☆" })
                                        .size(18.0)
                                        .color(if fav { Color32::from_rgb(234, 179, 8) } else { ui.visuals().weak_text_color() }),
                                )
                                .frame_when_inactive(false),
                            )
                            .tip(if fav { "Remove from favourites" } else { "Add to favourites" });
                        if star.clicked() {
                            action = Some(PageAction::ToggleFav);
                        }
                        egui::Popup::menu(&m).show(|ui| {
                            if ui.button(tr("⧉  Duplicate page")).clicked() { action = Some(PageAction::Duplicate); ui.close(); }
                            if ui.button(tr("🕓  Page history…")).clicked() { action = Some(PageAction::History); ui.close(); }
                            if ui.button(tr("⎘  Copy as Markdown")).clicked() { action = Some(PageAction::Copy); ui.close(); }
                            if crate::export::printout_source(page, &|id| st.canvas.heights.get(&id).copied()).is_some() {
                                if ui.button(tr("⇩  Export printout with notes (PDF)…")).tip("The original PDF pages, with your ink and typed notes on top").clicked() {
                                    action = Some(PageAction::Pdf(false));
                                    ui.close();
                                }
                                if ui.button(tr("⇩  Export whole page as PDF…")).clicked() {
                                    action = Some(PageAction::Pdf(true));
                                    ui.close();
                                }
                            } else if ui.button(tr("⇩  Export as PDF…")).tip("Just the part of the page you wrote on, with the title, on A4 pages").clicked() {
                                action = Some(PageAction::Pdf(true));
                                ui.close();
                            }
                            if ui.button(tr("⇪  Export as Markdown…")).clicked() { action = Some(PageAction::Export); ui.close(); }
                            if page.kind == PageKind::Deck && ui.button(tr("▶  Study this deck")).clicked() { action = Some(PageAction::Study); ui.close(); }
                            ui.separator();
                            if ui.button(RichText::new(tr("🗑  Delete page")).color(Color32::from_rgb(200, 60, 50))).clicked() {
                                action = Some(PageAction::Delete);
                                ui.close();
                            }
                        });
                    });
                });
            }
            if page.kind == PageKind::Deck {
                ui.add_space(14.0);
                if deck_view(ui, page, st, study) {
                    action = Some(PageAction::Study);
                }
                return;
            }
            audio_bar(ui, page, st);
            ui.add_space(6.0);
            let (y, full) = (ui.cursor().top(), ui.max_rect());
            ui.painter().hline((full.left() - ml as f32)..=(full.right() + 12.0), y, EStroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
            ui.add_space(1.0);
            let head = Rect::from_x_y_ranges((full.left() - ml as f32)..=(full.right() + 12.0), head_top..=y);
            if st.canvas.input != Input::Mouse
                && matches!(st.canvas.tool, Tool::Pen | Tool::Highlighter | Tool::Shape(_))
                && !ui.ctx().egui_is_using_pointer()
                && ui.input(|i| i.pointer.any_pressed() && i.pointer.press_origin().is_some_and(|p| head.contains(p)))
            {
                st.say("The page title is up here. Ink goes on the paper below the line.");
            }
            let r = ui.available_rect_before_wrap();
            let mut sheet = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(
                Pos2::new(r.left() - ml as f32, r.top()),
                Pos2::new(r.right() + 12.0, r.bottom() - 10.0),
            )));
            ui.advance_cursor_after_rect(r);
            let ui = &mut sheet;
            let avail = ui.available_size();
            let outer = Rect::from_min_size(ui.cursor().min, avail);
            let mut view = st.canvas.views.get(&page.id).copied().unwrap_or(Rect::from_min_size(Pos2::ZERO, avail));
            let mut scale = st.canvas.scales.get(&page.id).copied().unwrap_or_else(|| {
                let right = page
                    .blocks
                    .iter()
                    .map(|b| b.pos[0] + b.width)
                    .chain(page.strokes.iter().flat_map(|s| s.points.iter().map(|p| p[0])))
                    .fold(0.0, f32::max);
                if avail.x < 600.0 && right > avail.x - 12.0 {
                    (avail.x / (right + 12.0)).clamp(0.55, 1.0)
                } else {
                    1.0
                }
            });
            if let Some(c) = st.canvas.capture.as_mut().filter(|c| c.page == page.id && !c.done()) {
                let s = c.scale(avail);
                let th = avail.y / s;
                view = Rect::from_min_size(Pos2::new(c.content.left(), c.content.top() + c.k as f32 * th), avail / s);
                scale = s;
                st.canvas.pan = Vec2::ZERO;
                st.canvas.zoom_req = None;
                let ctx = ui.ctx().clone();
                let mut capture_failed = false;
                let recv = c.rx.as_ref().map(|rx| rx.try_recv());
                if let Some(Ok(img)) = recv {
                    let ppp = ctx.pixels_per_point();
                    let remaining = (c.content.bottom() - view.top()) * s;
                    let crop = Rect::from_min_size(outer.min, Vec2::new(outer.width(), outer.height().min(remaining)));
                    let (x0, y0) = ((crop.left() * ppp).max(0.0) as u32, (crop.top() * ppp).max(0.0) as u32);
                    let (w, h) = (
                        ((crop.width() * ppp).max(0.0) as u32).min((img.size[0] as u32).saturating_sub(x0)),
                        ((crop.height() * ppp).max(0.0) as u32).min((img.size[1] as u32).saturating_sub(y0)),
                    );
                    let full = image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, img.as_raw().to_vec());
                    if let Some(full) = full {
                        if w > 0 && h > 0 {
                            c.tiles.push(image::imageops::crop_imm(&full, x0, y0, w, h).to_image());
                        }
                    }
                    c.rx = None;
                    c.wait = 0;
                    c.k += 1;
                    if c.content.top() + c.k as f32 * th >= c.content.bottom() {
                        c.wait = u8::MAX;
                    }
                } else if matches!(recv, Some(Err(std::sync::mpsc::TryRecvError::Disconnected))) {
                    capture_failed = true;
                } else if c.rx.is_none() {
                    c.wait += 1;
                    if c.wait > 3 {
                        let (tx, rx) = std::sync::mpsc::channel();
                        ctx.request_screenshot(move |img| {
                            let _ = tx.send(img);
                        });
                        c.rx = Some(rx);
                    }
                }
                if capture_failed {
                    if let Some(c) = st.canvas.capture.take() {
                        (st.p.show_nav, st.p.show_ai, st.p.ribbon_open) = c.keep;
                        if let Some(v) = c.view { st.canvas.views.insert(c.page, v); }
                        match c.scale { Some(sc) => { st.canvas.scales.insert(c.page, sc); } None => { st.canvas.scales.remove(&c.page); } }
                        st.say("⚠ PDF export failed because the screenshot was interrupted.");
                    }
                }
                ctx.request_repaint();
            }
            if st.canvas.pan != Vec2::ZERO {
                view = view.translate(-std::mem::take(&mut st.canvas.pan));
            }
            if let Some(q) = st.canvas.find_text.take() {
                let hit = page
                    .blocks
                    .iter()
                    .filter(|b| match &b.kind {
                        Kind::Text { text, .. } => text.to_lowercase().contains(&q),
                        k => k.summary().to_lowercase().contains(&q),
                    })
                    .min_by(|a, b| a.pos[1].total_cmp(&b.pos[1]));
                if let Some(b) = hit {
                    view = view.translate(Vec2::new(0.0, (b.pos[1] - 40.0).max(0.0) - view.top()));
                }
            }
            if !ui.ctx().egui_wants_keyboard_input() {
                let (up, down) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::PageUp), i.consume_key(Modifiers::NONE, Key::PageDown)));
                if up || down {
                    view = view.translate(Vec2::new(0.0, if down { 1.0 } else { -1.0 } * view.height() * 0.85));
                    if view.top() < 0.0 {
                        view = view.translate(Vec2::new(0.0, -view.top()));
                    }
                }
            }
            if let Some(z) = st.canvas.zoom_req.take() {
                let cur = scale;
                let target = match z {
                    ZoomReq::By(f) => cur * f,
                    ZoomReq::To(t) => t,
                    ZoomReq::FitWidth => {
                        let right = page
                            .blocks
                            .iter()
                            .map(|b| b.pos[0] + b.width)
                            .chain(page.strokes.iter().flat_map(|s| s.points.iter().map(|p| p[0])))
                            .fold(400.0, f32::max);
                        view = Rect::from_min_size(Pos2::new(0.0, view.top()), view.size());
                        avail.x / (right + 40.0)
                    }
                }
                .clamp(0.3, 4.0);
                if !matches!(z, ZoomReq::FitWidth) {
                    view = Rect::from_center_size(view.center(), avail / target);
                }
                scale = target;
            }
            view = Rect::from_min_size(view.min, avail / scale);
            if avail.y < st.canvas.canvas_avail.y - 1.0 {
                if let Some(b) = st.canvas.editing.and_then(|id| page.blocks.iter().find(|b| b.id == id)) {
                    let h = st.canvas.heights.get(&b.id).copied().unwrap_or(40.0).min(view.height() * 0.6);
                    if b.pos[1] + h > view.bottom() - 10.0 {
                        view = view.translate(Vec2::new(0.0, b.pos[1] + h - view.bottom() + 24.0));
                    }
                    if b.pos[1] < view.top() {
                        view = view.translate(Vec2::new(0.0, b.pos[1] - view.top() - 24.0));
                    }
                }
            }
            st.canvas.canvas_avail = avail;
            egui::Scene::new()
                .zoom_range(0.3..=4.0)
                .max_inner_size(Vec2::splat(75_000.0))
                .show(ui, &mut view, |ui| canvas(ui, page, st));
            view = view.translate(Vec2::new((-view.left()).max(0.0), (-view.top()).max(0.0)));
            // Guard against tiny squeezed viewports (e.g. phone nav open) corrupting the saved zoom scale.
            if avail.x >= 120.0 {
                scale = (avail.x / view.width().max(1.0)).clamp(0.3, 4.0);
                st.canvas.scales.insert(page.id, scale);
                st.canvas.zoom_now = scale;
                st.canvas.views.insert(page.id, view);
            }
        });
    action
}

fn canvas(ui: &mut egui::Ui, page: &mut Page, st: &mut St) {
    let mut extent = Vec2::new(900.0, 700.0);
    for b in &page.blocks {
        extent = extent.max(Vec2::new(b.pos[0] + b.width, b.pos[1] + st.canvas.heights.get(&b.id).copied().unwrap_or(40.0)) + Vec2::splat(320.0));
    }
    for s in &page.strokes {
        for p in &s.points {
            extent = extent.max(Vec2::new(p[0], p[1]) + Vec2::splat(320.0));
        }
    }
    let view = st.canvas.views.get(&page.id).copied().unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 900.0)));
    let size = extent.max(view.max.to_vec2() + Vec2::splat(200.0));
    let (rect, bg) = ui.allocate_exact_size(size, Sense::click());
    let o = rect.min;
    let dark = ui.visuals().dark_mode;
    let clip = ui.clip_rect();
    let line = if dark { Color32::from_gray(52) } else { Color32::from_rgb(220, 232, 240) };
    let margin_col = if dark { Color32::from_rgb(90, 50, 50) } else { Color32::from_rgb(248, 200, 196) };
    match page.paper {
        Paper::Plain => {}
        Paper::Ruled => {
            let y0 = o.y + 36.0 + ((clip.top() - (o.y + 36.0)).max(0.0) / 28.0).floor() * 28.0;
            let mut y = y0;
            while y < rect.bottom().min(clip.bottom() + 28.0) {
                ui.painter().hline(rect.x_range(), y, EStroke::new(1.0, line));
                y += 28.0;
            }
            ui.painter().vline(o.x + 14.0, rect.y_range(), EStroke::new(1.0, margin_col));
        }
        Paper::Grid => {
            let mut y = o.y + ((clip.top() - o.y).max(0.0) / 24.0).floor() * 24.0;
            while y < rect.bottom().min(clip.bottom() + 24.0) {
                ui.painter().hline(rect.x_range(), y, EStroke::new(1.0, line));
                y += 24.0;
            }
            let mut x = o.x + ((clip.left() - o.x).max(0.0) / 24.0).floor() * 24.0;
            while x < rect.right().min(clip.right() + 24.0) {
                ui.painter().vline(x, rect.y_range(), EStroke::new(1.0, line));
                x += 24.0;
            }
        }
        Paper::Cornell => {
            let y0 = o.y + 36.0 + ((clip.top() - (o.y + 36.0)).max(0.0) / 28.0).floor() * 28.0;
            let mut y = y0;
            while y < rect.bottom().min(clip.bottom() + 28.0) {
                ui.painter().hline(rect.x_range(), y, EStroke::new(1.0, line));
                y += 28.0;
            }
            ui.painter().vline(o.x + 210.0, rect.y_range(), EStroke::new(1.5, margin_col));
        }
        Paper::Music => {
            let staff = if dark { Color32::from_gray(78) } else { Color32::from_rgb(178, 196, 210) };
            let mut y = o.y + 48.0;
            while y < rect.bottom() {
                if y > clip.top() - 100.0 && y < clip.bottom() + 100.0 {
                    for k in 0..5 {
                        ui.painter().hline(rect.x_range(), y + k as f32 * 9.0, EStroke::new(1.0, staff));
                    }
                }
                y += 100.0;
            }
        }
        Paper::Dots => {
            let mut y = o.y + 12.0;
            while y < rect.bottom() {
                if y > clip.top() - 24.0 && y < clip.bottom() + 24.0 {
                    let mut x = o.x + 12.0 + ((clip.left() - o.x - 36.0).max(0.0) / 24.0).floor() * 24.0;
                    while x < rect.right().min(clip.right() + 24.0) {
                        ui.painter().circle_filled(Pos2::new(x, y), 1.3, line.gamma_multiply(1.6));
                        x += 24.0;
                    }
                }
                y += 24.0;
            }
        }
    }
    let auto_pen = st.canvas.tool == Tool::Type && !st.p.draw_finger && st.p.pen_detect;
    let to_page = ui.ctx().layer_transform_from_global(ui.layer_id()).unwrap_or_default();
    let pressed_at = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed())).map(|q| to_page * q);
    if auto_pen && st.canvas.input == Input::Pen && pressed_at.is_some_and(|p| ui.clip_rect().contains(p) && rect.contains(p)) {
        st.canvas.tool = Tool::Pen;
        st.canvas.editing = None;
        st.say("✏ Picked up your pen. Choose Type in the toolbar to type again.");
    }
    let makes_ink = matches!(st.canvas.tool, Tool::Pen | Tool::Highlighter | Tool::Shape(_));
    let allowed = match st.canvas.input {
        Input::Pen => true,
        Input::Finger => st.p.draw_finger,
        Input::Mouse => st.p.draw_mouse,
    };
    st.canvas.ink_pass = (makes_ink && !allowed) || st.canvas.tool == Tool::Type && auto_pen;
    let paper_click = if st.canvas.ink_pass { ink_input(ui, rect, page, st) } else { None };
    let clicked = bg.clicked() || paper_click.is_some();
    let click_pos = paper_click.or_else(|| bg.interact_pointer_pos());
    let typing = st.canvas.tool == Tool::Type || st.canvas.ink_pass;
    let heard = (clicked && typing && st.audio.listen && !page.recordings.is_empty())
        .then_some(click_pos)
        .flatten()
        .and_then(|p| {
            let q = p - o.to_vec2();
            page.strokes.iter().find(|s| s.points.iter().any(|v| (v[0] - q.x).hypot(v[1] - q.y) < 12.0)).map(|s| s.t)
        });
    if let Some(t) = heard {
        jump_to(page, st, t);
    } else if clicked && typing && st.p.click_to_type {
        let near = click_pos.and_then(|p| {
            let q = p - o.to_vec2();
            page.blocks
                .iter()
                .rev()
                .find(|b| {
                    matches!(b.kind, Kind::Text { .. })
                        && Rect::from_min_size(Pos2::new(b.pos[0], b.pos[1]), Vec2::new(b.width, st.canvas.heights.get(&b.id).copied().unwrap_or(30.0)))
                            .expand2(Vec2::new(8.0, 18.0))
                            .contains(q)
                })
                .map(|b| b.id)
        });
        if let Some(id) = near {
            st.canvas.editing = Some(id);
            st.nav.focus_id = Some(egui::Id::unique(("ed", id)));
        } else if let Some(p) = click_pos {
            let (x, mut y) = ((p.x - o.x - 4.0).max(0.0), (p.y - o.y - 16.0).max(0.0));
            if matches!(page.paper, Paper::Ruled | Paper::Cornell) {
                y = ((y - 8.0) / 28.0).round().max(0.0) * 28.0 + 8.0;
            }
            let w = st.canvas.views.get(&page.id).map_or(st.p.note_width, |v| st.p.note_width.min(v.right() - x - 12.0).max(160.0));
            let b = block(x, y, w, Kind::Text { text: String::new(), heading: 0 });
            st.nav.focus_id = Some(egui::Id::unique(("ed", b.id)));
            st.canvas.editing = Some(b.id);
            page.blocks.push(b);
        }
    }
    if page.blocks.is_empty() && page.strokes.is_empty() {
        let hint = tr(if st.nav.touch_ui {
            "Tap anywhere to type, or pick the pen to write. Type / for tables, maths and more."
        } else {
            "Click anywhere to type, or pick the pen to write. Type / for tables, maths and more."
        });
        let g = ui.painter().layout(hint.to_string(), FontId::proportional(14.0), ui.visuals().weak_text_color(), (ui.clip_rect().right() - o.x - 56.0).max(160.0));
        ui.painter().galley(o + Vec2::new(28.0, 24.0), g, ui.visuals().weak_text_color());
    }
    let later = playback_moment(page, st);
    let mut remove = None;
    let mut dup = None;
    let mut changed = false;
    let key_change = ui.input(|i| {
        i.events.iter().any(|e| {
            matches!(e, egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. }
                if *key == Key::Tab || (modifiers.alt && matches!(key, Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown)))
        })
    });
    if ui.input(|i| i.pointer.any_pressed()) || key_change {
        st.canvas.press_page = Some(page.clone());
    }
    for pictures in [true, false] {
        if !pictures {
            let dark = ui.visuals().dark_mode;
            for s in page.strokes.iter().filter(|s| s.highlighter) {
                if dark {
                    let mut f = s.clone();
                    f.color[3] = (f.color[3] as f32 * 0.6) as u8;
                    paint_stroke(&ui.painter_at(rect), &f, o);
                } else {
                    paint_stroke(&ui.painter_at(rect), s, o);
                }
            }
        }
        for b in page.blocks.iter_mut().filter(|b| matches!(b.kind, Kind::Image { .. }) == pictures) {
            match block_ui(ui, b, st, o) {
                BlockAction::Delete => remove = Some(b.id),
                BlockAction::Duplicate => dup = Some(b.id),
                BlockAction::Unpin => st.canvas.unpinned = Some(b.id),
                BlockAction::Card => {
                    let text = match &b.kind {
                        Kind::Text { text, .. } => text.clone(),
                        Kind::Math { src } => format!("${src}$"),
                        k => k.summary(),
                    };
                    let (f, back) = text.split_once("::").map_or((text.trim().to_string(), String::new()), |(f, b)| (f.trim().to_string(), b.trim().to_string()));
                    st.canvas.make_card = Some((f, back));
                }
                BlockAction::Changed => changed = true,
                BlockAction::Rotated => {
                    changed = true;
                    st.canvas.snap_press = true;
                }
                BlockAction::None => {}
            }
        }
    }
    if st.canvas.want_snap {
        st.canvas.want_snap = false;
        st.snap(page);
    }
    if let Some(p) = st.canvas.note_at.take() {
        let (x, y) = ((p.x - o.x - 4.0).max(0.0), (p.y - o.y - 16.0).max(0.0));
        let b = block(x, y, st.p.note_width.min(360.0), Kind::Text { text: String::new(), heading: 0 });
        st.nav.focus_id = Some(egui::Id::unique(("ed", b.id)));
        st.canvas.editing = Some(b.id);
        page.blocks.push(b);
    }
    if std::mem::take(&mut st.canvas.snap_press) {
        if let Some(before) = st.canvas.press_page.take().filter(|b| b.id == page.id) {
            st.snap(&before);
        }
    }
    if let Some((after, kind)) = st.canvas.insert_below.take() {
        if let Some(b) = page.blocks.iter().find(|b| b.id == after) {
            let h = st.canvas.heights.get(&after).copied().unwrap_or(40.0);
            let nb = block(b.pos[0], b.pos[1] + h + 16.0, b.width, kind);
            st.nav.focus_id = Some(egui::Id::unique(("ed", nb.id)));
            st.canvas.editing = Some(nb.id);
            st.snap(page);
            page.blocks.push(nb);
        }
    }
    if let Some(id) = remove {
        let empty_text = page.blocks.iter().any(|b| b.id == id && matches!(&b.kind, Kind::Text { text, .. } if text.trim().is_empty()));
        if !empty_text {
            st.snap(page);
        }
        page.blocks.retain(|b| b.id != id);
        changed = true;
    }
    if let Some(id) = dup {
        st.snap(page);
        if let Some(b) = page.blocks.iter().find(|b| b.id == id) {
            let h = st.canvas.heights.get(&id).copied().unwrap_or(40.0);
            let mut c = b.clone();
            c.id = new_id();
            c.pos[1] += h + 18.0;
            page.blocks.push(c);
        }
    }
    if changed {
        st.dirty = true;
        st.cache.derived_dirty = true;
    }

    let painter = ui.painter_at(rect);
    let pictures: Vec<Rect> = if ui.visuals().dark_mode {
        page.blocks
            .iter()
            .filter(|b| matches!(b.kind, Kind::Image { .. }))
            .map(|b| Rect::from_min_size(Pos2::new(b.pos[0], b.pos[1]), Vec2::new(b.width, st.canvas.heights.get(&b.id).copied().unwrap_or(b.width * 1.3))))
            .collect()
    } else {
        vec![]
    };
    let dark = ui.visuals().dark_mode;
    let shown = |s: &Stroke| -> Option<Stroke> {
        let [r, g, b, a] = s.color;
        if !dark || 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32 >= 60.0 {
            return None;
        }
        let p = s.points.get(s.points.len() / 2)?;
        if pictures.iter().any(|q| q.contains(Pos2::new(p[0], p[1]))) {
            return None;
        }
        Some(Stroke { color: [226, 230, 228, a], ..s.clone() })
    };
    for s in page.strokes.iter().filter(|s| !s.highlighter) {
        let light = shown(s);
        let s = light.as_ref().unwrap_or(s);
        if later.as_ref().is_some_and(|f| f(s.t)) {
            let mut faded = s.clone();
            faded.color[3] = (faded.color[3] as f32 * 0.22) as u8;
            paint_stroke(&painter, &faded, o);
        } else {
            paint_stroke(&painter, s, o);
        }
    }
    if let Some(s) = &st.canvas.current {
        paint_stroke(&painter, shown(s).as_ref().unwrap_or(s), o);
    }
    if st.canvas.tool == Tool::Eraser {
        st.canvas.selected.clear();
    }
    st.canvas.selected.retain(|id| page.strokes.iter().any(|s| s.id == *id));
    if let Some(r) = selection_bounds(page, &st.canvas.selected).filter(|_| st.canvas.capture.is_none()) {
        let r = r.translate(o.to_vec2()).expand(6.0);
        let acc = accent(ui);
        for side in [
            [r.left_top(), r.right_top()],
            [r.right_top(), r.right_bottom()],
            [r.right_bottom(), r.left_bottom()],
            [r.left_bottom(), r.left_top()],
        ] {
            painter.add(egui::Shape::dashed_line(&side, EStroke::new(1.2, acc), 5.0, 4.0));
        }
        painter.text(
            r.left_top() - Vec2::new(0.0, 4.0),
            Align2::LEFT_BOTTOM,
            tf!("{selected_n} selected · drag to move, corner to resize, knob to rotate", selected_n = st.canvas.selected.len()),
            FontId::proportional(11.0),
            acc,
        );
    }
    if st.canvas.lasso.len() > 1 {
        painter.add(egui::Shape::dashed_line(
            &st.canvas.lasso.iter().map(|p| *p + o.to_vec2()).collect::<Vec<_>>(),
            EStroke::new(1.5, accent(ui)),
            6.0,
            4.0,
        ));
    }
    if st.canvas.tool != Tool::Type && !st.canvas.ink_pass {
        ink_input(ui, rect, page, st);
    }
    if st.canvas.tool == Tool::Lasso {
        selection_handles(ui, o, page, st);
    }
}

/// A locked section: the password field. Returns the password when Unlock is pressed.
fn locked_view(ui: &mut egui::Ui, st: &mut St) -> Option<String> {
    let busy = matches!(st.locks.pending, Some(PendingLockOp::Unlock { .. }));
    let mut go = false;
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(380.0);
        ui.label(RichText::new("🔒").size(46.0));
        ui.label(RichText::new(tr("This section is locked")).font(crate::rich::serif(24.0)).color(ui.visuals().strong_text_color()));
        ui.label(RichText::new(tr("Its pages are encrypted. Enter the password to open them.")).weak());
        ui.add_space(10.0);
        let show_id = egui::Id::unique("unlock-show");
        let mut show = ui.data(|d| d.get_temp::<bool>(show_id)).unwrap_or(false);
        let r = ui
            .horizontal(|ui| {
                let r = ui.add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut st.locks.unlock_pw).password(!show).hint_text(tr("Password")).desired_width(250.0),
                );
                if ui.add_enabled(!busy, egui::Button::new(if show { "🙈" } else { "👁" }).frame_when_inactive(false)).tip(if show { "Hide the password" } else { "Show the password" }).clicked() {
                    show ^= true;
                }
                r
            })
            .inner;
        ui.data_mut(|d| d.insert_temp(show_id, show));
        if !busy && ui.memory(|m| m.focused().is_none()) {
            r.request_focus();
        }
        if !busy && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            go = true;
        }
        if !st.locks.unlock_err.is_empty() {
            ui.label(RichText::new(&st.locks.unlock_err).color(Color32::from_rgb(200, 60, 50)));
        }
        ui.add_space(6.0);
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr("Unlocking…"));
            });
        } else if ui.add(egui::Button::new(RichText::new(tr("Unlock")).color(Color32::WHITE)).fill(accent_base()).min_size(Vec2::new(140.0, 34.0))).clicked() {
            go = true;
        }
    });
    go.then(|| std::mem::take(&mut st.locks.unlock_pw))
}

pub(crate) fn open_path(p: &std::path::Path) {
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    open_path_named(p, &name);
}

pub(crate) fn open_path_named(p: &std::path::Path, name: &str) {
    #[cfg(target_os = "linux")]
    { let _ = name; let _ = std::process::Command::new("xdg-open").arg(p).spawn(); }
    #[cfg(windows)]
    { let _ = name; let _ = std::process::Command::new("explorer").arg(p).spawn(); }
    #[cfg(target_os = "android")]
    crate::droid::open(p, name);
    #[cfg(not(any(target_os = "linux", windows, target_os = "android")))]
    { let _ = (p, name); }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lasso_and_snippets() {
        use eframe::egui::pos2;
        let sq = [pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0), pos2(0.0, 10.0)];
        assert!(super::ink::point_in_poly(pos2(5.0, 5.0), &sq));
        assert!(!super::ink::point_in_poly(pos2(15.0, 5.0), &sq));
        let mut t = String::from("x + ");
        assert_eq!(super::blocks::insert_snippet(&mut t, 4, 4, r"\frac{}{}"), (10, 10));
        assert_eq!(t, r"x + \frac{}{}");
    }
}
