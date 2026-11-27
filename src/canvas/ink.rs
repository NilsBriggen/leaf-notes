//! Ink drawing, smoothing, painting, erasing, lasso selection, and transform handles.

use crate::*;
use eframe::egui::{Align2, FontId, Rect, Sense};

/// Where the Delete and Duplicate buttons sit under a selection box `r` (page coordinates).
pub(crate) fn sel_buttons(r: Rect) -> [Rect; 2] {
    let at = r.left_bottom() + Vec2::new(0.0, 8.0);
    [Rect::from_min_size(at, Vec2::new(40.0, 32.0)), Rect::from_min_size(at + Vec2::new(46.0, 0.0), Vec2::new(40.0, 32.0))]
}

/// A tap at `p` (page coordinates) on a selection button: 0 deletes the selected ink, 1 duplicates it.
pub(crate) fn sel_button_tap(page: &mut Page, st: &mut St, p: Pos2) -> bool {
    let Some(r) = selection_bounds(page, &st.canvas.selected) else { return false };
    let Some(k) = sel_buttons(r.expand(6.0)).iter().position(|b| b.contains(p)) else { return false };
    st.snap(page);
    if k == 0 {
        let sel = std::mem::take(&mut st.canvas.selected);
        page.strokes.retain(|s| !sel.contains(&s.id));
        st.say("Deleted. Undo brings it back.");
    } else {
        let copies: Vec<Stroke> = page
            .strokes
            .iter()
            .filter(|s| st.canvas.selected.contains(&s.id))
            .map(|s| Stroke { id: new_id(), points: s.points.iter().map(|p| [p[0] + 24.0, p[1] + 24.0, p[2]]).collect(), ..s.clone() })
            .collect();
        st.canvas.selected = copies.iter().map(|s| s.id).collect();
        page.strokes.extend(copies);
    }
    true
}

/// Handles on the selected ink: bottom-right corner resizes, the knob above rotates.
pub(crate) fn selection_handles(ui: &mut egui::Ui, o: Pos2, page: &mut Page, st: &mut St) {
    let Some(r) = selection_bounds(page, &st.canvas.selected) else { return };
    let r = r.expand(6.0);
    let acc = accent(ui);
    let (corner, knob) = (r.right_bottom(), r.center_top() - Vec2::new(0.0, 24.0));
    let painter = ui.painter();
    for (b, icon) in sel_buttons(r).into_iter().zip(["🗑", "⧉"]) {
        let b = b.translate(o.to_vec2());
        painter.rect(b, 8.0, ui.visuals().widgets.inactive.weak_bg_fill, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
        painter.text(b.center(), Align2::CENTER_CENTER, icon, FontId::proportional(15.0), ui.visuals().text_color());
    }
    painter.line_segment([o + r.center_top().to_vec2(), o + knob.to_vec2()], EStroke::new(1.2, acc));
    for (c, round) in [(corner, false), (knob, true)] {
        let c = o + c.to_vec2();
        if round {
            painter.circle(c, 6.0, Color32::WHITE, EStroke::new(1.5, acc));
        } else {
            painter.rect(Rect::from_center_size(c, Vec2::splat(11.0)), 2.0, Color32::WHITE, EStroke::new(1.5, acc), egui::StrokeKind::Middle);
        }
    }
    for (kind, c) in [(0u8, corner), (1u8, knob)] {
        let resp = ui.interact(
            Rect::from_center_size(o + c.to_vec2(), Vec2::splat(if st.nav.touch_ui { 36.0 } else { 22.0 })),
            ui.scope_id().with(("sel-handle", kind)),
            Sense::drag(),
        );
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if kind == 0 { egui::CursorIcon::ResizeNwSe } else { egui::CursorIcon::Grab });
        }
        if resp.drag_started() {
            st.snap(page);
            st.canvas.xform = Some((kind, if kind == 0 { r.left_top() + Vec2::splat(6.0) } else { r.center() }));
        }
        if resp.drag_stopped() {
            st.canvas.xform = None;
        }
        let (Some((k, pivot)), Some(pos)) = (st.canvas.xform, resp.interact_pointer_pos()) else { continue };
        if k != kind || !resp.dragged() {
            continue;
        }
        let p = pos - o.to_vec2();
        let prev = p - resp.drag_delta();
        let sel = st.canvas.selected.clone();
        if k == 0 {
            let f = ((p - pivot).length() / (prev - pivot).length().max(1.0)).clamp(0.5, 2.0);
            if (prev - pivot).length() < 4.0 {
                continue;
            }
            for s in page.strokes.iter_mut().filter(|s| sel.contains(&s.id)) {
                for q in &mut s.points {
                    q[0] = pivot.x + (q[0] - pivot.x) * f;
                    q[1] = pivot.y + (q[1] - pivot.y) * f;
                }
                s.width = (s.width * f).clamp(0.3, 60.0);
            }
        } else {
            let a = (p - pivot).angle() - (prev - pivot).angle();
            let (sn, cs) = a.sin_cos();
            for s in page.strokes.iter_mut().filter(|s| sel.contains(&s.id)) {
                for q in &mut s.points {
                    let (dx, dy) = (q[0] - pivot.x, q[1] - pivot.y);
                    q[0] = pivot.x + dx * cs - dy * sn;
                    q[1] = pivot.y + dx * sn + dy * cs;
                }
            }
        }
        st.dirty = true;
    }
}

