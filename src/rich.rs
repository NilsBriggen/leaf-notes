//! Markdown-lite for text notes: **bold**, *italic*, ~~strike~~, ==highlight==, `code`, $math$,
//! plus line prefixes (# heading, - bullet, 1. numbered, [ ] checkbox, > quote).
//! Editing shows the markers dimmed; viewing hides them and renders math.

use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, Stroke, TextFormat};
use std::sync::atomic::{AtomicBool, Ordering};
use crate::Tip;

pub static HAS_BOLD: AtomicBool = AtomicBool::new(false);
pub static HAS_SERIF: AtomicBool = AtomicBool::new(false);
/// Person's choice: serif titles and headings (on) or the plain UI font everywhere (off).
pub static USE_SERIF: AtomicBool = AtomicBool::new(true);
/// Line height of note text ×100, set from the person's spacing preference.
pub static LINE_X100: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(155);
pub fn line_height() -> f32 { LINE_X100.load(Ordering::Relaxed) as f32 / 100.0 }

/// The serif face used for titles and headings (falls back to the UI font).
pub fn serif(size: f32) -> FontId {
    if HAS_SERIF.load(Ordering::Relaxed) && USE_SERIF.load(Ordering::Relaxed) { FontId::new(size, FontFamily::Name("serif".into())) } else { FontId::proportional(size) }
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Style { pub bold: bool, pub italic: bool, pub strike: bool, pub code: bool, pub mark: bool }

#[derive(Debug, PartialEq, Clone)]
pub enum Piece { Marker(String), Text(String, Style), Math(String), Link(String) }

thread_local! {
    /// Lower-case titles of all pages (set by the app once a second), to show links to missing pages.
    static KNOWN_TITLES: std::cell::RefCell<std::collections::HashSet<String>> = Default::default();
}
pub fn set_known_titles(t: std::collections::HashSet<String>) { KNOWN_TITLES.with(|k| *k.borrow_mut() = t); }

/// `pieces`, remembered per line: notes are drawn every frame and their text rarely changes.
pub fn pieces_cached(line: &str) -> Vec<Piece> {
    thread_local! { static CACHE: std::cell::RefCell<std::collections::HashMap<String, Vec<Piece>>> = Default::default(); }
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(p) = c.get(line) { return p.clone(); }
        if c.len() > 5000 { c.clear(); } // shortcut: forget everything at once, fine at this size
        let p = pieces(line);
        c.insert(line.to_string(), p.clone());
        p
    })
}

