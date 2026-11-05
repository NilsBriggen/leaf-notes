//! Draws the app without a window, for `--screenshot` when the window gets no frames
//! (a locked or blanked screen, a hidden session, a server). It runs the same app code for the
//! same number of frames and paints egui's triangles on the CPU, blending like the GPU renderer
//! does (premultiplied colours in gamma space).
use eframe::egui::{self, epaint, Color32, TextureId};
use std::collections::HashMap;
use std::path::Path;

/// A texture as plain pixels: width, height, premultiplied colours.
type Tex = (usize, usize, Vec<Color32>);

/// Run the app for `frames` frames in a `size` (points) window at `ppp` pixels per point and save the last one as a PNG.
pub fn screenshot(path: &Path, frames: usize, size: egui::Vec2, ppp: f32) -> Result<(), String> {
    use eframe::App;
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = crate::NotesApp::new(&cc, Some(path.to_path_buf()));
    let mut frame = eframe::Frame::_new_kittest();
    let mut textures: HashMap<TextureId, Tex> = HashMap::new();
    let mut last = (vec![], ppp);
    for i in 0..frames.max(1) {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(i as f64 / 60.0),
            predicted_dt: 1.0 / 60.0,
            focused: true,
            ..Default::default()
        };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        let mut out = ctx.run_ui(raw, |ui| app.ui(ui, &mut frame));
        for (id, deltas) in &out.textures_delta.set { for d in deltas { apply(&mut textures, *id, d); } }
        last = (ctx.tessellate(out.shapes, out.pixels_per_point), out.pixels_per_point);
        for id in &out.textures_delta.free { textures.remove(id); }
        out.textures_delta.clear(); // handled above (egui checks that every texture change is used)
    }
    let (prims, ppp) = last;
    let (w, h) = ((size.x * ppp).round() as usize, (size.y * ppp).round() as usize);
    let bg = app.clear_color(&ctx.global_style().visuals);
    let mut px = vec![bg; w * h];
    for p in &prims { paint(&mut px, w, h, ppp, p, &textures); }
    let bytes: Vec<u8> = px.iter().flat_map(|c| [c[0], c[1], c[2]].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).into_iter().chain([255])).collect();
    if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
    image::save_buffer(path, &bytes, w as u32, h as u32, image::ColorType::Rgba8).map_err(|e| e.to_string())
}

fn apply(textures: &mut HashMap<TextureId, Tex>, id: TextureId, delta: &epaint::ImageDelta) {
    let epaint::ImageData::Color(img) = &delta.image;
    let [dw, dh] = img.size;
    match delta.pos {
        None => { textures.insert(id, (dw, dh, img.pixels.clone())); }
        Some([x0, y0]) => {
            let Some((tw, th, pixels)) = textures.get_mut(&id) else { return };
            for y in 0..dh.min(th.saturating_sub(y0)) {
                for x in 0..dw.min(tw.saturating_sub(x0)) { pixels[(y0 + y) * *tw + x0 + x] = img.pixels[y * dw + x]; }
            }
        }
    }
}

/// Bilinear sample (clamped), premultiplied RGBA in 0..1.
fn sample((tw, th, pixels): &Tex, u: f32, v: f32) -> [f32; 4] {
    let (x, y) = (u * *tw as f32 - 0.5, v * *th as f32 - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |dx: f32, dy: f32| {
        let c = pixels[((y0 + dy).clamp(0.0, *th as f32 - 1.0) as usize) * tw + (x0 + dx).clamp(0.0, *tw as f32 - 1.0) as usize];
        [c.r(), c.g(), c.b(), c.a()].map(|v| v as f32 / 255.0)
    };
    let (a, b, c, d) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0));
    std::array::from_fn(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
}

