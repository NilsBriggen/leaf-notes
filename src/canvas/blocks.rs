//! Free-placed note blocks: floating toolbar, menus, text, math, checklists, tables, code, images, files, links.

use super::popup::{apply_popup, popup_items, popup_query, popup_ui, set_cursor};
use super::{open_path, open_path_named, printout_page_num};
use crate::*;
use eframe::egui::{FontId, Rect, RichText, Sense};

pub(crate) enum BlockAction {
    None,
    Changed,
    Rotated,
    Delete,
    Duplicate,
    Card,
    Unpin,
}

#[derive(Clone, Copy)]
enum BarIcon {
    Grip,
    Resize,
    More,
    Delete,
}

fn bar_icon(p: &egui::Painter, c: Pos2, icon: BarIcon, col: Color32) {
    let s = EStroke::new(1.6, col);
    match icon {
        BarIcon::Grip => {
            for dx in [-3.0, 3.0] {
                for dy in [-5.0, 0.0, 5.0] {
                    p.circle_filled(c + Vec2::new(dx, dy), 1.5, col);
                }
            }
        }
        BarIcon::More => {
            for dx in [-5.0, 0.0, 5.0] {
                p.circle_filled(c + Vec2::new(dx, 0.0), 1.7, col);
            }
        }
        BarIcon::Delete => {
            p.line_segment([c + Vec2::new(-4.5, -4.5), c + Vec2::new(4.5, 4.5)], s);
            p.line_segment([c + Vec2::new(-4.5, 4.5), c + Vec2::new(4.5, -4.5)], s);
        }
        BarIcon::Resize => {
            p.line_segment([c + Vec2::new(-7.0, 0.0), c + Vec2::new(7.0, 0.0)], s);
            for d in [-1.0f32, 1.0] {
                let tip = c + Vec2::new(7.0 * d, 0.0);
                p.line_segment([tip, tip + Vec2::new(-3.5 * d, -3.5)], s);
                p.line_segment([tip, tip + Vec2::new(-3.5 * d, 3.5)], s);
            }
        }
    }
}

