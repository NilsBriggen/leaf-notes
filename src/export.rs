//! PDF & Markdown export.
//! * Pages with a PDF printout: the original document, page for page, with the ink and typed notes
//!   that sit on each printed page drawn on top (vector ink, Multiply highlighters, real rotated PDF text,
//!   without mutating shared indirect resource dictionaries).
//! * Other pages: streamed tile-by-tile onto A4 pages with clean line-aware breaks and the title.

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
        if found.as_ref().is_some_and(|f| f.0 != pdf) { continue; }
        let h = heights(b.id).unwrap_or(b.width * 1.294);
        found
            .get_or_insert_with(|| (pdf, vec![]))
            .1
            .push((n, Rect::from_min_size(Pos2::new(b.pos[0], b.pos[1]), Vec2::new(b.width, h))));
    }
    found
}

fn num(o: &Object) -> f32 {
    o.as_float().ok().or_else(|| o.as_i64().ok().map(|v| v as f32)).unwrap_or(0.0)
}

/// A page attribute, looked up through the page tree if the page inherits it.
fn inherited<'a>(doc: &'a Document, mut id: ObjectId, key: &[u8]) -> Option<&'a Object> {
    for _ in 0..32 {
        let d = doc.get_dictionary(id).ok()?;
        if let Ok(v) = d.get(key) {
            return Some(v);
        }
        id = d.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

/// Characters → WinAnsi bytes for the built-in Helvetica (anything else becomes '?').
fn winansi(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| match c {
            ' '..='~' => c as u8,
            '\u{a0}'..='\u{ff}' => c as u32 as u8,
            '€' => 0x80,
            '‚' => 0x82,
            '„' => 0x84,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            '™' => 0x99,
            _ => b'?',
        })
        .flat_map(|b| match b {
            b'(' | b')' | b'\\' => vec![b'\\', b],
            b => vec![b],
        })
        .collect()
}

/// A page as Markdown, with its pictures, attached files, and a picture of its handwriting in
/// `"<name>_files"` next to it. Built per-block so literal `"[image]"` text inside a note is never corrupted.
#[cfg_attr(target_os = "android", allow(dead_code))]
pub fn markdown_with_files(page: &Page, files: &Path, out: &Path) -> Result<(), String> {
    let dir_name = format!(
        "{}_files",
        out.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    );
    let dir = out.with_file_name(&dir_name);
    let mut parts: Vec<String> = vec![];
    if !page.title.trim().is_empty() {
        parts.push(format!("# {}", page.title.trim()));
    }
    if page.kind == PageKind::Deck {
        for c in page.deck() {
            parts.push(format!("- **{}** :: {}", c.front.trim(), c.back.trim()));
        }
    } else {
        let mut blocks: Vec<&Block> = page.blocks.iter().collect();
        blocks.sort_by(|a, b| {
            (a.pos[1] / 40.0)
                .floor()
                .total_cmp(&(b.pos[1] / 40.0).floor())
                .then(a.pos[0].total_cmp(&b.pos[0]))
        });
        for b in blocks {
            match &b.kind {
                Kind::Text { text, heading } => {
                    let prefix = match heading {
                        1 => "# ",
                        2 => "## ",
                        3 => "### ",
                        _ => "",
                    };
                    if !text.trim().is_empty() {
                        parts.push(format!("{prefix}{text}"));
                    }
                }
                Kind::Math { src } => {
                    if !src.trim().is_empty() {
                        parts.push(format!("$$\n{}\n$$", src.trim()));
                    }
                }
                Kind::Checklist { items } => {
                    let s: Vec<String> = items
                        .iter()
                        .map(|(d, t)| format!("- [{}] {t}", if *d { "x" } else { " " }))
                        .collect();
                    if !s.is_empty() {
                        parts.push(s.join("\n"));
                    }
                }
                Kind::Table { rows } => {
                    if let Some(first) = rows.first() {
                        let mut lines = vec![format!("| {} |", first.join(" | "))];
                        lines.push(format!("|{}|", first.iter().map(|_| "---").collect::<Vec<_>>().join("|")));
                        for r in rows.iter().skip(1) {
                            lines.push(format!("| {} |", r.join(" | ")));
                        }
                        parts.push(lines.join("\n"));
                    }
                }
                Kind::Code { lang, code } => {
                    parts.push(format!("```{lang}\n{code}\n```"));
                }
                Kind::Link { url, title } => {
                    let label = if title.trim().is_empty() { url.as_str() } else { title.as_str() };
                    parts.push(format!("[{label}]({url})"));
                }
                Kind::Image { file } => {
                    let copied = safe_name(file) && files.join(file).exists() && {
                        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                        std::fs::copy(files.join(file), dir.join(file)).is_ok()
                    };
                    parts.push(if copied {
                        format!("![](<{dir_name}/{file}>)")
                    } else {
                        "[picture]".to_string()
                    });
                }
                Kind::File { file, name, .. } => {
                    let copied = safe_name(file) && files.join(file).exists() && {
                        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                        std::fs::copy(files.join(file), dir.join(file)).is_ok()
                    };
                    let label = if name.trim().is_empty() { file.as_str() } else { name.as_str() };
                    parts.push(if copied {
                        format!("[{label}](<{dir_name}/{file}>)")
                    } else {
                        format!("[file: {label}]")
                    });
                }
            }
        }
        let ink: Vec<&Stroke> = page.strokes.iter().filter(|s| !s.highlighter).collect();
        if let Some(png) = rasterize(&ink) {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(dir.join("handwriting.png"), png).map_err(|e| e.to_string())?;
            parts.push(format!("![handwriting](<{dir_name}/handwriting.png>)"));
        } else if !page.strokes.is_empty() {
            parts.push(format!("[{} handwritten strokes]", page.strokes.len()));
        }
    }
    let md = parts.join("\n\n");
    crate::storage::write_atomic(out, md.as_bytes()).map_err(|e| e.to_string())
}