/// Split one line into styled pieces. Markers only open when a matching close exists on the line.
pub fn pieces(line: &str) -> Vec<Piece> {
    let ch: Vec<char> = line.chars().collect();
    let n = ch.len();
    let mut out = vec![];
    let mut st = Style::default();
    let mut buf = String::new();
    let find = |from: usize, pat: &[char]| (from..n).find(|&k| ch[k..].starts_with(pat));
    let mut i = 0;
    macro_rules! flush { () => { if !buf.is_empty() { out.push(Piece::Text(std::mem::take(&mut buf), st)); } } }
    while i < n {
        let c = ch[i];
        let pair = i + 1 < n && ch[i + 1] == c;
        if st.code {
            if c == '`' { flush!(); out.push(Piece::Marker("`".into())); st.code = false; } else { buf.push(c); }
            i += 1;
            continue;
        }
        if c == '\\' && i + 1 < n && "*`$=~\\".contains(ch[i + 1]) {
            flush!();
            out.push(Piece::Marker("\\".into()));
            buf.push(ch[i + 1]);
            i += 2;
            continue;
        }
        if c == '[' && pair {
            // [[Page name]] links to another page
            if let Some(j) = find(i + 2, &[']', ']']).filter(|&j| j > i + 2) {
                flush!();
                out.push(Piece::Marker("[[".into()));
                out.push(Piece::Link(ch[i + 2..j].iter().collect()));
                out.push(Piece::Marker("]]".into()));
                i = j + 2;
                continue;
            }
        }
        if c == '`' && find(i + 1, &['`']).is_some() { flush!(); out.push(Piece::Marker("`".into())); st.code = true; i += 1; continue; }
        if c == '$' {
            let (open, from) = if pair { (2, i + 2) } else { (1, i + 1) };
            let close: &[char] = if pair { &['$', '$'] } else { &['$'] };
            // like Pandoc: no space just inside either $, and no digit right after a single closing $,
            // so "$5 and $6" stays money
            if let Some(j) = find(from, close).filter(|&j| j > from && !ch[from].is_whitespace() && !ch[j - 1].is_whitespace() && !(open == 1 && ch.get(j + 1).is_some_and(|c| c.is_ascii_digit()))) {
                flush!();
                let m: String = close.iter().collect();
                out.push(Piece::Marker(m.clone()));
                out.push(Piece::Math(ch[from..j].iter().collect()));
                out.push(Piece::Marker(m));
                i = j + open;
                continue;
            }
        }
        if pair && matches!(c, '*' | '~' | '=') {
            let on = match c { '*' => st.bold, '~' => st.strike, _ => st.mark };
            if on || find(i + 2, &[c, c]).is_some() {
                flush!();
                out.push(Piece::Marker(format!("{c}{c}")));
                match c { '*' => st.bold = !on, '~' => st.strike = !on, _ => st.mark = !on }
                i += 2;
                continue;
            }
        }
        if c == '*' && (st.italic || (i + 1 < n && ch[i + 1] != ' ' && find(i + 1, &['*']).is_some())) {
            flush!();
            out.push(Piece::Marker("*".into()));
            st.italic = !st.italic;
            i += 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    flush!();
    out
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum LineKind { Heading(u8), Bullet, Number, Check(bool), Quote, Plain }

/// Returns the kind of line and the byte length of its prefix (including leading spaces).
pub fn classify(line: &str) -> (LineKind, usize) {
    let t = line.trim_start();
    let lead = line.len() - t.len();
    for (p, k) in [("### ", LineKind::Heading(3)), ("## ", LineKind::Heading(2)), ("# ", LineKind::Heading(1)),
        ("- [ ] ", LineKind::Check(false)), ("- [x] ", LineKind::Check(true)), ("[ ] ", LineKind::Check(false)), ("[x] ", LineKind::Check(true)),
        ("- ", LineKind::Bullet), ("* ", LineKind::Bullet), ("• ", LineKind::Bullet), ("> ", LineKind::Quote)] {
        if t.starts_with(p) { return (k, lead + p.len()); }
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && t[digits..].starts_with(". ") { return (LineKind::Number, lead + digits + 2); }
    (LineKind::Plain, 0)
}

pub struct Colors { pub text: Color32, pub strong: Color32, pub dim: Color32, pub accent: Color32, pub mark: Color32, pub code_bg: Color32 }

impl Colors {
    pub fn from(ui: &egui::Ui, accent: Color32) -> Self {
        let v = ui.visuals();
        Colors {
            text: v.text_color(), strong: v.strong_text_color(), dim: v.weak_text_color().gamma_multiply(0.8), accent,
            mark: if v.dark_mode { Color32::from_rgba_unmultiplied(200, 170, 0, 90) } else { Color32::from_rgb(255, 240, 150) },
            code_bg: if v.dark_mode { Color32::from_gray(45) } else { Color32::from_rgb(238, 241, 243) },
        }
    }
}

fn heading_size(base: f32, level: u8) -> f32 { match level { 1 => base * 1.5, 2 => base * 1.25, 3 => base * 1.1, _ => base } }


fn fmt(size: f32, s: Style, color: Color32, c: &Colors, head: bool) -> TextFormat {
    let bold = s.bold && HAS_BOLD.load(Ordering::Relaxed);
    TextFormat {
        line_height: Some(size * if head { 1.3 } else { line_height() }),
        font_id: if s.code { FontId::monospace(size * 0.88) } else if head { serif(size) } else if bold { FontId::new(size, FontFamily::Name("bold".into())) } else { FontId::proportional(size) },
        color: if s.bold && !bold { c.strong } else { color },
        italics: s.italic,
        strikethrough: if s.strike { Stroke::new(1.0, color) } else { Stroke::NONE },
        background: if s.code { c.code_bg } else if s.mark { c.mark } else { Color32::TRANSPARENT },
        ..Default::default()
    }
}

/// Layout for the editor: every character kept, markers dimmed.
pub fn edit_job(text: &str, base: f32, heading: u8, c: &Colors) -> LayoutJob {
    let mut job = LayoutJob::default();
    let block_color = if heading > 0 { c.strong } else { c.text };
    for line in text.split_inclusive('\n') {
        let (body, nl) = line.strip_suffix('\n').map_or((line, ""), |b| (b, "\n"));
        let (kind, plen) = classify(body);
        let (size, color) = match kind {
            LineKind::Heading(l) => (heading_size(base, l), c.strong),
            _ => (heading_size(base, heading), block_color),
        };
        let prefix_color = if matches!(kind, LineKind::Plain) { c.dim } else { c.accent };
        let lh = Some(size * if matches!(kind, LineKind::Heading(_)) || heading > 0 { 1.3 } else { line_height() });
        job.append(&body[..plen], 0.0, TextFormat { font_id: FontId::proportional(size), color: prefix_color, line_height: lh, ..Default::default() });
        for p in pieces_cached(&body[plen..]) {
            match p {
                Piece::Marker(m) => job.append(&m, 0.0, TextFormat { font_id: FontId::proportional(size), color: c.dim, line_height: lh, ..Default::default() }),
                Piece::Text(t, s) => job.append(&t, 0.0, fmt(size, s, color, c, heading > 0 || matches!(kind, LineKind::Heading(_)))),
                Piece::Math(m) => job.append(&m, 0.0, TextFormat { font_id: FontId::monospace(size * 0.88), color: c.accent, line_height: lh, ..Default::default() }),
                Piece::Link(l) => job.append(&l, 0.0, TextFormat { font_id: FontId::proportional(size), color: c.accent, underline: Stroke::new(1.0, c.accent), line_height: lh, ..Default::default() }),
            }
        }
        job.append(nl, 0.0, TextFormat { font_id: FontId::proportional(size), color, line_height: lh, ..Default::default() });
    }
    if text.is_empty() { job.append("", 0.0, TextFormat { font_id: FontId::proportional(heading_size(base, heading)), color: block_color, line_height: Some(heading_size(base, heading) * line_height()), ..Default::default() }); }
    job
}

/// Read-only rendering with markers hidden, $math$ drawn and [[links]] clickable.
/// Returns whether the text was clicked, and which page link was clicked (if any).
pub fn view(ui: &mut egui::Ui, text: &str, base: f32, heading: u8, c: &Colors) -> (bool, Option<String>) {
    let (clicked, link, _) = view_ticks(ui, text, base, heading, c);
    (clicked, link)
}

/// `view`, also saying which line's "[ ]" box was clicked (it can be ticked without editing the text).
pub fn view_ticks(ui: &mut egui::Ui, text: &str, base: f32, heading: u8, c: &Colors) -> (bool, Option<String>, Option<usize>) {
    let mut clicked = false;
    let mut link = None;
    let mut tick = None;
    ui.style_mut().interaction.selectable_labels = false;
    let mut number = 0;
    for (li, line) in text.split('\n').enumerate() {
        let (kind, plen) = classify(line);
        let (size, color) = match kind {
            LineKind::Heading(l) => (heading_size(base, l), c.strong),
            _ => (heading_size(base, heading), if heading > 0 { c.strong } else { c.text }),
        };
        if line.trim().is_empty() { ui.add_space(size * 0.8); continue; }
        number = if kind == LineKind::Number { next_number(line, number) } else { 0 };
        let indent = (line.len() - line.trim_start().len()) as f32 * 10.0; // two spaces: one sub-point step
        if matches!(kind, LineKind::Heading(_)) { ui.add_space(size * 0.3); }
        ui.spacing_mut().item_spacing.y = size * 0.3;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(indent);
            let lead = match kind {
                LineKind::Bullet => Some("•  ".to_string()),
                LineKind::Number => Some(format!("{number}.  ")),
                LineKind::Check(d) => Some(if d { "☑  " } else { "☐  " }.to_string()),
                _ => None,
            };
            if let Some(l) = lead {
                let r = ui.add(egui::Label::new(egui::RichText::new(l).size(size).color(c.accent)).sense(egui::Sense::click()));
                if matches!(kind, LineKind::Check(_)) && r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { tick = Some(li); }
            }
            if kind == LineKind::Quote {
                let (r, _) = ui.allocate_exact_size(egui::vec2(3.0, size * 1.2), egui::Sense::hover());
                ui.painter().rect_filled(r, 1.0, c.accent);
                ui.add_space(8.0);
            }
            let mut job = LayoutJob::default();
            let done = matches!(kind, LineKind::Check(true));
            for p in pieces_cached(&line[plen..]) {
                match p {
                    Piece::Marker(_) => {}
                    Piece::Text(t, mut s) => {
                        s.italic |= kind == LineKind::Quote;
                        s.strike |= done;
                        job.append(&t, 0.0, fmt(size, s, if done { c.dim } else { color }, c, heading > 0 || matches!(kind, LineKind::Heading(_))));
                    }
                    Piece::Math(m) => {
                        if !job.is_empty() { clicked |= ui.label(std::mem::take(&mut job)).clicked(); }
                        clicked |= crate::math::math_label(ui, &m, size).clicked();
                    }
                    Piece::Link(l) => {
                        if !job.is_empty() { clicked |= ui.label(std::mem::take(&mut job)).clicked(); }
                        // a link to a page that doesn't exist (yet) looks like it, instead of like a working link
                        let exists = KNOWN_TITLES.with(|k| { let k = k.borrow(); k.is_empty() || k.contains(&l.trim().to_lowercase()) });
                        let col = if exists { c.accent } else { c.dim };
                        let r = ui.add(egui::Label::new(egui::RichText::new(format!("↗ {l}")).size(size).color(col).underline()).sense(egui::Sense::click()))
                            .on_hover_cursor(egui::CursorIcon::PointingHand).tip(if exists { "Open this page" } else { "No page with this name yet — click to create it" });
                        if r.clicked() { link = Some(l); }
                    }
                }
            }
            if !job.is_empty() { clicked |= ui.label(job).clicked(); }
        });
    }
    (clicked, link, tick)
}

/// Tab / Shift+Tab on a list line: indent it by two spaces, or take two away (sub-points).
/// The new cursor (char index), or None when the cursor isn't on a list line.
pub fn indent_line(text: &mut String, cursor: usize, out: bool) -> Option<usize> {
    let cb = byte(text, cursor);
    let start = text[..cb].rfind('\n').map_or(0, |i| i + 1);
    let end = text[cb..].find('\n').map_or(text.len(), |i| cb + i);
    if !matches!(classify(&text[start..end]).0, LineKind::Bullet | LineKind::Number | LineKind::Check(_)) { return None; }
    if out {
        let n = text[start..end].chars().take_while(|c| *c == ' ').count().min(2);
        if n == 0 { return Some(cursor); }
        text.replace_range(start..start + n, "");
        Some(cursor.saturating_sub(n))
    } else {
        text.insert_str(start, "  ");
        Some(cursor + 2)
    }
}

/// The number shown for a numbered line: a list starts at the number typed (like Markdown), then counts on.
fn next_number(line: &str, prev: usize) -> usize {
    if prev > 0 { return prev + 1; }
    line.trim_start().chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(1)
}

/// Tick or untick the "[ ]" box of line `n` (the box at its start, not one written later in the line).
pub fn toggle_check(text: &mut String, n: usize) {
    let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
    let Some(l) = lines.get_mut(n) else { return };
    let (LineKind::Check(_), p) = classify(l) else { return };
    for (from, to) in [("[ ]", "[x]"), ("[x]", "[ ]"), ("[X]", "[ ]")] {
        if let Some(i) = l[..p].find(from) { l.replace_range(i..i + 3, to); break; }
    }
    *text = lines.join("\n");
}

// ---------- editing helpers (char indices) ----------

fn byte(text: &str, ci: usize) -> usize { text.char_indices().nth(ci).map_or(text.len(), |(b, _)| b) }

/// Wrap the selection a..b in `m` (or unwrap if already wrapped). Returns the new selection.
/// A selection over several lines gets the marks on each line (a mark can't span lines).
pub fn toggle_wrap(text: &mut String, a: usize, b: usize, m: &str) -> (usize, usize) {
    let ml = m.chars().count();
    let chars: Vec<char> = text.chars().collect();
    let mc: Vec<char> = m.chars().collect();
    if chars.get(a..b).is_some_and(|s| s.contains(&'\n')) {
        let sel: String = chars[a..b].iter().collect();
        let lines: Vec<&str> = sel.split('\n').collect();
        // each line's text without its list mark and spaces: (lead, text, tail)
        let parts = |l: &str| { let p = classify(l).1.max(l.len() - l.trim_start().len()); let e = l.trim_end().len().max(p); (l[..p].to_string(), l[p..e].to_string(), l[e..].to_string()) };
        let wrapped = |t: &str| t.chars().count() >= 2 * ml && t.starts_with(m) && t.ends_with(m);
        let all = lines.iter().map(|l| parts(l)).filter(|(_, t, _)| !t.is_empty()).all(|(_, t, _)| wrapped(&t));
        let new: Vec<String> = lines.iter().map(|l| {
            let (lead, t, tail) = parts(l);
            if t.is_empty() { l.to_string() } else if all { format!("{lead}{}{tail}", &t[m.len()..t.len() - m.len()]) } else { format!("{lead}{m}{t}{m}{tail}") }
        }).collect();
        let joined = new.join("\n");
        let n = joined.chars().count();
        let (ab, bb) = (byte(text, a), byte(text, b));
        text.replace_range(ab..bb, &joined);
        return (a, a + n);
    }
    // "*" next to "**word**" is the bold's, not italics: only an odd run of stars counts as italic
    let run = |it: &mut dyn Iterator<Item = &char>| it.take_while(|c| **c == mc[0]).count();
    let outside = |k: usize| if ml == 1 { k % 2 == 1 } else { k >= ml };
    let (before, after) = (run(&mut chars[..a.min(chars.len())].iter().rev()), run(&mut chars[b.min(chars.len())..].iter()));
    let marks_match = a >= ml && b + ml <= chars.len() && chars[a - ml..a] == mc[..] && chars[b..b + ml] == mc[..];
    if marks_match && outside(before) && outside(after) {
        let (bb, ab) = (byte(text, b), byte(text, a));
        text.replace_range(bb..bb + m.len(), "");
        text.replace_range(ab - m.len()..ab, "");
        return (a - ml, b - ml);
    }
    let (ab, bb) = (byte(text, a), byte(text, b));
    text.insert_str(bb, m);
    text.insert_str(ab, m);
    (a + ml, b + ml)
}

/// Toggle a line prefix ("- ", "1. ", "[ ] ", "# ", "> ") on every line touched by a..b.
pub fn toggle_prefix(text: &mut String, a: usize, b: usize, prefix: &str) -> (usize, usize) {
    let (ab, bb) = (byte(text, a), byte(text, b));
    let start = text[..ab].rfind('\n').map_or(0, |i| i + 1);
    let end = text[bb..].find('\n').map_or(text.len(), |i| bb + i);
    let old = text[start..end].to_string();
    let lines: Vec<&str> = old.split('\n').collect();
    let has = |l: &str| { let (k, p) = classify(l); (k != LineKind::Plain).then_some(p) };
    let numbered = prefix == "1. ";
    // the same prefix exactly ("## " isn't "# ", "- [ ] " isn't "- "): another kind is switched, not removed
    // with several lines, blank ones are left as they are (no bullet on an empty line, no gap in the numbers)
    let skip = |l: &str| lines.len() > 1 && l.trim().is_empty();
    let all = lines.iter().filter(|l| !skip(l)).all(|l| match has(l) { Some(p) => l[..p].trim() == prefix.trim() || (numbered && classify(l).0 == LineKind::Number), None => false });
    let mut k = 0;
    let new: Vec<String> = lines.iter().map(|l| {
        if skip(l) { return l.to_string(); }
        let indent = &l[..l.len() - l.trim_start().len()]; // sub-points stay sub-points
        let rest = has(l).map_or(l.trim_start(), |p| &l[p..]);
        k += 1;
        if all { format!("{indent}{rest}") } else if numbered { format!("{indent}{k}. {rest}") } else { format!("{indent}{prefix}{rest}") }
    }).collect();
    let joined = new.join("\n");
    let delta = joined.chars().count() as isize - old.chars().count() as isize;
    text.replace_range(start..end, &joined);
    let first_delta = new[0].chars().count() as isize - lines[0].chars().count() as isize;
    let a2 = (a as isize + first_delta).max(text[..start].chars().count() as isize) as usize;
    (a2, (b as isize + delta).max(a2 as isize) as usize)
}

/// After Enter: continue a list on the new line, or end it if the previous item was empty.
pub fn continue_list(text: &mut String, cursor: usize) -> Option<usize> {
    let cb = byte(text, cursor);
    if cursor == 0 || !text[..cb].ends_with('\n') { return None; }
    let prev_end = cb - 1;
    let prev_start = text[..prev_end].rfind('\n').map_or(0, |i| i + 1);
    let prev = &text[prev_start..prev_end];
    let (kind, plen) = classify(prev);
    if matches!(kind, LineKind::Plain | LineKind::Heading(_)) { return None; }
    if prev[plen..].trim().is_empty() {
        let removed = prev.chars().count();
        text.replace_range(prev_start..prev_end, "");
        return Some(cursor - removed);
    }
    let lead = &prev[..prev.len() - prev.trim_start().len()];
    let next = match kind {
        LineKind::Number => format!("{lead}{}. ", prev.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().unwrap_or(0) + 1),
        LineKind::Check(_) => format!("{lead}{}", if prev.trim_start().starts_with("- ") { "- [ ] " } else { "[ ] " }),
        _ => prev[..plen].to_string(),
    };
    let n = next.chars().count();
    text.insert_str(cb, &next);
    Some(cursor + n)
}

/// Apply `f(text, sel_start, sel_end) -> new selection` to a TextEdit's buffer at its stored cursor.
pub fn edit_at_cursor(ctx: &egui::Context, id: egui::Id, text: &mut String, f: impl FnOnce(&mut String, usize, usize) -> (usize, usize)) {
    use egui::text::{CCursor, CCursorRange};
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let len = text.chars().count();
    let (a, b) = state.cursor.char_range().map_or((len, len), |r| {
        let (x, y) = (r.primary.index.0.min(len), r.secondary.index.0.min(len));
        (x.min(y), x.max(y))
    });
    let (na, nb) = f(text, a, b);
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(na), CCursor::new(nb))));
    state.store(ctx, id);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splits_inline_styles() {
        let p = pieces("a **b** *c* $x^2$ `d*e` 2 * 3");
        assert!(p.contains(&Piece::Text("b".into(), Style { bold: true, ..Default::default() })));
        assert!(p.contains(&Piece::Text("c".into(), Style { italic: true, ..Default::default() })));
        assert!(p.contains(&Piece::Math("x^2".into())));
        assert!(p.contains(&Piece::Text("d*e".into(), Style { code: true, ..Default::default() })));
        assert_eq!(p.last(), Some(&Piece::Text(" 2 * 3".into(), Style::default())));
        assert_eq!(pieces("costs $5 today"), vec![Piece::Text("costs $5 today".into(), Style::default())]);
        assert!(!pieces("costs $5 and $6").iter().any(|p| matches!(p, Piece::Math(_))), "money isn't maths");
        assert!(pieces("so $a+b$ holds").contains(&Piece::Math("a+b".into())));
        assert!(pieces("see [[Week 2]] too").contains(&Piece::Link("Week 2".into())));
        assert!(!pieces("a [b] c").iter().any(|p| matches!(p, Piece::Link(_))));
    }
    #[test]
    fn editing_helpers() {
        let mut t = String::from("hello world");
        assert_eq!(toggle_wrap(&mut t, 6, 11, "**"), (8, 13));
        assert_eq!(t, "hello **world**");
        assert_eq!(toggle_wrap(&mut t, 8, 13, "**"), (6, 11));
        assert_eq!(t, "hello world");
        let mut l = String::from("one\ntwo");
        toggle_prefix(&mut l, 0, 7, "- ");
        assert_eq!(l, "- one\n- two");
        toggle_prefix(&mut l, 0, 11, "- ");
        assert_eq!(l, "one\ntwo");
        toggle_prefix(&mut l, 0, 7, "1. ");
        assert_eq!(l, "1. one\n2. two");
        // another kind of line is switched, not stripped
        let mut li = String::from("a\n- b");
        assert_eq!(indent_line(&mut li, 4, false), Some(6));
        assert_eq!(li, "a\n  - b");
        assert_eq!(indent_line(&mut li, 6, true), Some(4));
        assert_eq!(indent_line(&mut String::from("plain"), 2, false), None);
        let mut h = String::from("## Title");
        toggle_prefix(&mut h, 0, 2, "# ");
        assert_eq!(h, "# Title");
        let mut c = String::from("- milk\n");
        assert_eq!(continue_list(&mut c, 7), Some(9));
        assert_eq!(c, "- milk\n- ");
        let mut e = String::from("- milk\n- \n");
        assert_eq!(continue_list(&mut e, 10), Some(8));
        assert_eq!(e, "- milk\n\n");
        let mut n = String::from("1. a\n");
        continue_list(&mut n, 5);
        assert_eq!(n, "1. a\n2. ");
        // list buttons keep sub-points indented, skip blank lines and number only what gets a number
        let mut sub = String::from("one\n  two\n\nthree");
        toggle_prefix(&mut sub, 0, 15, "1. ");
        assert_eq!(sub, "1. one\n  2. two\n\n3. three");
        let n = sub.chars().count();
        toggle_prefix(&mut sub, 0, n, "1. ");
        assert_eq!(sub, "one\n  two\n\nthree");
        // italics inside bold adds a star instead of eating one of the bold's
        let mut b = String::from("**word**");
        assert_eq!(toggle_wrap(&mut b, 2, 6, "*"), (3, 7));
        assert_eq!(b, "***word***");
        assert_eq!(toggle_wrap(&mut b, 3, 7, "*"), (2, 6));
        assert_eq!(b, "**word**");
        let mut i = String::from("*it*");
        toggle_wrap(&mut i, 1, 3, "*");
        assert_eq!(i, "it");
        // across lines: each line gets its own marks
        let mut ml = String::from("one\n\ntwo");
        assert_eq!(toggle_wrap(&mut ml, 0, 8, "**"), (0, 16));
        assert_eq!(ml, "**one**\n\n**two**");
        toggle_wrap(&mut ml, 0, 16, "**");
        assert_eq!(ml, "one\n\ntwo");
        let mut li = String::from("- a\n- b");
        toggle_wrap(&mut li, 0, 7, "*");
        assert_eq!(li, "- *a*\n- *b*");
        // only the box at the start of the line is ticked
        let mut c = String::from("- [ ] buy [ ] tape");
        toggle_check(&mut c, 0);
        assert_eq!(c, "- [x] buy [ ] tape");
        let mut p = String::from("plain [ ] text");
        toggle_check(&mut p, 0);
        assert_eq!(p, "plain [ ] text");
        // numbered lists start at the number typed
        assert_eq!((next_number("3. c", 0), next_number("1. d", 3), next_number("  1. x", 0)), (3, 4, 1));
    }
}