pub(crate) fn block_ui(ui: &mut egui::Ui, b: &mut Block, st: &mut St, o: Pos2) -> BlockAction {
    let mut act = BlockAction::None;
    let (tag0, kind0) = (b.tag, std::mem::discriminant(&b.kind));
    let h = st.canvas.heights.get(&b.id).copied().unwrap_or(40.0);
    let outer = Rect::from_min_size(o + Vec2::new(b.pos[0], b.pos[1]), Vec2::new(b.width, h));
    let (bw, bh) = if st.nav.touch_ui { (44.0, 36.0) } else { (30.0, 26.0) };
    let bar_w = 4.0 * bw + 6.0;
    let mut bar = Rect::from_min_size(Pos2::new(outer.right() - bar_w, outer.top() - bh - 6.0), Vec2::new(bar_w, bh));
    let clip = ui.clip_rect();
    // If the toolbar would sit above the viewport (top-of-page notes), place it cleanly below the block instead of covering line 1.
    if bar.top() < clip.top() + 2.0 {
        let below = Rect::from_min_size(Pos2::new(outer.right() - bar_w, outer.bottom() + 6.0), Vec2::new(bar_w, bh));
        if below.bottom() <= clip.bottom() - 2.0 {
            bar = below;
        } else {
            bar = bar.translate(Vec2::new(0.0, clip.top() + 2.0 - bar.top()));
        }
    }
    if bar.left() < clip.left() + 2.0 {
        bar = bar.translate(Vec2::new(clip.left() + 2.0 - bar.left(), 0.0));
    }
    let body = ui.interact(outer, ui.scope_id().with(("blockbg", b.id)), Sense::click());
    let typing = st.canvas.tool == Tool::Type || st.canvas.ink_pass;
    let active = st.canvas.editing == Some(b.id);
    let hovered = typing && st.canvas.capture.is_none() && ui.rect_contains_pointer(outer.union(bar).expand(8.0));
    let acc = accent(ui);
    let v = ui.visuals().clone();
    let mut menu_from: Option<egui::Response> = None;
    if (hovered || active) && st.canvas.capture.is_none() {
        ui.painter().rect_stroke(
            outer.expand(4.0),
            8.0,
            EStroke::new(1.0, if active { acc.gamma_multiply(0.7) } else { v.widgets.noninteractive.bg_stroke.color }),
            egui::StrokeKind::Outside,
        );
        ui.painter().rect(bar.translate(Vec2::new(0.0, 1.5)), 9.0, Color32::from_black_alpha(if v.dark_mode { 60 } else { 14 }), EStroke::NONE, egui::StrokeKind::Inside);
        ui.painter().rect(bar, 9.0, v.extreme_bg_color, v.widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
        let slot = |k: usize| Rect::from_min_size(Pos2::new(bar.left() + 3.0 + k as f32 * bw, bar.top() + 2.0), Vec2::new(bw, bh - 4.0));
        let button = |ui: &mut egui::Ui, k: usize, icon: BarIcon, salt: &str, sense: Sense, tip: &str| {
            let r = slot(k);
            let resp = ui.interact(r, ui.scope_id().with((salt, b.id)), sense);
            if resp.hovered() || resp.dragged() {
                ui.painter().rect_filled(r.shrink(1.0), 6.0, v.widgets.hovered.weak_bg_fill);
            }
            let col = if matches!(icon, BarIcon::Delete) && resp.hovered() {
                Color32::from_rgb(214, 70, 60)
            } else if resp.hovered() || resp.dragged() {
                acc
            } else {
                v.text_color()
            };
            bar_icon(ui.painter(), r.center(), icon, col);
            let name = tr(tip).to_string();
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
            resp.tip(tip)
        };
        let pinned = matches!(&b.kind, Kind::Image { file } if printout_page_num(file).is_some()) && st.canvas.unpinned != Some(b.id);
        let g = if pinned { ui.allocate_rect(Rect::NOTHING, Sense::hover()) } else { button(ui, 0, BarIcon::Grip, "grip", Sense::drag(), "Drag to move") };
        if g.drag_started() {
            st.canvas.snap_press = true;
        }
        if g.dragged() {
            let d = g.drag_delta();
            b.pos[0] = (b.pos[0] + d.x).max(0.0);
            b.pos[1] = (b.pos[1] + d.y).max(0.0);
            act = BlockAction::Changed;
        }
        if g.hovered() || g.dragged() {
            ui.ctx().set_cursor_icon(if g.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }
        let rz = if pinned { ui.allocate_rect(Rect::NOTHING, Sense::hover()) } else { button(ui, 1, BarIcon::Resize, "rz", Sense::drag(), "Drag to make wider or narrower") };
        if rz.drag_started() {
            st.canvas.snap_press = true;
        }
        if rz.dragged() {
            b.width = (b.width + rz.drag_delta().x).clamp(120.0, 1600.0);
            act = BlockAction::Changed;
        }
        if rz.hovered() || rz.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let more = button(ui, 2, BarIcon::More, "more", Sense::click(), "More");
        let del = button(ui, 3, BarIcon::Delete, "del", Sense::click(), "Delete this note");
        if del.clicked() {
            return BlockAction::Delete;
        }
        menu_from = Some(more);
    }
    if st.canvas.editing == Some(b.id) {
        let mut d = Vec2::ZERO;
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::ALT, Key::ArrowLeft) { d.x -= 8.0; }
            if i.consume_key(Modifiers::ALT, Key::ArrowRight) { d.x += 8.0; }
            if i.consume_key(Modifiers::ALT, Key::ArrowUp) { d.y -= 8.0; }
            if i.consume_key(Modifiers::ALT, Key::ArrowDown) { d.y += 8.0; }
        });
        if d != Vec2::ZERO {
            st.canvas.snap_press = true;
            b.pos = [(b.pos[0] + d.x).max(0.0), (b.pos[1] + d.y).max(0.0)];
            act = BlockAction::Changed;
        }
    }
    if let Some(m) = &menu_from {
        egui::Popup::menu(m).show(|ui| block_menu(ui, b, &mut act));
    }
    body.context_menu(|ui| block_menu(ui, b, &mut act));
    let printed = matches!(&b.kind, Kind::Image { file } if printout_page_num(file).is_some());
    if body.clicked() && st.audio.listen && st.audio.recordings_span.is_some_and(|(a, z)| b.t >= a && b.t <= z) {
        st.audio.jump = Some(b.t);
    } else if body.clicked() && printed && typing && st.p.click_to_type {
        st.canvas.note_at = body.interact_pointer_pos();
    } else if body.clicked() {
        st.canvas.editing = Some(b.id);
        st.nav.focus_id = Some(egui::Id::unique(("ed", b.id)));
        if matches!(b.kind, Kind::Text { .. }) {
            st.canvas.last_text = Some(b.id);
        }
    }
    let inner_rect = Rect::from_min_size(outer.min, Vec2::new(b.width, f32::INFINITY));
    let faded = st.audio.fade_after.is_some_and(|(now, a, z)| b.t > now && b.t >= a && b.t <= z);
    let res = ui.scope_builder(egui::UiBuilder::new().max_rect(inner_rect).id_salt(("block", b.id)), |ui| {
        let sp = ui.spacing_mut();
        sp.interact_size.y = 26.0;
        sp.button_padding = Vec2::new(10.0, 5.0);
        sp.item_spacing = Vec2::new(8.0, 6.0);
        ui.set_width(b.width);
        if faded {
            ui.multiply_opacity(0.25);
        }
        ui.horizontal_top(|ui| {
            match b.tag {
                Tag::Todo | Tag::Done => {
                    let mut done = b.tag == Tag::Done;
                    if ui.checkbox(&mut done, "").tip("To Do").changed() {
                        b.tag = if done { Tag::Done } else { Tag::Todo };
                        act = BlockAction::Changed;
                    }
                }
                Tag::Important => {
                    ui.label(RichText::new("★").size(16.0).color(Color32::from_rgb(234, 179, 8)));
                }
                Tag::Question => {
                    ui.label(RichText::new("?").size(16.0).strong().color(Color32::from_rgb(124, 58, 237)));
                }
                Tag::None => {}
            }
            ui.vertical(|ui| {
                if block_body(ui, b, st, hovered || active) && matches!(act, BlockAction::None) {
                    act = BlockAction::Changed;
                }
            });
        });
    });
    st.canvas.heights.insert(b.id, res.response.rect.height());
    if b.tag != tag0 || std::mem::discriminant(&b.kind) != kind0 {
        st.canvas.snap_press = true;
    }
    if matches!(act, BlockAction::None) {
        if let Kind::Text { text, .. } = &b.kind {
            let fid = egui::Id::unique(("ed", b.id));
            if text.trim().is_empty() && st.canvas.editing != Some(b.id) && st.nav.focus_id != Some(fid) && !ui.memory(|m| m.has_focus(fid)) {
                return BlockAction::Delete;
            }
        }
    }
    act
}

