//! PDF export.
//! * Pages with a PDF printout: the original document, page for page, with the ink and typed notes
//!   that sit on each printed page drawn on top (vector ink, real PDF text).
//! * Other pages: a picture of just the written-on area, laid out on A4 pages with the title.

use crate::model::*;
use eframe::egui::{Pos2, Rect, Vec2};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use std::path::Path;

/// The PDF printed onto this page (if any): its stored file and where each printed page sits.
pub fn printout_source(page: &Page, heights: &dyn Fn(u64) -> Option<f32>) -> Option<(String, Vec<(u32, Rect)>)> {
    let mut found: Option<(String, Vec<(u32, Rect)>)> = None;
    for b in &page.blocks {
        let Kind::Image { file } = &b.kind else { continue };
        let Some((stem, n)) = file.strip_suffix(".png").and_then(|f| f.rsplit_once("-page")) else { continue };
        let Ok(n) = n.parse::<u32>() else { continue };
        let pdf = format!("{stem}.pdf");
        if !safe_name(&pdf) { continue; }
        if found.as_ref().is_some_and(|f| f.0 != pdf) { continue; } // only the first printed PDF
        let h = heights(b.id).unwrap_or(b.width * 1.294);
        found.get_or_insert_with(|| (pdf, vec![])).1.push((n, Rect::from_min_size(Pos2::new(b.pos[0], b.pos[1]), Vec2::new(b.width, h))));
    }
    found
}

fn num(o: &Object) -> f32 { o.as_float().ok().or_else(|| o.as_i64().ok().map(|v| v as f32)).unwrap_or(0.0) }