/// How many characters of the page's typed notes the printout export can't print (they come out as '?').
pub fn unprintable(page: &Page) -> usize {
    page.blocks
        .iter()
        .map(|b| {
            note_text(&b.kind)
                .chars()
                .filter(|c| !c.is_control() && winansi(&c.to_string()) == b"?" && *c != '?')
                .count()
        })
        .sum()
}

/// Plain text of a note for printing over a printout (Markdown markers removed, full Code & Table preserved).
fn note_text(k: &Kind) -> String {
    match k {
        Kind::Text { text, .. } => text
            .lines()
            .map(|l| {
                let (_, plen) = crate::rich::classify(l);
                let lead = if plen > 0 && l.trim_start().starts_with(['-', '*']) { "• " } else { "" };
                let body: String = crate::rich::pieces(&l[plen..])
                    .into_iter()
                    .map(|p| match p {
                        crate::rich::Piece::Text(t, _) | crate::rich::Piece::Math(t) | crate::rich::Piece::Link(t) => t,
                        crate::rich::Piece::Marker(_) => String::new(),
                    })
                    .collect();
                format!("{lead}{body}")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Kind::Checklist { items } => items
            .iter()
            .map(|(d, t)| format!("{} {t}", if *d { "[x]" } else { "[ ]" }))
            .collect::<Vec<_>>()
            .join("\n"),
        Kind::Code { code, .. } => code.clone(),
        Kind::Table { rows } => rows.iter().map(|r| r.join("  |  ")).collect::<Vec<_>>().join("\n"),
        Kind::Image { .. } => String::new(),
        other => other.summary(),
    }
}

/// Clone a page's `/Resources` dictionary (resolving any indirect reference) along with its
/// `/Font` and `/ExtGState` sub-dictionaries so adding LeafNotes resources never mutates or wipes
/// shared indirect dictionaries belonging to other pages.
fn clone_page_resources(doc: &Document, pid: ObjectId) -> (Dictionary, Dictionary, Dictionary) {
    let base_res = match inherited(doc, pid, b"Resources") {
        Some(Object::Reference(rid)) => doc.get_dictionary(*rid).cloned().unwrap_or_default(),
        Some(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    let clone_sub = |key: &[u8]| match base_res.get(key) {
        Ok(Object::Reference(rid)) => doc.get_dictionary(*rid).cloned().unwrap_or_default(),
        Ok(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    let fonts = clone_sub(b"Font");
    let gstates = clone_sub(b"ExtGState");
    (base_res, fonts, gstates)
}

/// Export the printed PDF with everything written on top of each printed page.
/// Returns how many pages carry notes.
pub fn export_printout(
    page: &Page,
    files: &Path,
    heights: &dyn Fn(u64) -> Option<f32>,
    note_size: f32,
    out: &Path,
) -> Result<usize, String> {
    let (pdf, placed) = printout_source(page, heights).ok_or("This page has no PDF printout.")?;
    let mut doc = Document::load(files.join(&pdf)).map_err(|e| format!("Couldn't open the original PDF ({e})"))?;
    if doc.is_encrypted() {
        return Err("The original PDF is password-protected.".into());
    }
    let pages = doc.get_pages();
    let mut annotated = 0;
    let font = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding"
    });
    for (n, area) in placed {
        let Some(&pid) = pages.get(&n) else { continue };
        let bx = inherited(&doc, pid, b"CropBox")
            .or_else(|| inherited(&doc, pid, b"MediaBox"))
            .and_then(|o| match o {
                Object::Reference(r) => doc.get_object(*r).ok(),
                other => Some(other),
            })
            .and_then(|o| o.as_array().ok())
            .map(|a| a.iter().map(num).collect::<Vec<_>>());
        let Some(bx) = bx.filter(|b| b.len() == 4) else { continue };
        let (x0, y0, x1, y1) = (bx[0].min(bx[2]), bx[1].min(bx[3]), bx[0].max(bx[2]), bx[1].max(bx[3]));
        let rot = inherited(&doc, pid, b"Rotate").map(|o| (num(o) as i32).rem_euclid(360)).unwrap_or(0);
        let (w, h) = (x1 - x0, y1 - y0);
        // canvas point → PDF user space, following the page's rotation
        let map = |x: f32, y: f32| {
            let (u, v) = ((x - area.left()) / area.width(), (y - area.top()) / area.height());
            match rot {
                90 => (x0 + v * w, y0 + u * h),
                180 => (x1 - u * w, y0 + v * h),
                270 => (x1 - v * w, y1 - u * h),
                _ => (x0 + u * w, y1 - v * h),
            }
        };
        let scale = if rot == 90 || rot == 270 { h } else { w } / area.width();
        let mut ops = String::from("Q q 1 J 1 j\n");
        let mut alphas: Vec<u8> = vec![];
        let mut hl_alphas: Vec<u8> = vec![];
        let mut used = false;

        // Draw highlighters first with Multiply blend mode so underlying PDF text stays sharp.
        for s in page.strokes.iter().filter(|s| s.highlighter).chain(page.strokes.iter().filter(|s| !s.highlighter)) {
            if s.points.is_empty() || !s.points.iter().any(|p| area.contains(Pos2::new(p[0], p[1]))) {
                continue;
            }
            used = true;
            let a = s.color[3];
            if s.highlighter {
                if !hl_alphas.contains(&a) {
                    hl_alphas.push(a);
                }
                ops += &format!("/LNm{a} gs\n");
            } else if a < 250 {
                if !alphas.contains(&a) {
                    alphas.push(a);
                }
                ops += &format!("/LNa{a} gs\n");
            } else {
                if !alphas.contains(&255) {
                    alphas.push(255);
                }
                ops += "/LNa255 gs\n";
            }
            ops += &format!(
                "{:.3} {:.3} {:.3} RG\n",
                s.color[0] as f32 / 255.0,
                s.color[1] as f32 / 255.0,
                s.color[2] as f32 / 255.0
            );
            let pts: Vec<[f32; 3]> = if s.points.len() == 1 {
                vec![s.points[0], s.points[0]]
            } else {
                s.points.clone()
            };
            for seg in pts.windows(2) {
                let wdt = crate::canvas::seg_width(
                    s,
                    (seg[0][2] + seg[1][2]) * 0.5,
                    seg[1][0] - seg[0][0],
                    seg[1][1] - seg[0][1],
                ) * scale;
                let ((ax, ay), (bx, by)) = (map(seg[0][0], seg[0][1]), map(seg[1][0], seg[1][1]));
                ops += &format!("{wdt:.2} w {ax:.2} {ay:.2} m {bx:.2} {by:.2} l S\n");
            }
        }

        // Typed notes sitting on this printed page (reset opaque Normal graphics state first!).
        let mut text_ops: Vec<u8> = vec![];
        let tj = |out: &mut Vec<u8>, t: &str| {
            out.push(b'(');
            out.extend(winansi(t));
            out.extend_from_slice(b") Tj T*\n");
        };
        let (tm_a, tm_b, tm_c, tm_d) = match rot {
            90 => (0, 1, -1, 0),
            180 => (-1, 0, 0, -1),
            270 => (0, -1, 1, 0),
            _ => (1, 0, 0, 1),
        };
        for b in &page.blocks {
            if matches!(b.kind, Kind::Image { .. }) {
                continue;
            }
            let r = Rect::from_min_size(Pos2::new(b.pos[0], b.pos[1]), Vec2::new(b.width, heights(b.id).unwrap_or(30.0)));
            if !area.intersects(r) {
                continue;
            }
            let text = note_text(&b.kind);
            if text.trim().is_empty() {
                continue;
            }
            if !used {
                used = true;
            }
            if text_ops.is_empty() {
                if !alphas.contains(&255) {
                    alphas.push(255);
                }
                text_ops.extend_from_slice(b"/LNa255 gs\n");
            }
            let size = note_size * scale;
            let per_line = ((b.width / (note_size * 0.5)) as usize).max(8);
            let (tx, ty) = map(b.pos[0], b.pos[1] + note_size * 1.1);
            text_ops.extend_from_slice(
                format!(
                    "BT /LNf {size:.2} Tf 0.12 0.16 0.23 rg {:.2} TL {tm_a} {tm_b} {tm_c} {tm_d} {tx:.2} {ty:.2} Tm\n",
                    size * 1.35
                )
                .as_bytes(),
            );
            for line in text.lines() {
                let mut cur = String::new();
                for word in line.split(' ') {
                    if cur.chars().count() + word.chars().count() + 1 > per_line && !cur.is_empty() {
                        tj(&mut text_ops, &cur);
                        cur.clear();
                    }
                    if !cur.is_empty() {
                        cur.push(' ');
                    }
                    cur.push_str(word);
                }
                tj(&mut text_ops, &cur);
            }
            text_ops.extend_from_slice(b"ET\n");
        }
        if !used {
            continue;
        }
        annotated += 1;
        let mut ops = ops.into_bytes();
        ops.extend(text_ops);
        ops.extend_from_slice(b"Q\n");

        // Clone page resources (including indirect Font / ExtGState dicts) before inserting LeafNotes entries.
        let (mut res, mut fonts, mut gstates) = clone_page_resources(&doc, pid);
        fonts.set("LNf", Object::Reference(font));
        for a in &alphas {
            let f = *a as f32 / 255.0;
            gstates.set(
                format!("LNa{a}"),
                Object::Dictionary(dictionary! { "Type" => "ExtGState", "CA" => f, "ca" => f, "BM" => "Normal" }),
            );
        }
        for a in &hl_alphas {
            let f = *a as f32 / 255.0;
            gstates.set(
                format!("LNm{a}"),
                Object::Dictionary(dictionary! { "Type" => "ExtGState", "CA" => f, "ca" => f, "BM" => "Multiply" }),
            );
        }
        res.set("Font", Object::Dictionary(fonts));
        res.set("ExtGState", Object::Dictionary(gstates));
        let open = doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
        let tail = doc.add_object(Stream::new(Dictionary::new(), ops));
        let mut list: Vec<Object> = vec![Object::Reference(open)];
        list.extend(doc.get_page_contents(pid).into_iter().map(Object::Reference));
        list.push(Object::Reference(tail));
        let pdict = doc.get_object_mut(pid).and_then(Object::as_dict_mut).map_err(|e| e.to_string())?;
        pdict.set("Resources", Object::Dictionary(res));
        pdict.set("Contents", list);
    }
    let mut bytes = vec![];
    doc.save_to(&mut bytes).map_err(|e| format!("Couldn't write the PDF ({e})"))?;
    crate::storage::write_atomic(out, &bytes).map_err(|e| format!("Couldn't write the PDF ({e})"))?;
    Ok(annotated)
}

/// True if a sampled row of RGB pixels looks safe to break across (either uniform paper background
/// OR a subtle ruled/grid/Cornell line without dark text or ink strokes).
fn row_is_clean(width: u32, mut pixel_at: impl FnMut(u32) -> [u8; 3]) -> bool {
    if width == 0 {
        return true;
    }
    let p0 = pixel_at(0);
    let bg_luma = (p0[0] as i32 * 3 + p0[1] as i32 * 6 + p0[2] as i32) / 10;
    let mut samples = 0usize;
    let mut heavy = 0usize;
    for x in (0..width).step_by(2) {
        samples += 1;
        let p = pixel_at(x);
        let diff = (p[0] as i32 - p0[0] as i32)
            .abs()
            .max((p[1] as i32 - p0[1] as i32).abs())
            .max((p[2] as i32 - p0[2] as i32).abs());
        if diff >= 12 {
            // Ignore thin faint ruling/margin lines (small count of low-contrast deviations),
            // but reject rows containing actual dark/saturated ink or text strokes.
            let luma = (p[0] as i32 * 3 + p[1] as i32 * 6 + p[2] as i32) / 10;
            if (luma - bg_luma).abs() > 38 || diff > 55 {
                heavy += 1;
                if heavy > 2 {
                    return false;
                }
            }
        }
    }
    samples > 0 && heavy <= 2
}

/// Stream captured RGBA tiles directly onto A4 pages without allocating a giant monolithic stitched image.
pub fn write_image_pdf_tiles(title: &str, tiles: &[image::RgbaImage], out: &Path) -> Result<usize, String> {
    let width = tiles.iter().map(|t| t.width()).max().unwrap_or(1).max(1);
    let mut offsets = Vec::with_capacity(tiles.len());
    let mut total_h = 0u32;
    for t in tiles {
        offsets.push((total_h, t.height()));
        total_h = total_h.saturating_add(t.height());
    }
    if total_h == 0 {
        return write_image_pdf(title, &image::RgbImage::from_pixel(1, 1, image::Rgb([255, 255, 255])), out);
    }
    let locate = |y: u32| -> Option<(usize, u32)> {
        let idx = offsets.partition_point(|&(start, h)| start + h <= y);
        let (start, h) = *offsets.get(idx)?;
        (y < start + h).then_some((idx, y - start))
    };
    let pixel_rgb = |x: u32, y: u32| -> [u8; 3] {
        if let Some((idx, ry)) = locate(y) {
            let t = &tiles[idx];
            if x < t.width() {
                let p = t.get_pixel(x, ry).0;
                return [p[0], p[1], p[2]];
            }
        }
        [255, 255, 255]
    };
    write_paged_pdf(
        title,
        width,
        total_h,
        |y| row_is_clean(width, |x| pixel_rgb(x, y)),
        |y0, h| {
            let mut slice = image::RgbImage::from_pixel(width, h, image::Rgb([255, 255, 255]));
            for dy in 0..h {
                if let Some((idx, ry)) = locate(y0 + dy) {
                    let t = &tiles[idx];
                    let tw = t.width().min(width);
                    for x in 0..tw {
                        let p = t.get_pixel(x, ry).0;
                        slice.put_pixel(x, dy, image::Rgb([p[0], p[1], p[2]]));
                    }
                }
            }
            slice
        },
        out,
    )
}

/// Write a tall picture of a page onto A4 pages (title on the first), breaking at clean rows.
pub fn write_image_pdf(title: &str, img: &image::RgbImage, out: &Path) -> Result<usize, String> {
    let (w, h) = (img.width().max(1), img.height().max(1));
    write_paged_pdf(
        title,
        w,
        h,
        |y| row_is_clean(w, |x| img.get_pixel(x, y).0),
        |y0, sh| image::imageops::crop_imm(img, 0, y0, w, sh).to_image(),
        out,
    )
}

fn write_paged_pdf(
    title: &str,
    width: u32,
    height: u32,
    mut is_clean_row: impl FnMut(u32) -> bool,
    mut make_slice: impl FnMut(u32, u32) -> image::RgbImage,
    out: &Path,
) -> Result<usize, String> {
    let (pw, ph, margin) = (595.0f32, 842.0f32, 40.0f32);
    let scale = (pw - 2.0 * margin) / width as f32;
    let title_h = 46.0;
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold", "Encoding" => "WinAnsiEncoding" });
    let small = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
    let mut kids = vec![];
    let mut y = 0u32;
    let mut n = 0;
    while y < height {
        let avail = (ph - 2.0 * margin - if n == 0 { title_h } else { 0.0 }) / scale;
        let mut end = (y + avail as u32).min(height);
        if end < height {
            let lo = end.saturating_sub((avail * 0.18) as u32).max(y + 1);
            if let Some(b) = (lo..end).rev().find(|&r| is_clean_row(r)) {
                end = b;
            }
        }
        let slice = make_slice(y, (end - y).max(1));
        let mut jpg = vec![];
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90)
            .encode_image(&slice)
            .map_err(|e| e.to_string())?;
        let im = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => slice.width() as i64,
                "Height" => slice.height() as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode"
            },
            jpg,
        ));
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
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), pw.into(), ph.into()],
            "Resources" => dictionary! {
                "XObject" => dictionary! { "Im0" => im },
                "Font" => dictionary! { "F1" => font, "F2" => small }
            }
        });
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
        for (i, o) in objs.iter().enumerate() {
            offs.push(pdf.len());
            pdf += &format!("{} 0 obj\n{o}\nendobj\n", i + 1);
        }
        let xref = pdf.len();
        pdf += &format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1);
        for o in offs {
            pdf += &format!("{o:010} 00000 n \n");
        }
        pdf += &format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1);
        pdf.into_bytes()
    }

    fn render_first_page(path: &Path) -> image::RgbaImage {
        let doc = hayro::hayro_syntax::Pdf::new(std::fs::read(path).unwrap()).unwrap();
        let pix = hayro::PixmapSettings { x_scale: 1.0, y_scale: 1.0, bg_color: hayro::vello_cpu::color::palette::css::WHITE };
        let png = hayro::render(&doc.pages()[0], &hayro::RenderCache::new(), &Default::default(), &Default::default(), &pix)
            .into_png()
            .unwrap();
        image::load_from_memory(&png).unwrap().to_rgba8()
    }

    #[test]
    fn printout_keeps_pages_and_adds_ink() {
        let dir = std::env::temp_dir().join(format!("leafnotes-export-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("77.pdf"), tiny_pdf()).unwrap();
        let mut page = Page::new("p");
        page.blocks.push(Block {
            kind: Kind::Image { file: "77-page1.png".into() },
            ..block(0.0, 0.0, 640.0, Kind::Text { text: String::new(), heading: 0 })
        });
        page.blocks.push(block(32.0, 32.0, 300.0, Kind::Text { text: "**Note** on top".into(), heading: 0 }));
        page.strokes.push(Stroke {
            id: 1,
            points: vec![[16.0, 80.0, 1.0], [300.0, 80.0, 1.0]],
            color: [220, 30, 30, 255],
            width: 8.0,
            highlighter: false,
            pen: PenKind::Marker,
            t: 0.0,
        });
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
    fn printout_preserves_indirect_resources_and_handles_rotation() {
        let dir = std::env::temp_dir().join(format!("leafnotes-export-rot-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Build a 2-page PDF sharing an indirect /Resources and indirect /Font dictionary, with /Rotate 90 on page 1.
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let orig_font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier" });
        let shared_fonts = doc.add_object(dictionary! { "F0" => orig_font });
        let shared_res = doc.add_object(dictionary! { "Font" => shared_fonts });
        let c1 = doc.add_object(Stream::new(dictionary! {}, b"BT /F0 14 Tf 40 40 Td (Original) Tj ET\n".to_vec()));
        let p1 = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => c1,
            "Resources" => shared_res, "Rotate" => 90,
            "MediaBox" => vec![0.into(), 0.into(), 400.into(), 500.into()]
        });
        let p2 = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => c1,
            "Resources" => shared_res,
            "MediaBox" => vec![0.into(), 0.into(), 400.into(), 500.into()]
        });
        doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![p1.into(), p2.into()], "Count" => 2 }));
        let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", cat);
        doc.save(dir.join("88.pdf")).unwrap();

        let mut page = Page::new("rot");
        page.blocks.push(block(0.0, 0.0, 640.0, Kind::Image { file: "88-page1.png".into() }));
        page.blocks.push(block(20.0, 20.0, 300.0, Kind::Table { rows: vec![vec!["A".into(), "B".into()], vec!["1".into(), "2".into()]] }));
        page.strokes.push(Stroke {
            id: 2,
            points: vec![[10.0, 50.0, 1.0], [200.0, 50.0, 1.0]],
            color: [253, 224, 71, 120],
            width: 14.0,
            highlighter: true,
            pen: PenKind::Marker,
            t: 0.0,
        });
        let out = dir.join("out.pdf");
        assert_eq!(export_printout(&page, &dir, &|_| Some(512.0), 14.0, &out).unwrap(), 1);
        let reloaded = Document::load(&out).unwrap();
        let pids = reloaded.get_pages();
        // Page 1 must have BOTH original /F0 and new /LNf, plus /LNm120 Multiply ExtGState.
        let p1_res = reloaded.get_dictionary(pids[&1]).unwrap().get(b"Resources").unwrap().as_dict().unwrap();
        let p1_fonts = p1_res.get(b"Font").unwrap().as_dict().unwrap();
        assert!(p1_fonts.has(b"F0") && p1_fonts.has(b"LNf"), "original indirect /F0 preserved alongside /LNf");
        let p1_gs = p1_res.get(b"ExtGState").unwrap().as_dict().unwrap();
        assert!(p1_gs.has(b"LNm120") && p1_gs.has(b"LNa255"), "Multiply highlighter and opaque reset ExtGState present");
        // Page 2's shared indirect resource must NOT have been mutated with /LNf!
        let p2_res_ref = reloaded.get_dictionary(pids[&2]).unwrap().get(b"Resources").unwrap().as_reference().unwrap();
        let p2_fonts_ref = reloaded.get_dictionary(p2_res_ref).unwrap().get(b"Font").unwrap().as_reference().unwrap();
        assert!(!reloaded.get_dictionary(p2_fonts_ref).unwrap().has(b"LNf"), "sibling page shared dict untouched");
        // Check rotated Tm matrix and full table rows appear in page 1 tail stream.
        let contents = reloaded.get_page_contents(pids[&1]);
        let tail = reloaded.get_object(*contents.last().unwrap()).unwrap().as_stream().unwrap();
        let s = String::from_utf8_lossy(&tail.content);
        assert!(s.contains("0 1 -1 0"), "rotated Tm matrix emitted: {s}");
        assert!(s.contains("1  |  2"), "all table rows exported: {s}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn markdown_export_preserves_literal_image_text_and_copies_files() {
        let dir = std::env::temp_dir().join(format!("leafnotes-md-{}", new_id()));
        let files = dir.join("files");
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(files.join("pic.png"), b"PNG").unwrap();
        std::fs::write(files.join("doc.pdf"), b"PDF").unwrap();
        let mut p = Page::new("Notes");
        p.blocks.push(block(0.0, 0.0, 300.0, Kind::Text { text: "Literal [image] marker stays intact".into(), heading: 0 }));
        p.blocks.push(block(0.0, 60.0, 300.0, Kind::Image { file: "pic.png".into() }));
        p.blocks.push(block(0.0, 120.0, 300.0, Kind::File { file: "doc.pdf".into(), name: "lecture.pdf".into(), size: 3 }));
        let out = dir.join("out.md");
        markdown_with_files(&p, &files, &out).unwrap();
        let md = std::fs::read_to_string(&out).unwrap();
        assert!(md.contains("Literal [image] marker stays intact"), "{md}");
        assert!(md.contains("![](<out_files/pic.png>)"), "{md}");
        assert!(md.contains("[lecture.pdf](<out_files/doc.pdf>)"), "{md}");
        assert!(dir.join("out_files/pic.png").exists() && dir.join("out_files/doc.pdf").exists());
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
        // Also verify streamed multi-tile export
        let tile = image::RgbaImage::from_fn(600, 900, |_, y| if y % 40 < 12 { image::Rgba([30, 30, 30, 255]) } else { image::Rgba([255, 255, 255, 255]) });
        let out2 = dir.join("tiles.pdf");
        let n2 = write_image_pdf_tiles("Tiles", &[tile.clone(), tile.clone(), tile], &out2).unwrap();
        assert!(n2 >= 3, "{n2} pages");
        let _ = std::fs::remove_dir_all(dir);
    }
}