fn block_menu(ui: &mut egui::Ui, b: &mut Block, act: &mut BlockAction) {
    let mut rotated: Option<String> = None;
    block_menu_items(ui, b, act, &mut rotated);
    if let Some(to) = rotated {
        b.kind = Kind::Image { file: to };
        *act = BlockAction::Rotated;
    }
}

fn block_menu_items(ui: &mut egui::Ui, b: &mut Block, act: &mut BlockAction, rotated: &mut Option<String>) {
    let textual = matches!(b.kind, Kind::Text { .. } | Kind::Math { .. } | Kind::Code { .. } | Kind::Checklist { .. });
    if ui.button(tr("⧉  Duplicate")).clicked() { *act = BlockAction::Duplicate; ui.close(); }
    if textual && ui.button(tr("▤  Make a flashcard")).tip("Into this page's deck: “front :: back”, or the note as the front").clicked() {
        *act = BlockAction::Card;
        ui.close();
    }
    match &b.kind {
        Kind::Image { file } => {
            if ui.button(tr("↗  Open picture")).clicked() { open_path(&files_path(file)); ui.close(); }
            let printout = printout_page_num(file).is_some();
            if !printout && ui.button(tr("↻  Rotate a quarter turn")).clicked() {
                let to = format!("{}.png", new_id());
                let ok = image::ImageReader::open(files_path(file))
                    .ok()
                    .and_then(|r| r.with_guessed_format().ok())
                    .and_then(|r| r.decode().ok())
                    .map(|i| {
                        let dst = files_path(&to);
                        let saved = i.rotate90().save(&dst).is_ok();
                        if saved { crate::storage::set_private_perms(&dst); }
                        saved
                    })
                    .unwrap_or(false);
                if ok {
                    *rotated = Some(to);
                }
                ui.close();
            }
            if printout && ui.button(tr("✥  Move or resize this page")).clicked() {
                *act = BlockAction::Unpin;
                ui.close();
            }
        }
        Kind::File { file, name, .. } => {
            if ui.button(tr("↗  Open file")).clicked() { open_path_named(&files_path(file), name); ui.close(); }
        }
        Kind::Link { url, .. } => {
            if ui.button(tr("⎘  Copy address")).clicked() { ui.ctx().copy_text(url.clone()); ui.close(); }
        }
        Kind::Math { src } => {
            if ui.button(tr("⎘  Copy LaTeX")).clicked() { ui.ctx().copy_text(src.clone()); ui.close(); }
        }
        Kind::Table { .. } => {
            if ui.button(tr("⎘  Copy as text")).clicked() { ui.ctx().copy_text(b.kind.summary()); ui.close(); }
        }
        _ => {
            if ui.button(tr("⎘  Copy text")).clicked() {
                ui.ctx().copy_text(match &b.kind {
                    Kind::Text { text, .. } => text.clone(),
                    Kind::Code { code, .. } => code.clone(),
                    k => k.summary(),
                });
                ui.close();
            }
        }
    }
    ui.menu_button(tr("🏷  Tag"), |ui| {
        for (t, label) in [(Tag::Todo, tr("☐ To Do")), (Tag::Important, tr("★ Important")), (Tag::Question, tr("? Question")), (Tag::None, tr("No tag"))] {
            if ui.button(label).clicked() { b.tag = t; *act = BlockAction::Changed; ui.close(); }
        }
    });
    if textual {
        let conv: Option<Kind> = ui
            .menu_button(tr("⇄  Turn into"), |ui| {
                let mut out = None;
                let text = match &b.kind {
                    Kind::Text { text, .. } => text.clone(),
                    Kind::Math { src } => src.clone(),
                    Kind::Code { code, .. } => code.clone(),
                    Kind::Checklist { items } => items.iter().map(|i| i.1.clone()).collect::<Vec<_>>().join("\n"),
                    other => other.summary(),
                };
                if !matches!(b.kind, Kind::Text { .. }) && ui.button(tr("Text")).clicked() { out = Some(Kind::Text { text: text.clone(), heading: 0 }); }
                if !matches!(b.kind, Kind::Math { .. }) && ui.button(tr("Equation")).clicked() { out = Some(Kind::Math { src: text.clone() }); }
                if !matches!(b.kind, Kind::Checklist { .. }) && ui.button(tr("Checklist")).clicked() {
                    out = Some(Kind::Checklist { items: text.lines().filter(|l| !l.trim().is_empty()).map(|l| (false, l.trim_start_matches(['-', '*', ' ']).to_string())).collect() });
                }
                if !matches!(b.kind, Kind::Code { .. }) && ui.button(tr("Code")).clicked() { out = Some(Kind::Code { lang: "text".into(), code: text }); }
                if out.is_some() { ui.close(); }
                out
            })
            .inner
            .flatten();
        if let Some(k) = conv {
            b.kind = k;
            *act = BlockAction::Changed;
        }
    }
    ui.separator();
    if ui.button(RichText::new(tr("🗑  Delete")).color(Color32::from_rgb(200, 60, 50))).clicked() {
        *act = BlockAction::Delete;
        ui.close();
    }
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
pub(crate) fn insert_snippet(text: &mut String, a: usize, b: usize, snippet: &str) -> (usize, usize) {
    let byte = |ci: usize| text.char_indices().nth(ci).map_or(text.len(), |(i, _)| i);
    let (ab, bb) = (byte(a), byte(b));
    text.replace_range(ab..bb, snippet);
    let c = snippet.find("{}").map_or(snippet.chars().count(), |i| snippet[..i].chars().count() + 1);
    (a + c, a + c)
}

fn block_body(ui: &mut egui::Ui, b: &mut Block, st: &mut St, active: bool) -> bool {
    let w = ui.available_width();
    let id = egui::Id::unique(("ed", b.id));
    let ctx = ui.ctx().clone();
    let acc = accent(ui);
    let mut changed = false;
    let bid = b.id;
    let focus = |r: &egui::Response, st: &mut St| {
        if st.nav.focus_id == Some(r.id) {
            r.request_focus();
            st.nav.focus_id = None;
        }
        if r.gained_focus() {
            st.canvas.editing = Some(bid);
            st.canvas.want_snap = true;
        }
    };
    match &mut b.kind {
        Kind::Text { text, heading } => {
            let has_focus = ui.memory(|m| m.has_focus(id));
            if has_focus {
                let k = |m: Modifiers, key: Key| ui.input_mut(|i| i.consume_key(m, key));
                if k(Modifiers::COMMAND, Key::B) { st.canvas.fmt = Some(Fmt::Wrap("**")); }
                if k(Modifiers::COMMAND, Key::I) { st.canvas.fmt = Some(Fmt::Wrap("*")); }
                if k(Modifiers::COMMAND | Modifiers::SHIFT, Key::H) { st.canvas.fmt = Some(Fmt::Wrap("==")); }
                if k(Modifiers::COMMAND, Key::M) { st.canvas.fmt = Some(Fmt::Wrap("$")); }
                if k(Modifiers::COMMAND, Key::Backtick) { st.canvas.fmt = Some(Fmt::Wrap("`")); }
            }
            if st.canvas.fmt.is_some() && Some(b.id) == st.canvas.editing.or(st.canvas.last_text) {
                match st.canvas.fmt.take().unwrap() {
                    Fmt::Wrap(m) => rich::edit_at_cursor(&ctx, id, text, |t, a, b| rich::toggle_wrap(t, a, b, m)),
                    Fmt::Prefix(p) => rich::edit_at_cursor(&ctx, id, text, |t, a, b| rich::toggle_prefix(t, a, b, p)),
                    Fmt::Heading(h) => *heading = h,
                }
                changed = true;
                st.canvas.editing = Some(b.id);
                st.nav.focus_id = Some(id);
            }
            let colors = rich::Colors::from(ui, acc);
            let hd = *heading;
            let size = st.p.note_size;
            let mut choose = None;
            if let Some(pp) = st.canvas.popup.clone().filter(|pp| pp.block == b.id && has_focus) {
                let k = |key: Key| ui.input_mut(|i| i.consume_key(Modifiers::NONE, key));
                let n = popup_items(&pp, &popup_query(&ctx, id, text, &pp), &st.cache.page_titles).len();
                if k(Key::ArrowDown) { st.canvas.popup.as_mut().unwrap().sel = (pp.sel + 1) % n.max(1); }
                if k(Key::ArrowUp) { st.canvas.popup.as_mut().unwrap().sel = (pp.sel + n.max(1) - 1) % n.max(1); }
                if n > 0 && (k(Key::Enter) || k(Key::Tab)) { choose = Some(pp.sel.min(n - 1)); }
            }
            if let Some(i) = choose {
                changed |= apply_popup(&ctx, id, b.id, text, i, st);
            }
            let popup_open_here = st.canvas.popup.as_ref().is_some_and(|pp| pp.block == b.id);
            if st.canvas.editing == Some(b.id) || st.nav.focus_id == Some(id) || has_focus || popup_open_here {
                let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                    let mut job = rich::edit_job(buf.as_str(), size, hd, &colors);
                    job.wrap.max_width = wrap;
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                if has_focus && st.canvas.popup.is_none() {
                    let shift = ui.input(|i| i.modifiers.shift);
                    if let Some(c) = egui::TextEdit::load_state(&ctx, id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0) {
                        let mut t = text.clone();
                        if let Some(nc) = rich::indent_line(&mut t, c, shift) {
                            if ui.input_mut(|i| i.consume_key(if shift { Modifiers::SHIFT } else { Modifiers::NONE }, Key::Tab)) {
                                *text = t;
                                changed = true;
                                set_cursor(&ctx, id, nc);
                            }
                        }
                    }
                }
                let out = egui::TextEdit::multiline(text)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .desired_rows(1)
                    .desired_width(w)
                    .hint_text(tr("Type here…  **bold**, - list, $x^2$, / for more, [[ to link a page"))
                    .layouter(&mut layouter)
                    .show(ui);
                let r = &out.response;
                focus(r, st);
                if let Some((_, p)) = st.canvas.click_at.take_if(|(bid, _)| *bid == b.id) {
                    set_cursor(&ctx, id, out.galley.cursor_from_pos(p - out.galley_pos).index.0);
                }
                let (picked, popup_interacting) = popup_ui(ui, &ctx, id, b.id, text, &out, st);
                if let Some(i) = picked {
                    changed |= apply_popup(&ctx, id, b.id, text, i, st);
                }
                let r = &out.response;
                if r.gained_focus() {
                    st.canvas.editing = Some(b.id);
                    st.canvas.last_text = Some(b.id);
                    st.canvas.want_snap = true;
                }
                if r.has_focus() {
                    st.canvas.last_text = Some(b.id);
                }
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
                if r.has_focus() && ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter)) {
                    st.canvas.insert_below = Some((b.id, Kind::Text { text: String::new(), heading: 0 }));
                }
                if r.has_focus() {
                    if let Some(c) = out.cursor_range.map(|cr| cr.primary.index.0) {
                        let line = text.chars().take(c).collect::<String>().rsplit('\n').next().unwrap_or("").to_string()
                            + &text.chars().skip(c).take_while(|ch| *ch != '\n').collect::<String>();
                        let maths: Vec<String> = rich::pieces_cached(&line).into_iter().filter_map(|p| if let rich::Piece::Math(m) = p { Some(m) } else { None }).collect();
                        if !maths.is_empty() {
                            egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    for m in &maths {
                                        math::math_label(ui, m, size);
                                        ui.add_space(12.0);
                                    }
                                });
                            });
                        }
                    }
                }
                if r.lost_focus() && !popup_interacting && st.canvas.editing == Some(b.id) && st.canvas.fmt.is_none() && st.nav.focus_id != Some(id) {
                    st.canvas.editing = None;
                }
            } else {
                let (clicked, link, tick) = rich::view_ticks(ui, text, size, hd, &colors);
                if let Some(n) = tick {
                    rich::toggle_check(text, n);
                    changed = true;
                    st.canvas.snap_press = true;
                } else if let Some(l) = link {
                    st.canvas.open_link = Some(l);
                } else if clicked {
                    st.canvas.editing = Some(b.id);
                    st.nav.focus_id = Some(id);
                    st.canvas.last_text = Some(b.id);
                    let to_page = ui.ctx().layer_transform_from_global(ui.layer_id()).unwrap_or_default();
                    st.canvas.click_at = ui.input(|i| i.pointer.interact_pos()).map(|p| (b.id, to_page * p));
                }
            }
        }
        Kind::Math { src } => {
            if st.canvas.editing == Some(b.id) {
                egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(8).inner_margin(10).show(ui, |ui| {
                    ui.set_width((w - 20.0).max(80.0));
                    if src.trim().is_empty() {
                        ui.label(RichText::new(tr("Type LaTeX below, or tap the symbols.")).weak());
                    } else {
                        math::math_label(ui, src, 22.0);
                    }
                    let unknown = math::unknown_commands(src);
                    if !unknown.is_empty() {
                        ui.label(RichText::new(tf!("Not known here: {cmds} (a typo?)", cmds = unknown.join(", "))).size(11.5).color(Color32::from_rgb(214, 70, 60)));
                    }
                    ui.add_space(6.0);
                    let r = ui.add(egui::TextEdit::multiline(src).id(id).code_editor().desired_rows(1).desired_width(f32::INFINITY).hint_text(tr(r"e.g. \frac{a}{b} or x^2 + y^2 = r^2")));
                    focus(&r, st);
                    changed |= r.changed();
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);
                        for (label, snip) in MATH_PALETTE {
                            if ui.add(egui::Button::new(RichText::new(*label).size(13.0)).min_size(Vec2::new(28.0, 24.0))).tip(*snip).clicked() {
                                rich::edit_at_cursor(&ctx, id, src, |t, a, b| insert_snippet(t, a, b, snip));
                                st.nav.focus_id = Some(id);
                                changed = true;
                            }
                        }
                    });
                    ui.add_space(4.0);
                    if ui.add(egui::Button::new(RichText::new(tr("Done")).color(Color32::WHITE)).fill(accent_base())).clicked() {
                        st.canvas.editing = None;
                    }
                });
            } else if src.trim().is_empty() {
                if ui.add(egui::Label::new(RichText::new(tr("∑  Empty equation — click to write one")).weak()).sense(Sense::click())).clicked() {
                    st.canvas.editing = Some(b.id);
                    st.nav.focus_id = Some(id);
                }
            } else if math::math_label(ui, src, 22.0).tip("Click to edit").clicked() {
                st.canvas.editing = Some(b.id);
                st.nav.focus_id = Some(id);
            }
        }
        Kind::Checklist { items } => {
            if st.nav.focus_id == Some(id) {
                st.nav.focus_id = Some(egui::Id::unique(("chk", b.id, items.len().saturating_sub(1))));
            }
            let mut remove = None;
            let mut new_item: Option<(usize, Vec<String>)> = None;
            let n = items.len();
            ui.spacing_mut().item_spacing.y = 8.0;
            for (i, (done, t)) in items.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if ui.checkbox(done, "").changed() {
                        changed = true;
                        st.canvas.snap_press = true;
                    }
                    let was_empty = t.is_empty();
                    let te_w = (w - 40.0 - if active && n > 1 { 30.0 } else { 0.0 }).max(48.0);
                    let mut te = egui::TextEdit::multiline(t)
                        .id(egui::Id::unique(("chk", b.id, i)))
                        .frame(egui::Frame::NONE)
                        .desired_rows(1)
                        .desired_width(te_w)
                        .hint_text(tr("To do…"));
                    if *done {
                        te = te.text_color(ui.visuals().weak_text_color());
                    }
                    let r = ui.add(te);
                    focus(&r, st);
                    changed |= r.changed();
                    if let Some(nl) = t.find('\n') {
                        let rest: Vec<String> = t[nl + 1..].split('\n').map(String::from).collect();
                        t.truncate(nl);
                        new_item = Some((i, rest));
                    }
                    if r.has_focus() && was_empty && n > 1 && ui.input(|i| i.key_pressed(Key::Backspace)) {
                        remove = Some(i);
                    }
                    if active && n > 1 && ui.add(egui::Button::new(RichText::new("×").weak()).frame_when_inactive(false)).tip("Remove this item").clicked() {
                        remove = Some(i);
                        st.canvas.snap_press = true;
                    }
                });
            }
            if let Some((i, rest)) = new_item {
                let k = rest.len();
                let pasted = rest.into_iter().filter(|t| !t.trim().is_empty() || k == 1).map(|t| (false, t));
                let k = {
                    let before = items.len();
                    let tail = items.split_off(i + 1);
                    items.extend(pasted);
                    let k = items.len() - before;
                    items.extend(tail);
                    k
                };
                st.nav.focus_id = Some(egui::Id::unique(("chk", b.id, i + k.max(1))));
                changed = true;
            }
            if let Some(i) = remove {
                items.remove(i);
                st.nav.focus_id = Some(egui::Id::unique(("chk", b.id, i.saturating_sub(1))));
                changed = true;
            }
            let done = items.iter().filter(|i| i.0).count();
            ui.horizontal(|ui| {
                if items.len() > 1 {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(80.0, 4.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, ui.visuals().widgets.inactive.weak_bg_fill);
                    ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width() * done as f32 / items.len() as f32, r.height())), 2.0, acc);
                    ui.label(RichText::new(tf!("{done} of {items_n} done", done = done, items_n = items.len())).size(11.0).weak());
                }
                if active && ui.small_button(tr("+ item")).clicked() {
                    st.canvas.snap_press = true;
                    items.push((false, String::new()));
                    st.nav.focus_id = Some(egui::Id::unique(("chk", b.id, items.len() - 1)));
                    changed = true;
                }
            });
        }
        Kind::Table { rows } => {
            let mut add_row = false;
            let mut next_cell: Option<(usize, usize)> = None;
            let cols = rows.first().map(|r| r.len()).unwrap_or(1).max(1);
            let bold = rich::HAS_BOLD.load(std::sync::atomic::Ordering::Relaxed);
            let head_font = if bold { FontId::new(14.0, egui::FontFamily::Name("bold".into())) } else { FontId::proportional(14.0) };
            // Proportionally fill the block width `w` across all columns, wrapping long text inside each cell.
            let spacing_x = 10.0;
            let inner_w = (w - 14.0 - (cols.saturating_sub(1) as f32 * spacing_x)).max(cols as f32 * 28.0);
            let natural: Vec<f32> = (0..cols)
                .map(|c| {
                    rows.iter()
                        .enumerate()
                        .filter_map(|(ri, r)| r.get(c).map(|t| (ri, t)))
                        .map(|(ri, t)| {
                            let longest = t.lines().map(|l| ui.painter().layout_no_wrap(l.to_string(), if ri == 0 { head_font.clone() } else { FontId::proportional(14.0) }, Color32::WHITE).size().x).fold(0.0, f32::max);
                            (longest + 14.0).clamp(28.0, inner_w * 0.65)
                        })
                        .fold(28.0, f32::max)
                })
                .collect();
            let sum: f32 = natural.iter().sum::<f32>().max(1.0);
            let colw: Vec<f32> = natural.iter().map(|&cw| (cw * inner_w / sum).max(26.0)).collect();
            egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).corner_radius(6).inner_margin(6).show(ui, |ui| {
                ui.set_width((w - 12.0).max(80.0));
                let n_rows = rows.len();
                egui::Grid::new(("tbl", b.id)).striped(true).spacing([spacing_x, 6.0]).show(ui, |ui| {
                    for (ri, row) in rows.iter_mut().enumerate() {
                        for (ci, cell) in row.iter_mut().enumerate() {
                            let cell_id = egui::Id::unique(("cell", b.id, ri, ci));
                            let cell_has_focus = ui.memory(|m| m.has_focus(cell_id));
                            // Tab moves to next cell (or adds a row on the last cell); Shift+Tab moves back.
                            if cell_has_focus {
                                if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab)) {
                                    if ci + 1 < cols {
                                        next_cell = Some((ri, ci + 1));
                                    } else if ri + 1 < n_rows {
                                        next_cell = Some((ri + 1, 0));
                                    } else {
                                        add_row = true;
                                    }
                                } else if ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Tab)) {
                                    if ci > 0 {
                                        next_cell = Some((ri, ci - 1));
                                    } else if ri > 0 {
                                        next_cell = Some((ri - 1, cols - 1));
                                    }
                                }
                            }
                            let cw = colw[ci.min(cols - 1)];
                            ui.allocate_ui_with_layout(Vec2::new(cw, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.set_width(cw);
                                let mut te = egui::TextEdit::multiline(cell)
                                    .desired_rows(1)
                                    .desired_width(cw)
                                    .frame(egui::Frame::NONE)
                                    .id(cell_id);
                                if ri == 0 {
                                    te = te.font(head_font.clone()).text_color(acc);
                                }
                                let r = ui.add(te);
                                ui.painter().hline(r.rect.x_range(), r.rect.bottom() + 2.0, ui.visuals().widgets.noninteractive.bg_stroke);
                                if (ri == 0 && ci == 0 && st.nav.focus_id == Some(id)) || st.nav.focus_id == Some(r.id) {
                                    r.request_focus();
                                    st.nav.focus_id = None;
                                }
                                if r.gained_focus() {
                                    st.canvas.editing = Some(bid);
                                    st.canvas.want_snap = true;
                                }
                                changed |= r.changed();
                            });
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some((ri, ci)) = next_cell {
                st.nav.focus_id = Some(egui::Id::unique(("cell", b.id, ri, ci)));
            }
            if add_row {
                st.canvas.snap_press = true;
                rows.push(vec![String::new(); cols]);
                st.nav.focus_id = Some(egui::Id::unique(("cell", b.id, rows.len() - 1, 0)));
                changed = true;
            }
            if active {
                ui.horizontal_wrapped(|ui| {
                    let before = (rows.len(), cols);
                    if ui.small_button(tr("+ row")).clicked() { rows.push(vec![String::new(); cols]); changed = true; }
                    if ui.small_button(tr("+ column")).clicked() { for r in rows.iter_mut() { r.push(String::new()); } changed = true; }
                    if rows.len() > 1 && ui.small_button(tr("− row")).clicked() { rows.pop(); changed = true; }
                    if cols > 1 && ui.small_button(tr("− column")).clicked() { for r in rows.iter_mut() { r.pop(); } changed = true; }
                    if before != (rows.len(), rows.first().map_or(0, |r| r.len())) {
                        st.canvas.snap_press = true;
                    }
                });
            }
        }
        Kind::Code { lang, code } => {
            let dark = ui.visuals().dark_mode;
            egui::Frame::new()
                .fill(if dark { Color32::from_rgb(20, 24, 25) } else { Color32::from_rgb(244, 247, 248) })
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .corner_radius(8)
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("{ }").size(11.0).color(acc));
                        let r = ui.add(egui::TextEdit::singleline(lang).desired_width(90.0).font(egui::TextStyle::Small).frame(egui::Frame::NONE));
                        if r.gained_focus() {
                            st.canvas.editing = Some(bid);
                            st.canvas.want_snap = true;
                        }
                        changed |= r.changed();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(tr("⧉ Copy")).clicked() {
                                ui.ctx().copy_text(code.clone());
                            }
                        });
                    });
                    let r = ui.add(egui::TextEdit::multiline(code).id(id).code_editor().desired_rows(2).desired_width((w - 16.0).max(60.0)).frame(egui::Frame::NONE));
                    focus(&r, st);
                    changed |= r.changed();
                });
        }
        Kind::Image { file } => {
            let est = Vec2::new(w, st.canvas.heights.get(&b.id).copied().unwrap_or(w * 1.3));
            let near = ui.is_rect_visible(Rect::from_min_size(ui.cursor().min, est).expand(600.0));
            let frame = ui.input(|i| i.time);
            let cached = match st.cache.textures.get_mut(file) {
                Some((t, _, used)) => {
                    if near {
                        *used = frame;
                    }
                    Some(t.clone())
                }
                None => None,
            };
            let tex = match cached {
                Some(t) => t,
                None if st.canvas.capture.is_some() => {
                    // Synchronous fallback during tile capture so exported PDFs never contain a loading placeholder.
                    let loaded = (|| -> Option<(egui::TextureHandle, usize)> {
                        let mut img = image::ImageReader::open(files_path(file)).ok()?.with_guessed_format().ok()?.decode().ok()?;
                        if img.width().max(img.height()) > 4096 {
                            img = img.resize(4096, 4096, image::imageops::FilterType::Triangle);
                        }
                        let img = img.to_rgba8();
                        let bytes = (img.width() as usize).saturating_mul(img.height() as usize).saturating_mul(4);
                        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                        Some((ui.ctx().load_texture(file.clone(), ci, egui::TextureOptions::LINEAR), bytes))
                    })();
                    let t = loaded.as_ref().map(|(t, _)| t.clone());
                    let bytes = loaded.as_ref().map_or(0, |(_, b)| *b);
                    if t.is_some() || files_path(file).exists() {
                        st.cache.textures.insert(file.clone(), (t.clone(), bytes, frame));
                    }
                    t
                }
                None => {
                    let (rect, _) = ui.allocate_exact_size(est, Sense::hover());
                    if near {
                        if !files_path(file).exists() {
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                tr("⚠ This picture's file is missing"),
                                FontId::proportional(13.0),
                                ui.visuals().weak_text_color(),
                            );
                        } else {
                            st.cache.request_image(file.clone(), files_path(file));
                            ui.painter().rect_filled(rect, 8.0, ui.visuals().faint_bg_color);
                            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_center_size(rect.center(), Vec2::splat(24.0))));
                            child.spinner();
                        }
                    }
                    return changed;
                }
            };
            match &tex {
                Some(t) => {
                    let s = t.size_vec2();
                    let size = Vec2::new(w, w * s.y / s.x.max(1.0));
                    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                    ui.painter().image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    ui.label(RichText::new(tr("⚠ This picture's file is missing")).weak());
                }
            }
        }
        Kind::File { file, name, size } => {
            egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(8).inner_margin(8).show(ui, |ui| {
                ui.set_width((w - 16.0).max(80.0));
                ui.horizontal(|ui| {
                    ui.label(RichText::new("📄").size(24.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr("Open")).clicked() {
                            open_path_named(&files_path(file), name);
                        }
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.add(egui::Label::new(RichText::new(&*name).strong()).truncate());
                            ui.label(RichText::new(tf!("{x} KB", x = format!("{:.1}", *size as f64 / 1024.0))).small().weak());
                        });
                    });
                });
            });
        }
        Kind::Link { url, title } => {
            if st.canvas.editing == Some(b.id) {
                let fw = (w - 80.0).max(80.0);
                egui::Grid::new(("link", b.id)).num_columns(2).show(ui, |ui| {
                    ui.label(tr("Address"));
                    let r = ui.add(egui::TextEdit::singleline(url).id(id).desired_width(fw));
                    focus(&r, st);
                    changed |= r.changed();
                    ui.end_row();
                    ui.label(tr("Title"));
                    let r2 = ui.add(egui::TextEdit::singleline(title).hint_text(tr("optional")).desired_width(fw));
                    if r2.gained_focus() {
                        st.canvas.editing = Some(bid);
                        st.canvas.want_snap = true;
                    }
                    changed |= r2.changed();
                    ui.end_row();
                });
                if ui.add(egui::Button::new(RichText::new(tr("Done")).color(Color32::WHITE)).fill(accent_base())).clicked() {
                    st.canvas.editing = None;
                }
            } else {
                egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(8).inner_margin(8).show(ui, |ui| {
                    ui.set_width((w - 16.0).max(80.0));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🔗").size(18.0));
                        ui.vertical(|ui| {
                            let safe = ["https://", "http://", "mailto:"].iter().any(|p| url.to_ascii_lowercase().starts_with(p));
                            if safe {
                                ui.hyperlink_to(RichText::new(if title.is_empty() { url.as_str() } else { title.as_str() }).size(14.0), url.as_str());
                            } else {
                                ui.label(RichText::new(if title.is_empty() { url.as_str() } else { title.as_str() }).size(14.0));
                            }
                            if !title.is_empty() {
                                ui.add(egui::Label::new(RichText::new(url.as_str()).size(11.0).weak()).truncate());
                            }
                        });
                    });
                });
            }
        }
    }
    changed
}