pub(crate) fn selection_bounds(page: &Page, sel: &[u64]) -> Option<Rect> {
    if sel.is_empty() {
        return None;
    }
    let pts: Vec<Pos2> = page
        .strokes
        .iter()
        .filter(|s| sel.contains(&s.id))
        .flat_map(|s| s.points.iter().map(|p| Pos2::new(p[0], p[1])))
        .collect();
    (!pts.is_empty()).then(|| Rect::from_points(&pts))
}

fn stroke_extent(s: &Stroke) -> f32 {
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in &s.points {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (hi[0] - lo[0]).max(hi[1] - lo[1])
}

pub(crate) fn paint_stroke(painter: &egui::Painter, s: &Stroke, o: Pos2) {
    if s.points.is_empty() {
        return;
    }
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in &s.points {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let bounds = Rect::from_min_max(Pos2::new(o.x + lo[0], o.y + lo[1]), Pos2::new(o.x + hi[0], o.y + hi[1])).expand(s.width * 2.0 + 2.0);
    if !painter.clip_rect().intersects(bounds) {
        return;
    }
    let col = c4(s.color);
    let pts: Vec<Pos2> = s.points.iter().map(|p| Pos2::new(o.x + p[0], o.y + p[1])).collect();
    let w0 = seg_width(s, s.points[0][2], 1.0, 0.0);
    if pts.len() == 1 {
        painter.circle_filled(pts[0], w0 * 0.5, col);
        return;
    }
    let even = s.highlighter || s.pen == PenKind::Marker || (s.pen == PenKind::Ballpoint && s.points.iter().all(|p| (p[2] - s.points[0][2]).abs() < 0.05));
    if even {
        painter.add(egui::Shape::line(pts, EStroke::new(w0, col)));
        return;
    }
    if s.pen == PenKind::Calligraphy {
        paint_calligraphy(painter, s, &pts, col);
        return;
    }
    for (i, w) in pts.windows(2).enumerate() {
        let (dx, dy) = (w[1].x - w[0].x, w[1].y - w[0].y);
        let pw = seg_width(s, (s.points[i][2] + s.points[i + 1][2]) * 0.5, dx, dy);
        match s.pen {
            PenKind::Calligraphy => unreachable!("drawn whole by paint_calligraphy"),
            PenKind::Pencil => {
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

/// The live part of smoothing: each new point trails the pen a little.
pub(crate) fn trail(last: [f32; 3], x: f32, y: f32, stabilizer: f32) -> (f32, f32) {
    let k = 1.0 - stabilizer.clamp(0.0, 1.0) * 0.75;
    (last[0] + (x - last[0]) * k, last[1] + (y - last[1]) * k)
}

/// The finishing part of smoothing, applied when the pen lifts.
pub(crate) fn smooth_passes(points: &mut [[f32; 3]], stabilizer: f32) {
    if points.len() < 3 {
        return;
    }
    for _ in 0..1 + (stabilizer * 3.0).round() as usize {
        let src = points.to_vec();
        for i in 1..src.len() - 1 {
            for k in 0..3 {
                points[i][k] = (src[i - 1][k] + 2.0 * src[i][k] + src[i + 1][k]) / 4.0;
            }
        }
    }
}

/// Line width for this pen at pressure `p`, moving in direction (dx, dy).
pub(crate) fn seg_width(s: &Stroke, p: f32, dx: f32, dy: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    if s.highlighter {
        return s.width;
    }
    match s.pen {
        PenKind::Fountain => s.width * (0.35 + 0.65 * p),
        PenKind::Ballpoint => s.width * (0.8 + 0.2 * p),
        PenKind::Pencil => s.width * (0.55 + 0.45 * p),
        PenKind::Calligraphy => s.width * 1.8 * (0.18 + 0.82 * (dy.atan2(dx) - std::f32::consts::FRAC_PI_4).sin().abs()),
        PenKind::Marker => s.width * 1.6,
    }
}

pub(crate) fn point_in_poly(p: Pos2, poly: &[Pos2]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Pen, highlighter, shapes, eraser and lasso.
pub(crate) fn ink_input(ui: &mut egui::Ui, rect: Rect, page: &mut Page, st: &mut St) -> Option<Pos2> {
    let from_global = ui.ctx().layer_transform_from_global(ui.layer_id()).unwrap_or_default();
    if ui.input(|i| i.multi_touch()).is_some() {
        if st.canvas.input == Input::Pen && st.canvas.current.is_some() {
            return None;
        }
        st.canvas.current = None;
        st.canvas.lasso.clear();
        // egui::Scene already applies multi-touch pan/zoom; do not double-pan here.
        return None;
    }
    // Never draw through an open dropdown/popup menu; clicking dismisses the popup cleanly.
    if egui::Popup::is_any_open(ui.ctx()) {
        return None;
    }
    let resp = ui.interact(rect, ui.scope_id().with("ink"), Sense::click_and_drag());
    let o = rect.min;
    if st.canvas.ink_pass {
        if resp.dragged_by(egui::PointerButton::Primary) {
            st.canvas.pan += resp.drag_delta();
        }
        if resp.drag_started_by(egui::PointerButton::Primary) && st.canvas.input == Input::Mouse && !st.canvas.mouse_hint && st.canvas.tool != Tool::Type {
            st.canvas.mouse_hint = true;
            st.say("Tip: your stylus draws and the mouse works as usual. To draw with the mouse too, switch it on in Settings ▸ Pen & input.");
        }
        return resp.clicked().then(|| resp.interact_pointer_pos()).flatten();
    }
    let shift = ui.input(|i| i.modifiers.shift);
    let (primary_pressed, primary_down, primary_released) = ui.input(|i| (
        i.pointer.button_pressed(egui::PointerButton::Primary),
        i.pointer.button_down(egui::PointerButton::Primary),
        i.pointer.button_released(egui::PointerButton::Primary),
    ));
    let pointer_pos = resp.interact_pointer_pos().or_else(|| {
        if st.canvas.current.is_some() || primary_pressed {
            ui.input(|i| i.pointer.latest_pos()).map(|q| from_global * q)
        } else {
            None
        }
    });
    if let Some(pos) = pointer_pos {
        let p = pos - o.to_vec2();
        let pen_like = matches!(st.canvas.tool, Tool::Pen | Tool::Highlighter);
        // Start pen/highlighter strokes immediately on primary press (no drag-threshold drop on Attempt 1 or dots).
        let started = if pen_like {
            st.canvas.current.is_none() && ((primary_pressed && resp.contains_pointer()) || resp.drag_started_by(egui::PointerButton::Primary))
        } else {
            resp.drag_started_by(egui::PointerButton::Primary)
        };
        if started {
            match st.canvas.tool {
                Tool::Pen | Tool::Highlighter => {
                    let hl = st.canvas.tool == Tool::Highlighter;
                    let start = ui.input(|i| i.pointer.press_origin()).map_or(p, |q| from_global * q - o.to_vec2());
                    let pen = *st.pen();
                    st.canvas.snapped = false;
                    st.canvas.still = (start, ui.input(|i| i.time));
                    st.canvas.current = Some(Stroke {
                        id: new_id(),
                        points: vec![[
                            start.x,
                            start.y,
                            if st.p.pressure {
                                st.canvas.frame_force.map(|f| f.clamp(0.0, 1.0).powf(st.p.pressure_curve)).unwrap_or(0.8)
                            } else {
                                0.85
                            },
                        ]],
                        color: if hl { st.p.hl_color } else { pen.color },
                        width: if hl { st.p.hl_width } else { pen.width },
                        highlighter: hl,
                        pen: pen.kind,
                        t: now_f(),
                    });
                }
                Tool::Eraser => st.canvas.snapped = false,
                Tool::Lasso => {
                    let sel = selection_bounds(page, &st.canvas.selected);
                    if sel.is_some_and(|r| sel_buttons(r.expand(6.0)).iter().any(|b| b.contains(p))) {
                        st.canvas.sel_press = Some(p);
                    } else if sel.is_some_and(|r| r.expand(8.0).contains(p)) {
                        st.canvas.moving = true;
                        st.snap(page);
                    } else {
                        st.canvas.lasso = vec![p];
                        st.clear_selection();
                    }
                }
                Tool::Shape(_) => {
                    st.canvas.anchor = Some([p.x, p.y]);
                    let pen = *st.pen();
                    st.canvas.current = Some(Stroke {
                        id: new_id(),
                        points: vec![],
                        color: pen.color,
                        width: pen.width.max(1.5),
                        highlighter: false,
                        pen: PenKind::Ballpoint,
                        t: now_f(),
                    });
                }
                Tool::Type => {}
            }
        }
        let active_draw = if pen_like {
            primary_down && st.canvas.current.is_some()
        } else {
            resp.dragged_by(egui::PointerButton::Primary)
        };
        if active_draw {
            match st.canvas.tool {
                Tool::Pen | Tool::Highlighter => {
                    if let Some(s) = &mut st.canvas.current {
                        let now = ui.input(|i| i.time);
                        if (p - st.canvas.still.0).length() > 3.0 * from_global.scaling {
                            st.canvas.still = (p, now);
                        }
                        if st.p.snap_shapes && !st.canvas.snapped && !s.highlighter && now - st.canvas.still.1 > st.p.snap_delay as f64 && s.points.len() > 8 {
                            if let Some((pts, kind)) = snap_shape(&s.points) {
                                s.points = pts;
                                st.canvas.snapped = true;
                                st.status = tf!("Snapped to a {kind}.", kind = tr(kind));
                                st.status_age = 0.0;
                            }
                            st.canvas.still.1 = now;
                        }
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                        let samples: Vec<(Pos2, Option<f32>)> = ui.input(|i| {
                            let touch: Vec<(Pos2, Option<f32>)> = i
                                .events
                                .iter()
                                .filter_map(|e| match e {
                                    egui::Event::Touch { pos, force: Some(f), phase: egui::TouchPhase::Move, .. } => {
                                        Some((*pos, Some(if *f > 1.0 { f - 1.0 } else { f.abs() })))
                                    }
                                    _ => None,
                                })
                                .collect();
                            if !touch.is_empty() {
                                return touch;
                            }
                            i.events
                                .iter()
                                .filter_map(|e| match e {
                                    egui::Event::PointerMoved(q) => Some((*q, None)),
                                    _ => None,
                                })
                                .collect()
                        });
                        let samples: Vec<(Pos2, Option<f32>)> = if samples.is_empty() {
                            vec![(p, None)]
                        } else {
                            samples.into_iter().map(|(q, f)| (from_global * q - o.to_vec2(), f)).collect()
                        };
                        for (q, force) in samples {
                            let last = s.points.last().copied().unwrap_or([q.x, q.y, 0.8]);
                            let d = (last[0] - q.x).hypot(last[1] - q.y);
                            if d > 1.0 && !st.canvas.snapped {
                                let pr = if !st.p.pressure {
                                    0.85
                                } else {
                                    force
                                        .or(st.canvas.frame_force)
                                        .map(|f| f.clamp(0.0, 1.0).powf(st.p.pressure_curve))
                                        .unwrap_or_else(|| last[2] * 0.75 + (1.1 - d / 28.0).clamp(0.45, 1.0) * 0.25)
                                };
                                let (x, y) = trail(last, q.x, q.y, st.p.stabilizer);
                                s.points.push([x, y, pr]);
                            }
                        }
                    }
                }
                Tool::Eraser => {
                    let (r, partial) = (st.p.eraser_size, st.p.erase_partial);
                    let prev = p - resp.drag_delta();
                    let n = ((p - prev).length() / (r * 0.5).max(1.0)).ceil().clamp(1.0, 50.0) as usize;
                    for k in 1..=n {
                        let q = prev + (p - prev) * (k as f32 / n as f32);
                        let reach = |s: &Stroke| if partial { r * 0.9 + s.width * 0.5 } else { r + 1.0 + s.width * 0.5 };
                        if !page.strokes.iter().any(|s| stroke_hit(s, [q.x, q.y], reach(s))) {
                            continue;
                        }
                        if !st.canvas.snapped {
                            st.snap(page);
                            st.canvas.snapped = true;
                        }
                        if partial {
                            erase_partial(&mut page.strokes, [q.x, q.y], r * 0.9);
                        } else {
                            page.strokes.retain(|s| !stroke_hit(s, [q.x, q.y], reach(s)));
                        }
                        st.dirty = true;
                    }
                }
                Tool::Lasso => {
                    if st.canvas.moving {
                        let d = resp.drag_delta();
                        for s in page.strokes.iter_mut().filter(|s| st.canvas.selected.contains(&s.id)) {
                            for q in &mut s.points {
                                q[0] += d.x;
                                q[1] += d.y;
                            }
                        }
                    } else if st.canvas.sel_press.is_none() {
                        st.canvas.lasso.push(p);
                    }
                }
                Tool::Shape(shape) => {
                    if let (Some(a), Some(cur)) = (st.canvas.anchor, &mut st.canvas.current) {
                        let mut b = [p.x, p.y];
                        if !shift && matches!(shape, Shape::Line | Shape::Arrow) {
                            b = level(a, b);
                        }
                        if shift {
                            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                            if matches!(shape, Shape::Rect | Shape::Ellipse) {
                                let m = dx.abs().max(dy.abs());
                                b = [a[0] + m * dx.signum(), a[1] + m * dy.signum()];
                            } else {
                                let ang = (dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
                                let len = dx.hypot(dy);
                                b = [a[0] + len * ang.cos(), a[1] + len * ang.sin()];
                            }
                        }
                        cur.points = shape_points(shape, a, b);
                    }
                }
                Tool::Type => {}
            }
        }
    }
    let stopped = resp.drag_stopped_by(egui::PointerButton::Primary)
        || (primary_released && (st.canvas.current.is_some() || !st.canvas.lasso.is_empty() || st.canvas.moving || st.canvas.sel_press.is_some()));
    if stopped {
        match st.canvas.tool {
            Tool::Pen | Tool::Highlighter | Tool::Shape(_) => {
                if let Some(mut s) = st.canvas.current.take() {
                    let mut straight = false;
                    if s.highlighter && st.p.straighten_hl && s.points.len() > 1 {
                        let (a, b) = (s.points[0], *s.points.last().unwrap());
                        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                        let off = s
                            .points
                            .iter()
                            .map(|q| ((b[1] - a[1]) * q[0] - (b[0] - a[0]) * q[1] + b[0] * a[1] - b[1] * a[0]).abs() / len.max(1.0))
                            .fold(0.0, f32::max);
                        if len > 40.0 && off < 6.0 {
                            s.points = densify(&[a, b], 6.0);
                            straight = true;
                        }
                    }
                    if !straight && !matches!(st.canvas.tool, Tool::Shape(_)) && !st.canvas.snapped && s.points.len() > 3 {
                        if let Some(end) = resp.interact_pointer_pos().or(ui.input(|i| i.pointer.latest_pos()).map(|q| from_global * q)) {
                            let e = end - o.to_vec2();
                            let last = *s.points.last().unwrap();
                            s.points.push([e.x, e.y, last[2]]);
                        }
                        smooth_passes(&mut s.points, st.p.stabilizer);
                    }
                    let gone = if st.p.scratch_out && !s.highlighter && !st.canvas.snapped && st.canvas.tool == Tool::Pen {
                        crate::model::scratched(&s.points, &page.strokes)
                    } else {
                        vec![]
                    };
                    if !gone.is_empty() {
                        st.snap(page);
                        page.strokes.retain(|k| !gone.contains(&k.id));
                        st.say(tf!("Scribbled out {n} stroke(s). Undo brings them back.", n = gone.len()));
                    } else if !s.points.is_empty() && !(matches!(st.canvas.tool, Tool::Shape(_)) && stroke_extent(&s) < 3.0) {
                        if matches!(st.canvas.tool, Tool::Shape(_)) || st.canvas.snapped {
                            s.points = densify(&s.points, 6.0);
                        }
                        st.snap(page);
                        page.strokes.push(s);
                    }
                    st.canvas.anchor = None;
                }
            }
            Tool::Lasso => {
                if st.canvas.moving {
                    st.canvas.moving = false;
                } else if st.canvas.sel_press.is_some() {
                    if let Some(p) = st.canvas.sel_press.take() {
                        sel_button_tap(page, st, p);
                    }
                } else if st.canvas.lasso.len() > 2 {
                    let poly = std::mem::take(&mut st.canvas.lasso);
                    st.canvas.selected = page
                        .strokes
                        .iter()
                        .filter(|s| {
                            let inside = s.points.iter().filter(|q| point_in_poly(Pos2::new(q[0], q[1]), &poly)).count();
                            inside * 2 > s.points.len()
                        })
                        .map(|s| s.id)
                        .collect();
                    if st.canvas.selected.is_empty() {
                        st.say("Nothing selected — draw a loop around the ink.");
                    } else if st.canvas.selected.len() == 1 {
                        st.say("1 stroke selected — drag to move, or convert it.");
                        st.canvas.status_is_sel = true;
                    } else {
                        st.say(tf!("{n} strokes selected — drag to move, or convert them.", n = st.canvas.selected.len()));
                        st.canvas.status_is_sel = true;
                    }
                } else {
                    st.clear_selection();
                }
                st.canvas.lasso.clear();
            }
            _ => {}
        }
    }
    if let Some(h) = resp.hover_pos().filter(|_| resp.hovered() || resp.dragged()) {
        let painter = ui.painter_at(rect);
        match st.canvas.tool {
            Tool::Pen => {
                let pen = *st.pen();
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                let [r, g, b, a] = pen.color;
                let dark = ui.visuals().dark_mode;
                let col = if dark && 0.2126 * (r as f32) + 0.7152 * (g as f32) + 0.0722 * (b as f32) < 60.0 {
                    c4([226, 230, 228, a])
                } else {
                    c4(pen.color)
                };
                painter.circle_filled(h, (pen.width * 0.5).max(1.5), col);
            }
            Tool::Highlighter => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                painter.rect_filled(Rect::from_center_size(h, Vec2::new(5.0, st.p.hl_width)), 1.0, c4(st.p.hl_color));
            }
            Tool::Eraser => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                painter.circle_stroke(h, st.p.eraser_size, EStroke::new(1.2, ui.visuals().text_color()));
            }
            Tool::Lasso if selection_bounds(page, &st.canvas.selected).is_some_and(|r| r.expand(8.0).contains(h - o.to_vec2())) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
            }
            _ => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
        }
    }
    None
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    #[test]
    fn selection_buttons() {
        let ctx = egui::Context::default();
        let mut app = crate::NotesApp::new(&eframe::CreationContext::_new_kittest(ctx), Some(std::env::temp_dir().join("ln-sel.png")));
        let st = &mut app.st;
        let mut page = Page::new("t");
        page.strokes.push(Stroke {
            id: 7,
            points: vec![[100.0, 100.0, 1.0], [200.0, 140.0, 1.0]],
            color: [0, 0, 0, 255],
            width: 2.0,
            highlighter: false,
            pen: PenKind::Ballpoint,
            t: 0.0,
        });
        st.canvas.selected = vec![7];
        let [del, dup] = sel_buttons(selection_bounds(&page, &st.canvas.selected).unwrap().expand(6.0));
        assert!(!sel_button_tap(&mut page, st, Pos2::new(500.0, 500.0)), "elsewhere is not a button");
        assert!(sel_button_tap(&mut page, st, dup.center()));
        assert_eq!(page.strokes.len(), 2);
        assert!(sel_button_tap(&mut page, st, del.center() + Vec2::new(24.0, 24.0)), "the copy is selected, its buttons moved with it");
        assert_eq!(page.strokes.iter().map(|s| s.id).collect::<Vec<_>>(), vec![7]);
    }
}