fn paint(px: &mut [[f32; 4]], w: usize, h: usize, ppp: f32, p: &epaint::ClippedPrimitive, textures: &HashMap<TextureId, Tex>) {
    let epaint::Primitive::Mesh(mesh) = &p.primitive else { return }; // custom GPU callbacks can't be drawn here
    let tex = textures.get(&mesh.texture_id);
    let cx0 = (p.clip_rect.min.x * ppp).floor().max(0.0) as usize;
    let cy0 = (p.clip_rect.min.y * ppp).floor().max(0.0) as usize;
    let cx1 = ((p.clip_rect.max.x * ppp).ceil().max(0.0) as usize).min(w);
    let cy1 = ((p.clip_rect.max.y * ppp).ceil().max(0.0) as usize).min(h);
    let edge = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    for t in mesh.indices.chunks_exact(3) {
        let v = [t[0], t[1], t[2]].map(|i| &mesh.vertices[i as usize]);
        let q = v.map(|v| [v.pos.x * ppp, v.pos.y * ppp]);
        let area = edge(q[0], q[1], q[2]);
        if area.abs() < 1e-6 { continue; }
        let col = v.map(|v| [v.color.r(), v.color.g(), v.color.b(), v.color.a()].map(|c| c as f32 / 255.0));
        let x0 = q.iter().map(|p| p[0]).fold(f32::MAX, f32::min).floor().max(cx0 as f32) as usize;
        let x1 = (q.iter().map(|p| p[0]).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(cx1);
        let y0 = q.iter().map(|p| p[1]).fold(f32::MAX, f32::min).floor().max(cy0 as f32) as usize;
        let y1 = (q.iter().map(|p| p[1]).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(cy1);
        for y in y0..y1 {
            for x in x0..x1 {
                let c = [x as f32 + 0.5, y as f32 + 0.5];
                let (b0, b1) = (edge(q[1], q[2], c) / area, edge(q[2], q[0], c) / area);
                let b2 = 1.0 - b0 - b1;
                if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 { continue; }
                let mut src: [f32; 4] = std::array::from_fn(|i| col[0][i] * b0 + col[1][i] * b1 + col[2][i] * b2);
                if let Some(tex) = tex {
                    let (u, vv) = (v[0].uv.x * b0 + v[1].uv.x * b1 + v[2].uv.x * b2, v[0].uv.y * b0 + v[1].uv.y * b1 + v[2].uv.y * b2);
                    let s = sample(tex, u, vv);
                    for i in 0..4 { src[i] *= s[i]; }
                }
                let d = &mut px[y * w + x];
                for i in 0..4 { d[i] = src[i] + d[i] * (1.0 - src[3]); }
            }
        }
    }
}

/// Click-through tests: the real app, driven by its accessibility tree (buttons are found by
/// their names, as a screen reader would), without a window.
#[cfg(test)]
mod clickthrough {
    use eframe::egui::{self, accesskit};
    use std::collections::HashMap;

    struct Harness { ctx: egui::Context, app: crate::NotesApp, frame: eframe::Frame, t: f64, nodes: HashMap<accesskit::NodeId, (String, egui::Rect)> }

    impl Harness {
        fn new() -> Harness {
            crate::i18n::set("en");
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let app = crate::NotesApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()), Some(std::env::temp_dir().join("ln-click.png")));
            let mut h = Harness { ctx, app, frame: eframe::Frame::_new_kittest(), t: 0.0, nodes: HashMap::new() };
            h.run(vec![]);
            h.run(vec![]);
            h
        }
        fn run(&mut self, events: Vec<egui::Event>) {
            use eframe::App;
            self.t += 1.0 / 60.0;
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))), time: Some(self.t), events, focused: true, ..Default::default() };
            let mut out = self.ctx.run_ui(raw, |ui| self.app.ui(ui, &mut self.frame));
            out.textures_delta.clear();
            if let Some(u) = out.platform_output.accesskit_update {
                for (id, node) in u.nodes {
                    if let (Some(label), Some(b)) = (node.label(), node.bounds()) {
                        self.nodes.insert(id, (label.to_string(), egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))));
                    } else { self.nodes.remove(&id); }
                }
            }
        }
        /// Click the control with this name (the topmost-left one if there are several).
        fn click(&mut self, label: &str) {
            let rect = self.nodes.values().filter(|(l, _)| l == label).map(|(_, r)| *r).min_by(|a, b| (a.top(), a.left()).partial_cmp(&(b.top(), b.left())).unwrap())
                .unwrap_or_else(|| panic!("no control named {label:?}; there are: {:?}", self.nodes.values().map(|v| &v.0).collect::<Vec<_>>()));
            let p = rect.center();
            let button = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            self.run(vec![egui::Event::PointerMoved(p)]);
            self.run(vec![button(true)]);
            self.run(vec![button(false)]);
            self.run(vec![]);
        }
    }

    #[test]
    fn main_tasks_by_clicking() {
        let _turn = crate::i18n::tests::LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut h = Harness::new();
        // a new page from the pages column
        let before = h.app.store.notebooks[h.app.nb].sections[h.app.sec].pages.len();
        h.click("+ Page");
        assert_eq!(h.app.store.notebooks[h.app.nb].sections[h.app.sec].pages.len(), before + 1);
        // Insert ▸ Checklist puts a checklist on the page
        h.click("Insert");
        h.click("Checklist");
        let page = &h.app.store.notebooks[h.app.nb].sections[h.app.sec].pages[h.app.pg];
        assert!(page.blocks.iter().any(|b| matches!(b.kind, crate::model::Kind::Checklist { .. })));
        // another section from the side panel
        h.click("Ideas");
        assert_eq!(h.app.store.notebooks[h.app.nb].sections[h.app.sec].name, "Ideas");
        // the help window
        h.click("?");
        assert!(h.app.st.show_help);
    }

    #[test]
    fn two_finger_tap_undoes() {
        let _turn = crate::i18n::tests::LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut h = Harness::new();
        let blocks = |h: &Harness| h.app.store.notebooks[h.app.nb].sections[h.app.sec].pages[h.app.pg].blocks.len();
        let before = blocks(&h);
        h.click("Insert");
        h.click("Checklist");
        assert_eq!(blocks(&h), before + 1);
        let touch = |id: u64, x: f32, phase| egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(id), phase, pos: egui::pos2(x, 600.0), force: None };
        h.run(vec![touch(1, 900.0, egui::TouchPhase::Start), touch(2, 1000.0, egui::TouchPhase::Start)]);
        h.run(vec![touch(1, 900.0, egui::TouchPhase::End), touch(2, 1000.0, egui::TouchPhase::End)]);
        assert_eq!(blocks(&h), before, "a quick two-finger tap undoes");
        // a two-finger drag (scrolling) does not (the Insert tab is still open)
        h.click("Checklist");
        assert_eq!(blocks(&h), before + 1);
        h.run(vec![touch(1, 900.0, egui::TouchPhase::Start), touch(2, 1000.0, egui::TouchPhase::Start)]);
        h.run(vec![touch(1, 900.0, egui::TouchPhase::Move), egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(2), phase: egui::TouchPhase::Move, pos: egui::pos2(1000.0, 500.0), force: None }]);
        h.run(vec![touch(1, 900.0, egui::TouchPhase::End), touch(2, 1000.0, egui::TouchPhase::End)]);
        assert_eq!(blocks(&h), before + 1);
    }
}
