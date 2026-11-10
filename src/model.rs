use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub fn new_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // the time plus some randomness: two devices started at the same moment still count from different places (and stay below 2^53)
    let mut r = [0u8; 4];
    let _ = getrandom::fill(&mut r);
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(1) + u32::from_le_bytes(r) as u64;
    let _ = NEXT.compare_exchange(0, seed, Ordering::SeqCst, Ordering::SeqCst);
    NEXT.fetch_add(1, Ordering::SeqCst)
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Store {
    pub notebooks: Vec<Notebook>,
    #[serde(default)] pub trash: Vec<Trashed>,
    /// Flashcard progress, keyed by `card<id>` (deck pages) or `card_key` of the question (cards in notes).
    #[serde(default)] pub study: std::collections::HashMap<String, CardState>,
}

/// Spaced-repetition state of one flashcard (Leitner box and when it's next due).
#[derive(Clone, Copy, Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct CardState { pub bx: u8, pub due: u64 }

/// An audio recording on a page; `start` is when it began (Unix seconds), so notes taken
/// meanwhile can be matched to the moment they were written.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Recording { pub id: u64, pub file: String, pub start: f64, pub secs: f32 }

pub fn now_f() -> f64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0) }

/// Deleted things wait here so a mistaken delete can be restored.
#[derive(Clone, Serialize, Deserialize)]
pub struct Trashed {
    pub label: String, pub deleted: u64, pub item: TrashItem,
    /// Where it was (its place among its siblings), so a restore puts it back there.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub at: Option<usize>,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum TrashItem { Page { notebook: u64, section: u64, page: Page }, Section { notebook: u64, section: Section }, Notebook(Notebook) }

#[derive(Clone, Serialize, Deserialize)]
pub struct Notebook { pub id: u64, pub name: String, pub color: [u8; 3], pub sections: Vec<Section> }

#[derive(Clone, Serialize, Deserialize)]
pub struct Section {
    pub id: u64, pub name: String, pub color: [u8; 3], pub pages: Vec<Page>,
    /// Password-locked: the pages, encrypted (see vault.rs). While locked, `pages` holds only a placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub lock: Option<Lock>,
}

/// A locked section's encrypted pages: base64 salt (for the password), nonce and data.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Lock { pub salt: String, pub nonce: String, pub data: String }

impl Section {
    /// The page shown in place of a locked section's pages (its id is the section's, which no page has).
    pub fn placeholder(&self) -> Page { Page { id: self.id, title: "🔒 Locked".into(), created: 0, kind: PageKind::Locked, ..Page::new("") } }
    /// Locked and not opened in this session.
    pub fn sealed(&self) -> bool { self.lock.is_some() && self.pages.iter().all(|p| p.kind == PageKind::Locked) }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum Paper { #[default] Plain, Ruled, Grid, Dots, Cornell, Music }

/// Every page background: (paper, icon, name).
pub const PAPERS: [(Paper, &str, &str); 6] = [(Paper::Plain, "▢", "Plain"), (Paper::Ruled, "☰", "Ruled"), (Paper::Grid, "▦", "Grid"), (Paper::Dots, "⁘", "Dots"), (Paper::Cornell, "◫", "Cornell"), (Paper::Music, "♫", "Music")];

/// What a page is: free-form notes, or a deck of flashcards.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum PageKind { #[default] Notes, Deck, Locked }

/// One flashcard in a deck page.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct DeckCard { pub id: u64, pub front: String, pub back: String }

impl DeckCard { pub fn new(front: &str, back: &str) -> Self { DeckCard { id: new_id(), front: front.into(), back: back.into() } } }

#[derive(Clone, Serialize, Deserialize)]
pub struct Page {
    pub id: u64,
    pub title: String,
    pub created: u64,
    #[serde(default)] pub level: u8, // 0 = page, 1 = subpage
    #[serde(default)] pub paper: Paper,
    pub blocks: Vec<Block>,
    pub strokes: Vec<Stroke>,
    #[serde(default)] pub recordings: Vec<Recording>,
    #[serde(default)] pub kind: PageKind,
    #[serde(default)] pub cards: Vec<DeckCard>,
    /// Set on the copy sync makes when two devices changed this page at once: the page it came from.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub other_of: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum Tag { #[default] None, Todo, Important, Question, Done }

#[derive(Clone, Serialize, Deserialize)]
pub struct Block { pub id: u64, pub pos: [f32; 2], pub width: f32, #[serde(default)] pub tag: Tag, pub kind: Kind, /** when written (Unix s), for audio sync */ #[serde(default)] pub t: f64 }

#[derive(Clone, Serialize, Deserialize)]
pub enum Kind {
    Text { text: String, #[serde(default)] heading: u8 },
    Math { src: String },
    Checklist { items: Vec<(bool, String)> },
    Table { rows: Vec<Vec<String>> },
    Code { lang: String, code: String },
    Image { file: String },
    File { file: String, name: String, size: u64 },
    Link { url: String, title: String },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Stroke { pub id: u64, pub points: Vec<[f32; 3]>, pub color: [u8; 4], pub width: f32, pub highlighter: bool, #[serde(default)] pub pen: PenKind, /** when drawn (Unix s), for audio sync */ #[serde(default)] pub t: f64 }

/// How a pen lays down ink (older notes were drawn with the fountain pen).
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum PenKind { #[default] Fountain, Ballpoint, Pencil, Calligraphy, Marker }

impl PenKind {
    pub const ALL: [PenKind; 5] = [PenKind::Fountain, PenKind::Ballpoint, PenKind::Pencil, PenKind::Calligraphy, PenKind::Marker];
    pub fn name(self) -> &'static str {
        match self { PenKind::Fountain => "Fountain pen", PenKind::Ballpoint => "Ballpoint", PenKind::Pencil => "Pencil", PenKind::Calligraphy => "Calligraphy", PenKind::Marker => "Marker" }
    }
}

/// One of the person's pens.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
pub struct Pen { pub kind: PenKind, pub color: [u8; 4], pub width: f32 }

pub fn default_pens() -> Vec<Pen> {
    vec![
        Pen { kind: PenKind::Fountain, color: [30, 41, 59, 255], width: 2.5 },
        Pen { kind: PenKind::Ballpoint, color: [37, 99, 235, 255], width: 2.0 },
        Pen { kind: PenKind::Ballpoint, color: [220, 38, 38, 255], width: 2.0 },
        Pen { kind: PenKind::Pencil, color: [75, 85, 99, 255], width: 2.0 },
        Pen { kind: PenKind::Calligraphy, color: [15, 118, 110, 255], width: 4.0 },
        Pen { kind: PenKind::Marker, color: [22, 163, 74, 235], width: 5.0 },
    ]
}

pub const PALETTE: [[u8; 3]; 8] = [
    [15, 118, 110], [232, 106, 90], [217, 155, 40], [79, 90, 200], [205, 70, 120], [40, 140, 200], [110, 140, 60], [100, 116, 139],
];

impl Page {
    pub fn new(title: &str) -> Self {
        Page { id: new_id(), title: title.into(), created: now_secs(), level: 0, paper: Paper::Plain, blocks: vec![], strokes: vec![], recordings: vec![], kind: PageKind::Notes, cards: vec![], other_of: None }
    }
    /// The page as Markdown (blocks in reading order), used for search, AI context and export.
    pub fn plain_text(&self) -> String {
        let mut out = format!("# {}\n\n", self.title);
        let mut blocks: Vec<&Block> = self.blocks.iter().collect();
        blocks.sort_by(|a, b| (a.pos[1] / 40.0).floor().total_cmp(&(b.pos[1] / 40.0).floor()).then(a.pos[0].total_cmp(&b.pos[0])));
        for b in blocks {
            match b.tag { Tag::Done => out += "☑ ", Tag::Todo => out += "☐ ", Tag::Important => out += "★ ", Tag::Question => out += "❓ ", Tag::None => {} }
            match &b.kind {
                Kind::Text { text, heading } => { if *heading > 0 && *heading < 4 { out += &"#".repeat(*heading as usize + 1); out.push(' '); } out += text }
                Kind::Math { src } => out += &format!("$${src}$$"),
                Kind::Checklist { items } => for (d, t) in items { out += &format!("- [{}] {t}\n", if *d { "x" } else { " " }) },
                Kind::Table { rows } => for (i, r) in rows.iter().enumerate() {
                    let cells: Vec<String> = r.iter().map(|c| c.replace('|', "\\|").replace('\n', " ")).collect();
                    out += &format!("| {} |\n", cells.join(" | "));
                    // Markdown needs the line under the header row to show a table at all
                    if i == 0 { out += &format!("|{}\n", " --- |".repeat(r.len().max(1))); }
                },
                Kind::Code { lang, code } => out += &format!("```{lang}\n{code}\n```"),
                Kind::Image { .. } => out += "[image]",
                Kind::File { name, .. } => out += &format!("[file: {name}]"),
                Kind::Link { url, title } => out += &format!("[{}]({url})", if title.is_empty() { url } else { title }),
            }
            out += "\n\n";
        }
        if !self.strokes.is_empty() { out += &format!("[{} handwritten strokes]\n", self.strokes.len()); }
        for c in &self.cards { out += &format!("- {} :: {}\n", c.front.replace('\n', " "), c.back.replace('\n', " ")); }
        out
    }
}

impl Kind {
    pub fn summary(&self) -> String {
        match self {
            Kind::Text { text, .. } => text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim_start_matches(['#', ' ', '-', '>']).to_string(),
            Kind::Math { src } => src.clone(),
            Kind::Checklist { items } => items.iter().map(|i| i.1.as_str()).collect::<Vec<_>>().join(", "),
            Kind::Table { rows } => rows.first().map(|r| r.join(" · ")).unwrap_or_default(),
            Kind::Code { code, .. } => code.lines().next().unwrap_or("").to_string(),
            Kind::Image { .. } => "Picture".into(),
            Kind::File { name, .. } => name.clone(),
            Kind::Link { url, title } => if title.is_empty() { url.clone() } else { title.clone() },
        }
    }
}

/// Turn Markdown (e.g. an AI reply) into note blocks: code fences, $$math$$, checklists, tables, text.
/// A page made from a Markdown file: its notes stacked down the page.
pub fn page_from_markdown(title: &str, md: &str, width: f32) -> Page {
    let mut p = Page::new(title);
    let mut y = 8.0;
    for k in blocks_from_markdown(md) {
        // a rough height so the notes don't overlap before they're first drawn (they size themselves then)
        let lines = match &k { Kind::Text { text, .. } => text.lines().map(|l| 1 + l.len() / 70).sum::<usize>(), Kind::Checklist { items } => items.len(), Kind::Table { rows } => rows.len() + 1, Kind::Code { code, .. } => code.lines().count() + 2, _ => 3 };
        let w = if matches!(k, Kind::Checklist { .. } | Kind::Table { .. }) { 380.0 } else { width };
        p.blocks.push(block(24.0, y, w, k));
        y += 26.0 * lines.max(1) as f32 + 18.0;
    }
    p
}

pub fn blocks_from_markdown(md: &str) -> Vec<Kind> {
    let mut out = vec![];
    let mut text = String::new();
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;
    let flush = |text: &mut String, out: &mut Vec<Kind>| {
        let t = text.trim_matches('\n').to_string();
        if !t.trim().is_empty() { out.push(Kind::Text { text: t, heading: 0 }); }
        text.clear();
    };
    while i < lines.len() {
        let l = lines[i].trim();
        if let Some(lang) = l.strip_prefix("```") {
            flush(&mut text, &mut out);
            let mut code = vec![];
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") { code.push(lines[i]); i += 1; }
            out.push(Kind::Code { lang: if lang.is_empty() { "text".into() } else { lang.trim().into() }, code: code.join("\n") });
        } else if (l.starts_with("$$") || l.starts_with("\\["))
            // only when it ends within a few lines: an unclosed $$ stays text instead of swallowing the rest
            && std::iter::once(&l[2..]).chain(lines.iter().skip(i + 1).take(30).map(|x| x.trim())).any(|x| x.ends_with("$$") || x.ends_with("\\]")) {
            flush(&mut text, &mut out);
            let mut src = l.to_string();
            while !(src.len() > 2 && (src.ends_with("$$") || src.ends_with("\\]"))) && i + 1 < lines.len() { i += 1; src += "\n"; src += lines[i].trim(); }
            let src = src.trim_start_matches("$$").trim_end_matches("$$").trim_start_matches("\\[").trim_end_matches("\\]").trim().to_string();
            out.push(Kind::Math { src });
        } else if l.starts_with("- [ ]") || l.starts_with("- [x]") || l.starts_with("- [X]") {
            flush(&mut text, &mut out);
            let mut items = vec![];
            while i < lines.len() {
                let t = lines[i].trim();
                let done = t.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("- [x]"));
                if !done && !t.starts_with("- [ ]") { break; }
                items.push((done, t[5..].trim().to_string()));
                i += 1;
            }
            out.push(Kind::Checklist { items });
            continue;
        } else if l.starts_with('|') && l.ends_with('|') {
            flush(&mut text, &mut out);
            let mut rows = vec![];
            while i < lines.len() && lines[i].trim().starts_with('|') {
                let t = lines[i].trim().trim_matches('|');
                if !t.chars().all(|c| matches!(c, '-' | ':' | '|' | ' ')) { rows.push(t.split('|').map(|c| c.trim().to_string()).collect()); }
                i += 1;
            }
            out.push(Kind::Table { rows });
            continue;
        } else {
            text += lines[i];
            text.push('\n');
        }
        i += 1;
    }
    flush(&mut text, &mut out);
    out
}

/// Extra points along the way, so no two in a row are more than `step` apart (a ruler line or a
/// box is otherwise just its corners, and the eraser and lasso, which look at points, miss its middle).
pub fn densify(pts: &[[f32; 3]], step: f32) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(pts.len());
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = ((b[0] - a[0]).hypot(b[1] - a[1]) / step.max(0.5)).ceil().clamp(1.0, 10_000.0) as usize;
        for k in 0..n { let t = k as f32 / n as f32; out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]); }
    }
    if let Some(l) = pts.last() { out.push(*l); }
    out
}

/// Whether the stroke passes within `r` of `p` (anywhere along it, not only at its points).
pub fn stroke_hit(s: &Stroke, p: [f32; 2], r: f32) -> bool {
    let near = |a: &[f32; 3], b: &[f32; 3]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
        (a[0] + dx * t - p[0]).hypot(a[1] + dy * t - p[1]) < r
    };
    match s.points.as_slice() { [] => false, [a] => near(a, a), pts => pts.windows(2).any(|w| near(&w[0], &w[1])) }
}

/// Cards from pasted text: "front :: back", "front<Tab>back", or "Q: …" / "A: …" line pairs.
pub fn cards_from_text(t: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut q: Option<String> = None;
    for l in t.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(x) = l.strip_prefix("Q:").or_else(|| l.strip_prefix("q:")) { q = Some(x.trim().to_string()); continue; }
        if let Some(x) = l.strip_prefix("A:").or_else(|| l.strip_prefix("a:")) { if let Some(f) = q.take() { out.push((f, x.trim().to_string())); } continue; }
        if let Some((f, b)) = l.split_once("::").or_else(|| l.split_once('\t')) {
            let (f, b) = (f.trim().trim_start_matches(['-', '*', ' ']), b.trim());
            if !f.is_empty() && !b.is_empty() { out.push((f.to_string(), b.to_string())); }
        }
    }
    out
}

/// Point `[[old]]` links (any letter case) at `new`. How many were changed.
pub fn relink(text: &mut String, old: &str, new: &str) -> usize {
    let (want, mut n, mut from) = (old.trim().to_lowercase(), 0, 0);
    while let Some(a) = text[from..].find("[[").map(|i| from + i) {
        let Some(z) = text[a + 2..].find("]]").map(|i| a + 2 + i) else { break };
        if text[a + 2..z].trim().to_lowercase() == want { text.replace_range(a + 2..z, new); n += 1; from = a + 2 + new.len() + 2; } else { from = a + 2; }
    }
    n
}

/// Remove the parts of strokes within `r` of `p`, splitting strokes that are cut in the middle.
pub fn erase_partial(strokes: &mut Vec<Stroke>, p: [f32; 2], r: f32) -> bool {
    let mut changed = false;
    let mut out = Vec::with_capacity(strokes.len());
    for s in strokes.drain(..) {
        let reach = r + s.width * 0.5;
        if !stroke_hit(&s, p, reach) { out.push(s); continue; }
        changed = true;
        let hit = |q: &[f32; 3]| (q[0] - p[0]).hypot(q[1] - p[1]) < reach;
        let mut run: Vec<[f32; 3]> = vec![];
        for q in &densify(&s.points, (reach * 0.5).max(1.0)) {
            if hit(q) {
                if run.len() > 1 { out.push(Stroke { id: new_id(), points: std::mem::take(&mut run), ..s.clone() }); }
                run.clear();
            } else { run.push(*q); }
        }
        if run.len() > 1 { out.push(Stroke { id: new_id(), points: run, ..s }); }
    }
    *strokes = out;
    changed
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape { Line, Arrow, Rect, Ellipse }

/// Points for a shape dragged from `a` to `b`.
pub fn shape_points(shape: Shape, a: [f32; 2], b: [f32; 2]) -> Vec<[f32; 3]> {
    let p = |x: f32, y: f32| [x, y, 1.0];
    match shape {
        Shape::Line => vec![p(a[0], a[1]), p(b[0], b[1])],
        Shape::Arrow => {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = dx.hypot(dy).max(1.0);
            let (ux, uy) = (dx / len, dy / len);
            let h = (len * 0.25).clamp(8.0, 22.0);
            let wing = |s: f32| p(b[0] - h * ux + s * h * 0.55 * uy, b[1] - h * uy - s * h * 0.55 * ux);
            vec![p(a[0], a[1]), p(b[0], b[1]), wing(1.0), p(b[0], b[1]), wing(-1.0)]
        }
        Shape::Rect => vec![p(a[0], a[1]), p(b[0], a[1]), p(b[0], b[1]), p(a[0], b[1]), p(a[0], a[1])],
        Shape::Ellipse => {
            let (cx, cy, rx, ry) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (b[0] - a[0]).abs() / 2.0, (b[1] - a[1]).abs() / 2.0);
            (0..=48).map(|i| { let t = i as f32 / 48.0 * std::f32::consts::TAU; p(cx + rx * t.cos(), cy + ry * t.sin()) }).collect()
        }
    }
}

/// A flashcard made from notes.
#[derive(Clone, Debug, PartialEq)]
pub struct Card { pub key: String, pub front: String, pub back: String }

impl Page {
    /// The study cards of a deck page (progress is keyed by each card's id, so editing a card keeps it).
    pub fn deck(&self) -> Vec<Card> {
        if self.kind != PageKind::Deck { return vec![]; }
        self.cards.iter().filter(|c| !c.front.trim().is_empty() && !c.back.trim().is_empty())
            .map(|c| Card { key: format!("card{}", c.id), front: c.front.clone(), back: c.back.clone() }).collect()
    }
}

/// Days until a card comes back, by Leitner box.
pub const STUDY_DAYS: [u64; 6] = [0, 1, 3, 7, 16, 35];

fn push_card(cards: &mut Vec<Card>, f: &str, b: &str) {
    let (f, b) = (f.trim().to_string(), b.trim().to_string());
    if !f.is_empty() && !b.is_empty() && !cards.iter().any(|c| c.front == f) { cards.push(Card { key: card_key(&f), front: f, back: b.chars().take(600).collect() }); }
}

/// The progress key of a card written inside notes: a hash of its question, the same on every device
/// (the question itself would stay readable in the library after its section is locked).
pub fn card_key(front: &str) -> String {
    format!("q{:016x}", front.bytes().fold(0xcbf29ce484222325u64, |h, x| (h ^ x as u64).wrapping_mul(0x100000001b3)))
}

fn is_card_key(k: &str) -> bool {
    k.strip_prefix("card").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        || k.strip_prefix('q').is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Progress saved by older versions under the question text: moved to the hashed key.
pub fn rekey_study(s: &mut Store) {
    let old: Vec<String> = s.study.keys().filter(|k| !is_card_key(k)).cloned().collect();
    for k in old {
        if let Some(v) = s.study.remove(&k) { s.study.entry(card_key(k.trim())).or_insert(v); }
    }
}

/// Suggest flashcards from a notes page, offline, from the ways people already write notes:
/// `term :: definition`, `Q:`/`A:` pairs, `**term**: definition`, ==highlights== (fill in the blank),
/// headings with what follows them (text or equations), and table rows.
pub fn flashcards(page: &Page) -> Vec<Card> {
    let mut cards: Vec<Card> = vec![];
    let mut blocks: Vec<&Block> = page.blocks.iter().collect();
    blocks.sort_by(|a, b| a.pos[1].total_cmp(&b.pos[1]).then(a.pos[0].total_cmp(&b.pos[0])));
    // heading → whatever follows until the next heading
    let mut heading: Option<(String, String)> = None;
    for b in blocks {
        match &b.kind {
            Kind::Text { text, heading: hl } => {
                if *hl > 0 {
                    if let Some((f, bk)) = heading.take() { push_card(&mut cards, &f, &bk); }
                    heading = Some((text.trim().to_string(), String::new()));
                    continue;
                }
                let mut lines = text.lines().peekable();
                while let Some(raw) = lines.next() {
                    let (kind, plen) = crate::rich::classify(raw);
                    let line = raw[plen..].trim();
                    if let crate::rich::LineKind::Heading(_) = kind {
                        if let Some((f, bk)) = heading.take() { push_card(&mut cards, &f, &bk); }
                        heading = Some((line.to_string(), String::new()));
                        continue;
                    }
                    if let Some((_, body)) = &mut heading { body.push_str(raw); body.push('\n'); }
                    if let Some((f, bk)) = line.split_once("::") { push_card(&mut cards, f, bk); continue; }
                    if let Some(q) = line.strip_prefix("Q:") {
                        if let Some(a) = lines.peek().and_then(|n| n.trim().strip_prefix("A:")) { push_card(&mut cards, q, a); lines.next(); continue; }
                    }
                    if let Some(rest) = line.strip_prefix("**") {
                        if let Some((term, def)) = rest.split_once("**") {
                            if def.trim_start().starts_with([':', '—', '-']) {
                                let def = def.trim_start().trim_start_matches([':', '—', '-']);
                                if !def.trim().is_empty() { push_card(&mut cards, term, def); continue; }
                            }
                        }
                    }
                    // each ==highlight== becomes a fill-in-the-blank card
                    let marks: Vec<&str> = line.split("==").collect();
                    if marks.len() >= 3 {
                        for k in (1..marks.len() - 1).step_by(2) {
                            let front: String = marks.iter().enumerate().map(|(j, m)| if j == k { "______".to_string() } else { m.to_string() }).collect();
                            // the answer side shows the whole sentence with the answer in bold, not the word alone
                            let back: String = marks.iter().enumerate().map(|(j, m)| if j == k { format!("**{m}**") } else { m.to_string() }).collect();
                            push_card(&mut cards, &front, &back);
                        }
                    }
                }
            }
            Kind::Math { src } => { if let Some((_, body)) = &mut heading { body.push_str(&format!("$${src}$$\n")); } }
            Kind::Table { rows } if rows.len() > 1 && rows[0].len() > 1 => {
                for r in &rows[1..] {
                    let back: Vec<String> = rows[0].iter().zip(r).skip(1).filter(|(_, v)| !v.trim().is_empty()).map(|(h, v)| format!("**{h}**: {v}")).collect();
                    push_card(&mut cards, &format!("{}: {}", rows[0][0], r[0]), &back.join("\n"));
                }
            }
            other => { if let Some((_, body)) = &mut heading { body.push_str(&other.summary()); body.push('\n'); } }
        }
    }
    if let Some((f, bk)) = heading.take() { push_card(&mut cards, &f, &bk); }
    cards
}

/// Scratch-out: if `pts` is a scribble (a tight back-and-forth zigzag), the ids of the strokes it
/// covers, which it erases. Empty when it isn't a scribble or covers nothing (then it stays ink).
pub fn scratched(pts: &[[f32; 3]], strokes: &[Stroke]) -> Vec<u64> {
    if pts.len() < 12 { return vec![]; }
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in pts { for k in 0..2 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
    let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
    let axis = if w >= h { 0 } else { 1 };
    let path: f32 = pts.windows(2).map(|q| (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1])).sum();
    // direction changes along the long side, ignoring jitter under 4 points
    let (mut turns, mut dir, mut run) = (0, 0.0f32, 0.0f32);
    for q in pts.windows(2) {
        let d = q[1][axis] - q[0][axis];
        if d * dir < 0.0 { run += d.abs(); if run > 4.0 { turns += 1; dir = d.signum(); run = 0.0; } }
        else { if dir == 0.0 { dir = d.signum(); } run = 0.0; }
    }
    if turns < 4 || path < 3.5 * w.max(h) || w.max(h) < 12.0 { return vec![]; }
    let inside = |p: &[f32; 3]| p[0] >= lo[0] - 4.0 && p[0] <= hi[0] + 4.0 && p[1] >= lo[1] - 4.0 && p[1] <= hi[1] + 4.0;
    // the scribble must actually run over a stroke (not just near it, as a zigzag arrow or cursive next to it does)
    let scribble = Stroke { id: 0, points: pts.to_vec(), color: [0; 4], width: 0.0, highlighter: false, pen: PenKind::Ballpoint, t: 0.0 };
    let covered = |s: &Stroke| s.points.iter().filter(|q| stroke_hit(&scribble, [q[0], q[1]], 6.0 + s.width)).count() * 10 >= s.points.len() * 5;
    strokes.iter().filter(|s| !s.points.is_empty() && s.points.iter().filter(|p| inside(p)).count() * 10 >= s.points.len() * 6 && covered(s)).map(|s| s.id).collect()
}

/// The end of a line from `a` to `b`, made exactly level or upright when it's within 4° of that
/// (underlines and axes drawn by hand come out slightly slanted; tablets have no Shift key).
pub fn level(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let tilt = 4f32.to_radians().tan();
    if dy.abs() <= dx.abs() * tilt { [b[0], a[1]] } else if dx.abs() <= dy.abs() * tilt { [a[0], b[1]] } else { b }
}

/// Recognise a hand-drawn shape and return a perfect version: a straight line, a circle/ellipse,
/// a box, or a polygon (triangle, etc). None if it doesn't look like any of these.
pub fn snap_shape(pts: &[[f32; 3]]) -> Option<(Vec<[f32; 3]>, &'static str)> {
    if pts.len() < 6 { return None; }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in pts { x0 = x0.min(p[0]); y0 = y0.min(p[1]); x1 = x1.max(p[0]); y1 = y1.max(p[1]); }
    let (w, h) = (x1 - x0, y1 - y0);
    let diag = w.hypot(h);
    if diag < 14.0 { return None; }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let dist = |p: [f32; 3], q: [f32; 3]| (p[0] - q[0]).hypot(p[1] - q[1]);
    let flat = |p: (f32, f32)| [p.0, p.1, 0.85];
    if dist(a, b) > 0.25 * diag {
        // open: a straight line if nothing strays far from the chord
        let len = dist(a, b);
        let dev = pts.iter().map(|q| ((b[1] - a[1]) * q[0] - (b[0] - a[0]) * q[1] + b[0] * a[1] - b[1] * a[0]).abs() / len).fold(0.0, f32::max);
        let e = level([a[0], a[1]], [b[0], b[1]]);
        return (dev < 0.08 * len).then(|| (vec![flat((a[0], a[1])), flat((e[0], e[1]))], "line"));
    }
    // closed: how round is it? (0 = perfect ellipse through the bounding box)
    let (cx, cy, rx, ry) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (w / 2.0).max(1.0), (h / 2.0).max(1.0));
    let err = pts.iter().map(|p| ((((p[0] - cx) / rx).powi(2) + ((p[1] - cy) / ry).powi(2)).sqrt() - 1.0).abs()).sum::<f32>() / pts.len() as f32;
    let ellipse = || {
        let (rx, ry) = if (rx - ry).abs() < 0.15 * rx.max(ry) { let r = (rx + ry) / 2.0; (r, r) } else { (rx, ry) };
        let kind = if rx == ry { "circle" } else { "ellipse" };
        ((0..=64).map(|i| { let t = i as f32 / 64.0 * std::f32::consts::TAU; flat((cx + rx * t.cos(), cy + ry * t.sin())) }).collect(), kind)
    };
    if err < 0.09 { return Some(ellipse()); }
    // corners: simplify the outline, then drop points where it barely turns
    let mut poly = rdp(&pts.iter().map(|p| (p[0], p[1])).collect::<Vec<_>>(), 0.07 * diag);
    if poly.len() > 1 && (poly[0].0 - poly[poly.len() - 1].0).hypot(poly[0].1 - poly[poly.len() - 1].1) < 0.25 * diag { poly.pop(); }
    loop {
        let n = poly.len();
        if n < 3 { break; }
        let turn = |i: usize| {
            let (p, q, r) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
            let (a1, a2) = ((q.1 - p.1).atan2(q.0 - p.0), (r.1 - q.1).atan2(r.0 - q.0));
            let mut d = (a2 - a1).abs();
            if d > std::f32::consts::PI { d = std::f32::consts::TAU - d; }
            d
        };
        match (0..n).find(|&i| turn(i) < 0.45) { Some(i) => { poly.remove(i); } None => break }
    }
    match poly.len() {
        4 => {
            // nearly square-on boxes become exact boxes
            let axis = (0..4).all(|i| { let (p, q) = (poly[i], poly[(i + 1) % 4]); let ang = (q.1 - p.1).atan2(q.0 - p.0).abs() % std::f32::consts::FRAC_PI_2; ang < 0.26 || ang > std::f32::consts::FRAC_PI_2 - 0.26 });
            if axis { return Some((vec![flat((x0, y0)), flat((x1, y0)), flat((x1, y1)), flat((x0, y1)), flat((x0, y0))], "box")); }
            let mut v: Vec<[f32; 3]> = poly.iter().map(|&p| flat(p)).collect();
            v.push(v[0]);
            Some((v, "four-sided shape"))
        }
        3 | 5 | 6 => {
            let name = match poly.len() { 3 => "triangle", 5 => "pentagon", _ => "hexagon" };
            let mut v: Vec<[f32; 3]> = poly.iter().map(|&p| flat(p)).collect();
            v.push(v[0]);
            Some((v, name))
        }
        _ if err < 0.16 => Some(ellipse()),
        _ => None,
    }
}

/// Ramer–Douglas–Peucker line simplification.
fn rdp(p: &[(f32, f32)], eps: f32) -> Vec<(f32, f32)> {
    if p.len() < 3 { return p.to_vec(); }
    let (a, b) = (p[0], p[p.len() - 1]);
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let d = |q: (f32, f32)| if len < 1e-2 { (q.0 - a.0).hypot(q.1 - a.1) } else { ((b.1 - a.1) * q.0 - (b.0 - a.0) * q.1 + b.0 * a.1 - b.1 * a.0).abs() / len };
    let (i, dmax) = p.iter().enumerate().skip(1).take(p.len() - 2).map(|(i, &q)| (i, d(q))).fold((0, 0.0), |m, x| if x.1 > m.1 { x } else { m });
    if dmax > eps {
        let mut left = rdp(&p[..=i], eps);
        left.pop();
        left.extend(rdp(&p[i..], eps));
        left
    } else { vec![a, b] }
}

/// A new, empty flashcard deck page.
pub fn deck_page(title: &str) -> Page {
    Page { kind: PageKind::Deck, cards: vec![DeckCard::new("", "")], ..Page::new(title) }
}

/// Render a PDF's pages to PNG files next to it; returns the file names, one per line
/// (plus "…N" when the document had more than `max` pages).
pub fn print_pdf(pdf: &std::path::Path, max: usize) -> Result<String, String> {
    use hayro::{hayro_interpret::InterpreterSettings, hayro_syntax::Pdf, render, PixmapSettings, RenderCache, RenderSettings};
    let doc = Pdf::new(std::fs::read(pdf).map_err(|e| e.to_string())?).map_err(|e| format!("This PDF can't be read ({e:?})"))?;
    let (cache, settings) = (RenderCache::new(), InterpreterSettings::default());
    let pix = PixmapSettings { x_scale: 1.6, y_scale: 1.6, bg_color: hayro::vello_cpu::color::palette::css::WHITE };
    let stem = pdf.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut names = vec![];
    for (i, page) in doc.pages().iter().take(max).enumerate() {
        let png = render(page, &cache, &settings, &RenderSettings::default(), &pix).into_png().map_err(|e| format!("{e:?}"))?;
        let name = format!("{stem}-page{}.png", i + 1);
        std::fs::write(pdf.with_file_name(&name), png).map_err(|e| e.to_string())?;
        names.push(name);
    }
    if names.is_empty() { return Err("This PDF has no pages.".into()); }
    if doc.pages().len() > max { names.push(format!("…{}", doc.pages().len())); }
    Ok(names.join("\n"))
}

pub fn block(x: f32, y: f32, w: f32, kind: Kind) -> Block { Block { id: new_id(), pos: [x, y], width: w, tag: Tag::None, kind, t: now_f() } }

fn text(s: &str) -> Kind { Kind::Text { text: s.into(), heading: 0 } }

/// First-run content so the app opens on something useful.
pub fn sample() -> Store {
    let mut p = Page::new("Signals & Systems — Week 3");
    p.created = 1_793_700_000;
    p.paper = Paper::Ruled;
    p.blocks = vec![
        block(24.0, 8.0, 336.0, Kind::Text { text: "Fourier series".into(), heading: 1 }),
        block(24.0, 50.0, 336.0, text(r"Any **periodic** signal is a sum of sines and cosines. The ==coefficients== say how much of each frequency $n\omega$ is present.")),
        block(24.0, 128.0, 336.0, text("- Even functions → only cosines\n- Odd functions → only sines\n- Jumps cause the *Gibbs* overshoot\n- Worked through in [[Worked example: square wave]]")),
        block(24.0, 270.0, 336.0, Kind::Math { src: r"f(t) = \frac{a_0}{2} + \sum_{n=1}^{\infty} \left( a_n \cos(n \omega t) + b_n \sin(n \omega t) \right)".into() }),
        Block { tag: Tag::Important, ..block(24.0, 356.0, 336.0, Kind::Math { src: r"a_n = \frac{2}{T}\int_0^T f(t)\cos(n\omega t)\,dt".into() }) },
        block(24.0, 426.0, 336.0, Kind::Code { lang: "python".into(), code: "import numpy as np\nt = np.linspace(0, 1, 500)\nf = sum(np.sin(2*np.pi*(2*k+1)*t)/(2*k+1) for k in range(10))".into() }),
        Block { tag: Tag::None, ..block(376.0, 8.0, 232.0, Kind::Checklist { items: vec![(true, "Read chapter 3.2".into()), (false, "Problem set 4, ex. 1–6".into()), (false, "Ask about Gibbs overshoot".into())] }) },
        block(376.0, 190.0, 232.0, Kind::Table { rows: vec![vec!["Signal".into(), "Period".into(), "ω".into()], vec!["Square".into(), "2π".into(), "1".into()], vec!["Sawtooth".into(), "π".into(), "2".into()]] }),
    ];
    // handwriting: a sketched wave, a highlight and an underline
    let wave: Vec<[f32; 3]> = (0..54).map(|i| { let x = i as f32 * 4.0; [386.0 + x, 352.0 - 22.0 * (x / 18.0).sin(), 0.55 + 0.45 * (x / 40.0).sin().abs()] }).collect();
    p.strokes.push(Stroke { id: new_id(), points: wave, color: [37, 99, 235, 255], width: 3.0, highlighter: false, pen: PenKind::Fountain, t: 0.0 });
    p.strokes.push(Stroke { id: new_id(), points: vec![[376.0, 404.0, 1.0], [606.0, 404.0, 1.0]], color: [253, 224, 71, 120], width: 16.0, highlighter: true, pen: PenKind::Fountain, t: 0.0 });
    p.strokes.push(Stroke { id: new_id(), points: (0..34).map(|i| [386.0 + i as f32 * 3.0, 452.0 + (i as f32 * 0.7).sin() * 3.0, 1.0]).collect(), color: [220, 38, 38, 255], width: 2.5, highlighter: false, pen: PenKind::Fountain, t: 0.0 });
    p.blocks.push(block(376.0, 388.0, 214.0, text("← odd harmonics build a square wave")));

    let mut sub = Page::new("Worked example: square wave");
    sub.level = 1;
    sub.blocks = vec![
        block(24.0, 8.0, 500.0, Kind::Math { src: r"x(t) = \frac{4}{\pi}\sum_{k=0}^{\infty}\frac{\sin((2k+1)t)}{2k+1}".into() }),
        block(24.0, 100.0, 500.0, Kind::Math { src: r"R(\theta) = \begin{pmatrix} \cos\theta & -\sin\theta \\ \sin\theta & \cos\theta \end{pmatrix}".into() }),
        block(24.0, 200.0, 500.0, Kind::Math { src: r"|x| = \begin{cases} x & x \ge 0 \\ -x & x < 0 \end{cases}".into() }),

    ];
    let mut deck = Page::new("Fourier — flashcards");
    deck.kind = PageKind::Deck;
    deck.cards = vec![
        DeckCard::new("What is a harmonic?", "A whole-number multiple of the base frequency"),
        DeckCard::new("Which harmonics build a square wave?", "Only the **odd** ones"),
        DeckCard::new("Fourier coefficient $a_n$", r"$a_n = \frac{2}{T}\int_0^T f(t)\cos(n\omega t)\,dt$"),
        DeckCard::new("What is the Gibbs phenomenon?", "The overshoot near a jump that never goes away as you add terms"),
    ];
    let lect = Section { id: new_id(), name: "Lectures".into(), color: PALETTE[0], pages: vec![p, sub, deck, Page::new("Week 2 — Convolution"), Page::new("Week 1 — Intro")], lock: None };
    let mut idea = Page::new("App ideas");
    idea.blocks = vec![block(24.0, 8.0, 400.0, text("Sketch the onboarding flow with the pen, then convert it."))];
    let ideas = Section { id: new_id(), name: "Ideas".into(), color: PALETTE[2], pages: vec![idea], lock: None };
    let mut q = Page::new("Quick notes");
    q.blocks = vec![block(24.0, 8.0, 400.0, text("Tap anywhere on the page to type."))];
    let quick = Section { id: new_id(), name: "Quick Notes".into(), color: PALETTE[4], pages: vec![q], lock: None };
    let study = Notebook { id: new_id(), name: "Study".into(), color: PALETTE[0], sections: vec![lect, ideas, quick] };
    let mut pl = Page::new("Groceries");
    pl.blocks = vec![block(24.0, 8.0, 300.0, Kind::Checklist { items: vec![(false, "Oats".into()), (false, "Lemons".into())] })];
    let personal = Notebook { id: new_id(), name: "Personal".into(), color: PALETTE[3], sections: vec![Section { id: new_id(), name: "Home".into(), color: PALETTE[3], pages: vec![pl], lock: None }] };
    Store { notebooks: vec![study, personal], trash: vec![], study: Default::default() }
}

/// Attachment names come from other devices: only plain names, never paths.
pub fn safe_name(n: &str) -> bool {
    !n.is_empty() && n.len() < 200 && !n.starts_with('.') && n.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// An attachment in the files folder. Names that aren't plain (`../x`, from a synced note) never
/// leave the folder: they name a file that doesn't exist.
pub fn files_path(name: &str) -> PathBuf {
    data_dir().join("files").join(if safe_name(name) { name } else { "invalid-name" })
}

pub fn data_dir() -> PathBuf {
    // a different folder on request (portable use, tests)
    if let Some(d) = std::env::var_os("LEAFNOTES_DATA").filter(|d| !d.is_empty()) { return PathBuf::from(d); }
    #[cfg(target_os = "android")]
    { return crate::ANDROID_DATA.get().cloned().unwrap_or_else(|| PathBuf::from("/data/local/tmp/leafnotes")); }
    #[allow(unreachable_code)]
    {
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        };
        base.unwrap_or_else(|| PathBuf::from(".")).join("LeafNotes")
    }
}

pub fn load<T: for<'de> Deserialize<'de>>(name: &str) -> Option<T> {
    serde_json::from_slice(&std::fs::read(data_dir().join(name)).ok()?).ok()
}

/// Write via a temp file and rename so a crash mid-save never truncates the notebook file.
pub fn save<T: Serialize>(name: &str, value: &T) -> std::io::Result<()> {
    static BACKED_UP: std::sync::Once = std::sync::Once::new();
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    // Keep the version from before this session, so even a bad session can be rolled back by hand.
    if name.starts_with("notebooks") { BACKED_UP.call_once(|| { let _ = std::fs::copy(dir.join(name), dir.join(format!("{name}.backup"))); }); }
    crate::storage::write_atomic(&dir.join(name), &serde_json::to_vec(value)?)
}

/// Draw strokes onto a white PNG for handwriting recognition.
pub fn rasterize(strokes: &[&Stroke]) -> Option<Vec<u8>> {
    let pts = strokes.iter().filter(|s| !s.highlighter).flat_map(|s| s.points.iter());
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in pts { x0 = x0.min(p[0]); y0 = y0.min(p[1]); x1 = x1.max(p[0]); y1 = y1.max(p[1]); }
    if x0 > x1 { return None; }
    let m = 24.0;
    let scale = (1024.0 / (x1 - x0 + 2.0 * m).max(y1 - y0 + 2.0 * m)).min(2.0);
    let (w, h) = (((x1 - x0 + 2.0 * m) * scale) as u32 + 1, ((y1 - y0 + 2.0 * m) * scale) as u32 + 1);
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
    for s in strokes.iter().filter(|s| !s.highlighter && !s.points.is_empty()) {
        let r = (s.width * scale * 0.5).max(1.2);
        let map = |p: &[f32; 3]| ((p[0] - x0 + m) * scale, (p[1] - y0 + m) * scale);
        let mut stamp = |cx: f32, cy: f32| {
            let ri = r.ceil() as i32;
            for dy in -ri..=ri { for dx in -ri..=ri {
                if (dx * dx + dy * dy) as f32 <= r * r {
                    let (px, py) = (cx as i32 + dx, cy as i32 + dy);
                    if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h { img.put_pixel(px as u32, py as u32, image::Rgba([0, 0, 0, 255])); }
                }
            }}
        };
        let first = map(&s.points[0]);
        stamp(first.0, first.1);
        for seg in s.points.windows(2) {
            let (a, b) = (map(&seg[0]), map(&seg[1]));
            let steps = ((b.0 - a.0).hypot(b.1 - a.1) / 0.7).ceil().max(1.0) as i32;
            for i in 1..=steps { let t = i as f32 / steps as f32; stamp(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t); }
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Move the page at `from` (with the subpages under it) to where the page at `to` is now, before it.
/// Dropping a page onto its own subpage changes nothing.
pub fn move_pages(pages: &mut Vec<Page>, from: usize, to: usize) {
    if from >= pages.len() { return; }
    let lvl = pages[from].level;
    let end = from + 1 + pages[from + 1..].iter().take_while(|p| p.level > lvl).count();
    if (from..end).contains(&to) { return; }
    let run: Vec<Page> = pages.drain(from..end).collect();
    let at = if to > from { to - run.len() } else { to }.min(pages.len());
    for (k, p) in run.into_iter().enumerate() { pages.insert(at + k, p); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_card_progress_moves_to_hashed_keys() {
        let mut s = Store { notebooks: vec![], trash: vec![], study: Default::default() };
        let st = CardState { bx: 3, due: 9 };
        s.study.insert("What is a pole?".into(), st);
        s.study.insert("card42".into(), st);
        rekey_study(&mut s);
        assert_eq!(s.study.get(&card_key("What is a pole?")), Some(&st));
        assert!(s.study.contains_key("card42") && !s.study.contains_key("What is a pole?") && s.study.len() == 2);
        rekey_study(&mut s);
        assert_eq!(s.study.len(), 2, "already converted keys stay");
    }

    #[test]
    fn the_eraser_finds_the_middle_of_a_ruler_line() {
        let line = Stroke { id: 1, points: vec![[0.0, 0.0, 1.0], [200.0, 0.0, 1.0]], color: [0, 0, 0, 255], width: 2.0, highlighter: false, pen: PenKind::Ballpoint, t: 0.0 };
        assert!(stroke_hit(&line, [100.0, 3.0], 5.0) && !stroke_hit(&line, [100.0, 30.0], 5.0));
        let mut v = vec![line];
        assert!(erase_partial(&mut v, [100.0, 0.0], 5.0));
        assert_eq!(v.len(), 2, "cut in two at the middle");
        assert!(v.iter().all(|s| s.points.iter().all(|p| (p[0] - 100.0).abs() > 4.0)));
        assert!(densify(&[[0.0, 0.0, 1.0], [60.0, 0.0, 1.0]], 6.0).len() >= 11);
    }
    #[test]
    fn dragged_pages_land_where_the_line_shows_with_their_subpages() {
        let mk = |t: &str, level: u8| Page { level, ..Page::new(t) };
        let names = |v: &Vec<Page>| v.iter().map(|p| p.title.as_str()).collect::<Vec<_>>().join(" ");
        let mut v = vec![mk("A", 0), mk("a1", 1), mk("B", 0), mk("C", 0)];
        move_pages(&mut v, 0, 3); // A (and a1) dropped on the line above C
        assert_eq!(names(&v), "B A a1 C");
        move_pages(&mut v, 3, 0); // C up to the top
        assert_eq!(names(&v), "C B A a1");
        move_pages(&mut v, 2, 3); // A onto its own subpage: nothing
        assert_eq!(names(&v), "C B A a1");
        move_pages(&mut v, 0, 4); // C below the last page
        assert_eq!(names(&v), "B A a1 C");
    }

    #[test]
    fn cards_from_pasted_text() {
        let c = cards_from_text("- cell :: the unit of life\nATP\tenergy\nQ: 2+2?\nA: 4\njunk line");
        assert_eq!(c, vec![("cell".into(), "the unit of life".into()), ("ATP".into(), "energy".into()), ("2+2?".into(), "4".into())]);
    }
    #[test]
    fn links_follow_a_renamed_page() {
        let mut t = String::from("see [[Week 3]] and [[week 3]], not [[Week 30]] [[x");
        assert_eq!(relink(&mut t, "Week 3", "Fourier"), 2);
        assert_eq!(t, "see [[Fourier]] and [[Fourier]], not [[Week 30]] [[x");
    }
    #[test]
    fn an_unclosed_equation_stays_text() {
        let k = blocks_from_markdown("$$ x^2\nline two\nline three");
        assert!(!k.iter().any(|b| matches!(b, Kind::Math { .. })), "nothing swallowed");
        let k = blocks_from_markdown("$$\nx^2\n$$\nafter");
        assert!(matches!(&k[0], Kind::Math { src } if src == "x^2"));
        assert!(!files_path("../../etc/passwd").to_string_lossy().contains("etc"));
    }
    #[test]
    fn scratch_out() {
        let line = |id, pts: Vec<[f32; 3]>| Stroke { id, points: pts, color: [0, 0, 0, 255], width: 2.0, highlighter: false, pen: PenKind::Ballpoint, t: 0.0 };
        let word = line(1, (0..20).map(|i| [100.0 + i as f32 * 3.0, 50.0, 1.0]).collect());
        let far = line(2, (0..20).map(|i| [400.0 + i as f32 * 3.0, 300.0, 1.0]).collect());
        // a back-and-forth scribble across the word
        let zag: Vec<[f32; 3]> = (0..48).map(|i| [if i % 2 == 0 { 95.0 } else { 165.0 }, 42.0 + i as f32 * 0.35, 1.0]).collect();
        assert_eq!(scratched(&zag, &[word.clone(), far.clone()]), vec![1]);
        // a plain stroke over the word is not a scribble
        let plain: Vec<[f32; 3]> = (0..30).map(|i| [90.0 + i as f32 * 3.0, 50.0 + i as f32, 1.0]).collect();
        assert!(scratched(&plain, &[word.clone(), far]).is_empty());
        // a scribble over nothing erases nothing
        let alone: Vec<[f32; 3]> = zag.iter().map(|p| [p[0] + 600.0, p[1] + 600.0, 1.0]).collect();
        assert!(scratched(&alone, &[word]).is_empty());
    }
    #[test]
    fn rasterizes_and_roundtrips() {
        let s = Stroke { id: 1, points: vec![[0.0, 0.0, 1.0], [50.0, 30.0, 1.0]], color: [0, 0, 0, 255], width: 3.0, highlighter: false, pen: PenKind::Fountain, t: 0.0 };
        let png = rasterize(&[&s]).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let store = sample();
        let back: Store = serde_json::from_str(&serde_json::to_string(&store).unwrap()).unwrap();
        assert_eq!(back.notebooks.len(), store.notebooks.len());
    }
    #[test]
    fn deck_pages() {
        let d = deck_page("Deck");
        assert_eq!((d.kind, d.cards.len()), (PageKind::Deck, 1));
        assert!(d.deck().is_empty(), "empty cards aren't studied");
        let mut d = d;
        d.cards[0].front = "Q".into();
        d.cards[0].back = "A".into();
        let c = d.deck();
        assert_eq!((c.len(), c[0].key.starts_with("card")), (1, true));
        assert!(d.plain_text().contains("Q :: A"));
    }
    #[test]
    fn makes_flashcards() {
        let mut p = Page::new("t");
        p.blocks = vec![
            block(0.0, 0.0, 300.0, Kind::Text { text: "Fourier".into(), heading: 1 }),
            block(0.0, 40.0, 300.0, Kind::Text { text: "Sums of sines.\nOhm's law :: V = IR\nQ: Capital of France?\nA: Paris\n**Entropy**: disorder\nThe ==mitochondria== is the powerhouse".into(), heading: 0 }),
            block(0.0, 200.0, 300.0, Kind::Math { src: "x^2".into() }),
            block(0.0, 300.0, 300.0, Kind::Table { rows: vec![vec!["Signal".into(), "Period".into()], vec!["Square".into(), "2π".into()]] }),
        ];
        let c = flashcards(&p);
        let has = |f: &str, b: &str| c.iter().any(|x| x.front == f && x.back.contains(b));
        assert!(has("Ohm's law", "V = IR"), "{c:?}");
        assert!(has("Capital of France?", "Paris"), "{c:?}");
        assert!(has("Entropy", "disorder"), "{c:?}");
        assert!(has("The ______ is the powerhouse", "mitochondria"), "{c:?}");
        assert!(has("Signal: Square", "**Period**: 2π"), "{c:?}");
        assert!(has("Fourier", "$$x^2$$"), "{c:?}");
    }
    #[test]
    fn snaps_shapes() {
        let wobble = |i: usize| (i as f32 * 1.7).sin() * 1.5;
        let circle: Vec<[f32; 3]> = (0..=60).map(|i| { let t = i as f32 / 60.0 * std::f32::consts::TAU; [100.0 + (50.0 + wobble(i)) * t.cos(), 100.0 + (50.0 + wobble(i)) * t.sin(), 1.0] }).collect();
        assert_eq!(snap_shape(&circle).unwrap().1, "circle");
        let mut sq = vec![];
        for (a, b) in [((0.0, 0.0), (100.0, 0.0)), ((100.0, 0.0), (100.0, 100.0)), ((100.0, 100.0), (0.0, 100.0)), ((0.0, 100.0), (0.0, 2.0))] {
            for i in 0..20 { let t = i as f32 / 20.0; sq.push([a.0 + (b.0 - a.0) * t + wobble(i), a.1 + (b.1 - a.1) * t, 1.0]); }
        }
        assert_eq!(snap_shape(&sq).unwrap().1, "box");
        let mut tri = vec![];
        for (a, b) in [((0.0, 100.0), (50.0, 0.0)), ((50.0, 0.0), (100.0, 100.0)), ((100.0, 100.0), (2.0, 99.0))] {
            for i in 0..20 { let t = i as f32 / 20.0; tri.push([a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t + wobble(i), 1.0]); }
        }
        assert_eq!(snap_shape(&tri).unwrap().1, "triangle");
        let line: Vec<[f32; 3]> = (0..30).map(|i| [i as f32 * 5.0, 10.0 + wobble(i), 1.0]).collect();
        let (pts, kind) = snap_shape(&line).unwrap();
        assert_eq!((kind, pts.len()), ("line", 2));
        let scribble: Vec<[f32; 3]> = (0..40).map(|i| [i as f32 * 3.0, (i as f32 * 0.9).sin() * 30.0, 1.0]).collect();
        assert!(snap_shape(&scribble).is_none());
    }
    #[test]
    fn prints_pdf_pages() {
        // a one-page PDF with a filled square, written by hand (offsets computed below)
        let content = "0 0 1 rg 100 100 200 200 re f";
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
        let dir = std::env::temp_dir().join(format!("leafnotes-pdf-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.pdf");
        std::fs::write(&path, pdf).unwrap();
        let names = print_pdf(&path, 60).unwrap();
        assert_eq!(names, "doc-page1.png");
        let img = image::open(dir.join("doc-page1.png")).unwrap().to_rgba8();
        assert_eq!((img.width(), img.height()), (640, 640));
        assert_eq!(img.get_pixel(320, 320).0, [0, 0, 255, 255], "square drawn blue");
        assert_eq!(img.get_pixel(20, 20).0, [255, 255, 255, 255], "paper is white");
        let _ = std::fs::remove_dir_all(dir);
    }
    /// Manual check with a real document: LEAFNOTES_PDF=/path/file.pdf cargo test --release -- --ignored real_pdf --nocapture
    #[test]
    #[ignore]
    fn real_pdf() {
        let Ok(p) = std::env::var("LEAFNOTES_PDF") else { return };
        let t = std::time::Instant::now();
        let r = print_pdf(std::path::Path::new(&p), 3);
        println!("{r:?} in {:?}", t.elapsed());
    }
    #[test]
    fn markdown_to_blocks_and_erasing() {
        let k = blocks_from_markdown("Intro **bold**\n\n$$x^2$$\n- [ ] a\n- [x] b\n| A | B |\n|---|---|\n| 1 | 2 |\n```rust\nfn x(){}\n```\nEnd");
        assert!(matches!(&k[0], Kind::Text { text, .. } if text == "Intro **bold**"));
        assert!(matches!(&k[1], Kind::Math { src } if src == "x^2"));
        assert!(matches!(&k[2], Kind::Checklist { items } if items.len() == 2 && items[1].0));
        assert!(matches!(&k[3], Kind::Table { rows } if rows.len() == 2));
        assert!(matches!(&k[4], Kind::Code { lang, .. } if lang == "rust"));
        assert!(matches!(&k[5], Kind::Text { text, .. } if text == "End"));
        let mut s = vec![Stroke { id: 1, points: (0..11).map(|i| [i as f32 * 10.0, 0.0, 1.0]).collect(), color: [0; 4], width: 2.0, highlighter: false, pen: PenKind::Fountain, t: 0.0 }];
        assert!(erase_partial(&mut s, [50.0, 0.0], 4.0));
        assert_eq!(s.len(), 2);
        assert_eq!(shape_points(Shape::Rect, [0.0, 0.0], [10.0, 5.0]).len(), 5);
    }
}