/// A page attribute, looked up through the page tree if the page inherits it.
fn inherited<'a>(doc: &'a Document, mut id: ObjectId, key: &[u8]) -> Option<&'a Object> {
    for _ in 0..32 {
        let d = doc.get_dictionary(id).ok()?;
        if let Ok(v) = d.get(key) { return Some(v); }
        id = d.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

/// Characters → WinAnsi bytes for the built-in Helvetica (anything else becomes '?').
fn winansi(s: &str) -> Vec<u8> {
    s.chars().map(|c| match c {
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '€' => 0x80, '‚' => 0x82, '„' => 0x84, '…' => 0x85, '‘' => 0x91, '’' => 0x92, '“' => 0x93, '”' => 0x94, '•' => 0x95, '–' => 0x96, '—' => 0x97, '™' => 0x99,
        _ => b'?',
    }).flat_map(|b| match b { b'(' | b')' | b'\\' => vec![b'\\', b], b => vec![b] }).collect()
}

/// Plain text of a note for printing over a printout (Markdown markers removed).
fn note_text(k: &Kind) -> String {
    match k {
        Kind::Text { text, .. } => text.lines().map(|l| {
            let (_, plen) = crate::rich::classify(l);
            let lead = if plen > 0 && l.trim_start().starts_with(['-', '*']) { "• " } else { "" };
            let body: String = crate::rich::pieces(&l[plen..]).into_iter().map(|p| match p {
                crate::rich::Piece::Text(t, _) | crate::rich::Piece::Math(t) | crate::rich::Piece::Link(t) => t,
                crate::rich::Piece::Marker(_) => String::new(),
            }).collect();
            format!("{lead}{body}")
        }).collect::<Vec<_>>().join("\n"),
        Kind::Checklist { items } => items.iter().map(|(d, t)| format!("{} {t}", if *d { "[x]" } else { "[ ]" })).collect::<Vec<_>>().join("\n"),
        Kind::Image { .. } => String::new(),
        other => other.summary(),
    }
}

/// Export the printed PDF with everything written on top of each printed page.
/// Returns how many pages carry notes.
pub fn export_printout(page: &Page, files: &Path, heights: &dyn Fn(u64) -> Option<f32>, note_size: f32, out: &Path) -> Result<usize, String> {
    let (pdf, placed) = printout_source(page, heights).ok_or("This page has no PDF printout.")?;
    let mut doc = Document::load(files.join(&pdf)).map_err(|e| format!("Couldn't open the original PDF ({e})"))?;
    if doc.is_encrypted() { return Err("The original PDF is password-protected.".into()); }
    let pages = doc.get_pages();
    let mut annotated = 0;
    for (n, area) in placed {
        let Some(&pid) = pages.get(&n) else { continue };
        let bx = inherited(&doc, pid, b"CropBox").or_else(|| inherited(&doc, pid, b"MediaBox")).and_then(|o| o.as_array().ok()).map(|a| a.iter().map(num).collect::<Vec<_>>());
        let Some(bx) = bx.filter(|b| b.len() == 4) else { continue };
        let (x0, y0, x1, y1) = (bx[0].min(bx[2]), bx[1].min(bx[3]), bx[0].max(bx[2]), bx[1].max(bx[3]));
        let rot = inherited(&doc, pid, b"Rotate").map(|o| (num(o) as i32).rem_euclid(360)).unwrap_or(0);
        let (w, h) = (x1 - x0, y1 - y0);
        // canvas point → PDF user space, following the page's rotation
        let map = |x: f32, y: f32| {
            let (u, v) = ((x - area.left()) / area.width(), (y - area.top()) / area.height());
            match rot { 90 => (x0 + v * w, y0 + u * h), 180 => (x1 - u * w, y0 + v * h), 270 => (x1 - v * w, y1 - u * h), _ => (x0 + u * w, y1 - v * h) }
        };
        let scale = if rot == 90 || rot == 270 { h } else { w } / area.width();
        let mut ops = String::from("Q q 1 J 1 j\n");
        let mut alphas: Vec<u8> = vec![];
        let mut used = false;
        for s in &page.strokes {
            if s.points.is_empty() || !s.points.iter().any(|p| area.contains(Pos2::new(p[0], p[1]))) { continue; }
            used = true;
            let a = s.color[3];
            if a < 250 { if !alphas.contains(&a) { alphas.push(a); } ops += &format!("/LNa{a} gs\n"); } else { ops += "/LNa255 gs\n"; if !alphas.contains(&255) { alphas.push(255); } }
            ops += &format!("{:.3} {:.3} {:.3} RG\n", s.color[0] as f32 / 255.0, s.color[1] as f32 / 255.0, s.color[2] as f32 / 255.0);
            let pts: Vec<[f32; 3]> = if s.points.len() == 1 { vec![s.points[0], s.points[0]] } else { s.points.clone() };
            for seg in pts.windows(2) {
                let wdt = crate::canvas::seg_width(s, (seg[0][2] + seg[1][2]) * 0.5, seg[1][0] - seg[0][0], seg[1][1] - seg[0][1]) * scale;
                let ((ax, ay), (bx, by)) = (map(seg[0][0], seg[0][1]), map(seg[1][0], seg[1][1]));
                ops += &format!("{wdt:.2} w {ax:.2} {ay:.2} m {bx:.2} {by:.2} l S\n");
            }
        }
        // typed notes that sit on this printed page
        let mut text_ops: Vec<u8> = vec![];
        let tj = |out: &mut Vec<u8>, t: &str| { out.push(b'('); out.extend(winansi(t)); out.extend_from_slice(b") Tj T*\n"); };
        for b in &page.blocks {
            if matches!(b.kind, Kind::Image { .. }) { continue; }
            let r = Rect::from_min_size(Pos2::new(b.pos[0], b.pos[1]), Vec2::new(b.width, heights(b.id).unwrap_or(30.0)));
            if !area.intersects(r) { continue; }
            let text = note_text(&b.kind);
            if text.trim().is_empty() { continue; }
            used = true;
            let size = note_size * scale;
            let per_line = ((b.width / (note_size * 0.5)) as usize).max(8);
            let (mut tx, mut ty) = map(b.pos[0], b.pos[1] + note_size * 1.1);
            if rot != 0 { tx = tx.max(x0); ty = ty.max(y0); } // rotated pages: keep text upright and on the page
            text_ops.extend_from_slice(format!("BT /LNf {size:.2} Tf 0.12 0.16 0.23 rg {:.2} TL {tx:.2} {ty:.2} Td\n", size * 1.35).as_bytes());
            for line in text.lines() {
                let mut cur = String::new();
                for word in line.split(' ') {
                    if cur.chars().count() + word.chars().count() + 1 > per_line && !cur.is_empty() {
                        tj(&mut text_ops, &cur);
                        cur.clear();
                    }
                    if !cur.is_empty() { cur.push(' '); }
                    cur.push_str(word);
                }
                tj(&mut text_ops, &cur);
            }
            text_ops.extend_from_slice(b"ET\n");
        }
        if !used { continue; }
        annotated += 1;
        let mut ops = ops.into_bytes();
        ops.extend(text_ops);
        ops.extend_from_slice(b"Q\n");
        // resources: our font and transparency levels, added next to the page's own
        let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
        let res = doc.get_or_create_resources(pid).and_then(Object::as_dict_mut).map_err(|e| e.to_string())?;
        add_entry(res, b"Font", "LNf", Object::Reference(font));
        for a in &alphas { let f = *a as f32 / 255.0; add_entry(res, b"ExtGState", &format!("LNa{a}"), Object::Dictionary(dictionary! { "CA" => f, "ca" => f })); }
        // wrap the original drawing in q … Q so our ink starts from a clean state
        let open = doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
        let tail = doc.add_object(Stream::new(Dictionary::new(), ops));
        let mut list: Vec<Object> = vec![Object::Reference(open)];
        list.extend(doc.get_page_contents(pid).into_iter().map(Object::Reference));
        list.push(Object::Reference(tail));
        doc.get_object_mut(pid).and_then(Object::as_dict_mut).map_err(|e| e.to_string())?.set("Contents", list);
    }
    let mut bytes = vec![];
    doc.save_to(&mut bytes).map_err(|e| format!("Couldn't write the PDF ({e})"))?;
    crate::storage::write_atomic(out, &bytes).map_err(|e| format!("Couldn't write the PDF ({e})"))?;
    Ok(annotated)
}

fn add_entry(res: &mut Dictionary, kind: &[u8], name: &str, value: Object) {
    if !matches!(res.get(kind), Ok(Object::Dictionary(_))) {
        // an indirect sub-dictionary would be shared; give the page its own copy-free entry instead
        res.set(kind, Dictionary::new());
    }
    if let Ok(Object::Dictionary(d)) = res.get_mut(kind) { d.set(name, value); }
}

/// Write a tall picture of a page onto A4 pages (title on the first), breaking at blank rows.
pub fn write_image_pdf(title: &str, img: &image::RgbImage, out: &Path) -> Result<usize, String> {
    let (pw, ph, margin) = (595.0f32, 842.0f32, 40.0f32);
    let scale = (pw - 2.0 * margin) / img.width() as f32; // points per pixel
    let title_h = 46.0;
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold", "Encoding" => "WinAnsiEncoding" });
    let small = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
    let blank = |y: u32| { let p0 = img.get_pixel(0, y); (0..img.width()).step_by(2).all(|x| { let p = img.get_pixel(x, y); (0..3).all(|k| (p[k] as i32 - p0[k] as i32).abs() < 10) }) };
    let mut kids = vec![];
    let mut y = 0u32;
    let mut n = 0;
    while y < img.height() {
        let avail = (ph - 2.0 * margin - if n == 0 { title_h } else { 0.0 }) / scale;
        let mut end = (y + avail as u32).min(img.height());
        if end < img.height() {
            // prefer to cut where the row is empty, so lines of text aren't sliced
            let lo = end.saturating_sub((avail * 0.18) as u32).max(y + 1);
            if let Some(b) = (lo..end).rev().find(|&r| blank(r)) { end = b; }
        }
        let slice = image::imageops::crop_imm(img, 0, y, img.width(), end - y).to_image();
        let mut jpg = vec![];
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90).encode_image(&slice).map_err(|e| e.to_string())?;
        let im = doc.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => slice.width() as i64, "Height" => slice.height() as i64,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode" }, jpg));
        let (w, h) = (slice.width() as f32 * scale, slice.height() as f32 * scale);
        let top = ph - margin - if n == 0 { title_h } else { 0.0 };
        let ops = format!("q {w:.2} 0 0 {h:.2} {margin:.2} {:.2} cm /Im0 Do Q\n", top - h);
        let mut ops = ops.into_bytes();
        if n == 0 {
            ops.extend_from_slice(format!("BT /F1 20 Tf 0.1 0.13 0.16 rg {margin} {:.2} Td (", ph - margin - 22.0).as_bytes());
            ops.extend(winansi(title));
            ops.extend_from_slice(b") Tj ET\n");
        }
        ops.extend_from_slice(format!("BT /F2 9 Tf 0.5 0.55 0.6 rg {:.2} 20 Td ({}) Tj ET\n", pw / 2.0 - 4.0, n + 1).as_bytes());
        let content = doc.add_object(Stream::new(Dictionary::new(), ops));
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content, "MediaBox" => vec![0.into(), 0.into(), pw.into(), ph.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => im }, "Font" => dictionary! { "F1" => font, "F2" => small } } });
        kids.push(Object::Reference(page));
        n += 1;
        y = end.max(y + 1);
    }
    let count = kids.len() as i64;
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut bytes = vec![];
    doc.save_to(&mut bytes).map_err(|e| format!("Couldn't write the PDF ({e})"))?;
    crate::storage::write_atomic(out, &bytes).map_err(|e| format!("Couldn't write the PDF ({e})"))?;
    Ok(n)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A one-page 400×400 pt PDF with a blue square in the middle, written by hand.
    pub fn tiny_pdf() -> Vec<u8> {
        let content = "0 0 1 rg 150 150 100 100 re f";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offs = vec![];
        for (i, o) in objs.iter().enumerate() { offs.push(pdf.len()); pdf += &format!("{} 0 obj\n{o}\nendobj\n", i + 1); }
        let xref = pdf.len();
        pdf += &format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1);
        for o in offs { pdf += &format!("{o:010} 00000 n \n"); }
        pdf += &format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1);
        pdf.into_bytes()
    }

    fn render_first_page(path: &Path) -> image::RgbaImage {
        let doc = hayro::hayro_syntax::Pdf::new(std::fs::read(path).unwrap()).unwrap();
        let pix = hayro::PixmapSettings { x_scale: 1.0, y_scale: 1.0, bg_color: hayro::vello_cpu::color::palette::css::WHITE };
        let png = hayro::render(&doc.pages()[0], &hayro::RenderCache::new(), &Default::default(), &Default::default(), &pix).into_png().unwrap();
        image::load_from_memory(&png).unwrap().to_rgba8()
    }

    #[test]
    fn printout_keeps_pages_and_adds_ink() {
        let dir = std::env::temp_dir().join(format!("leafnotes-export-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("77.pdf"), tiny_pdf()).unwrap();
        let mut page = Page::new("p");
        // the printed page sits at (0,0), 640 wide (so 1.6 canvas units per PDF point)
        page.blocks.push(Block { kind: Kind::Image { file: "77-page1.png".into() }, ..block(0.0, 0.0, 640.0, Kind::Text { text: String::new(), heading: 0 }) });
        page.blocks.push(block(32.0, 32.0, 300.0, Kind::Text { text: "**Note** on top".into(), heading: 0 }));
        // a thick red line across the top-left quarter: PDF y = 400 - 80/1.6 = 350
        page.strokes.push(Stroke { id: 1, points: vec![[16.0, 80.0, 1.0], [300.0, 80.0, 1.0]], color: [220, 30, 30, 255], width: 8.0, highlighter: false, pen: PenKind::Marker, t: 0.0 });
        let h = |_| Some(640.0);
        let out = dir.join("out.pdf");
        assert_eq!(export_printout(&page, &dir, &h, 15.0, &out).unwrap(), 1);
        let img = render_first_page(&out);
        assert_eq!((img.width(), img.height()), (400, 400), "same page size as the original");
        assert_eq!(img.get_pixel(200, 200).0[..3], [0, 0, 255], "original drawing kept");
        let ink = img.get_pixel(100, 50).0;
        assert!(ink[0] > 180 && ink[1] < 90, "ink drawn on top: {ink:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn page_picture_splits_onto_a4() {
        let dir = std::env::temp_dir().join(format!("leafnotes-export-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 600 px wide, 2500 px tall: white with dark bands every 40 px
        let img = image::RgbImage::from_fn(600, 2500, |_, y| if y % 40 < 12 { image::Rgb([30, 30, 30]) } else { image::Rgb([255, 255, 255]) });
        let out = dir.join("page.pdf");
        let n = write_image_pdf("Fourier — notes", &img, &out).unwrap();
        assert!(n >= 3, "{n} pages");
        let doc = hayro::hayro_syntax::Pdf::new(std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(doc.pages().len(), n);
        let _ = std::fs::remove_dir_all(dir);
    }
}
