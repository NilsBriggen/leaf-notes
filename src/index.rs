//! Search inside handwriting and PDFs. A background job reads each page's ink with the offline
//! recognizer and the text of PDFs attached to it; search looks there too. Each device keeps its
//! own copy in search-index.json (not synced, so devices never conflict over it). Pages of locked
//! sections are never indexed, so their contents can't leak into this file.
use crate::model::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

const FILE: &str = "search-index.json";

#[derive(Default)]
pub struct Index {
    /// page id → (what the text was made from, the text)
    pub map: HashMap<u64, (u64, String)>,
    job: Option<(u64, u64, Receiver<String>)>,
    cursor: usize,
    dirty: bool,
    saved_at: f64,
}

/// What a page's searchable extra text depends on: its ink and its PDFs.
fn source_hash(p: &Page) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    let mut add = |x: u64| h = (h ^ x).wrapping_mul(0x100000001b3);
    for s in p.strokes.iter().filter(|s| !s.highlighter) { add(s.id); add(s.points.len() as u64); }
    for f in pdfs(p) { for b in f.bytes() { add(b as u64); } }
    h
}

fn pdfs(p: &Page) -> Vec<String> {
    p.blocks.iter().filter_map(|b| match &b.kind { Kind::File { file, name, .. } if name.to_lowercase().ends_with(".pdf") || file.ends_with(".pdf") => Some(file.clone()), _ => None }).collect()
}

/// Text of a PDF (first 100 pages, at most 100 000 characters).
fn pdf_text(path: &std::path::Path) -> String {
    let Ok(doc) = lopdf::Document::load(path) else { return String::new() };
    let pages: Vec<u32> = doc.get_pages().keys().copied().take(100).collect();
    doc.extract_text(&pages).unwrap_or_default().chars().take(100_000).collect()
}

impl Index {
    pub fn load(persist: bool) -> Index {
        let map = if persist { load::<HashMap<u64, (u64, String)>>(FILE).unwrap_or_default() } else { HashMap::new() };
        Index { map, ..Default::default() }
    }

    /// Some extra searchable text for this page (recognised handwriting, PDF text).
    pub fn text(&self, id: u64) -> Option<&str> { self.map.get(&id).map(|(_, t)| t.as_str()).filter(|t| !t.is_empty()) }

    /// A little work per frame: collect a finished job, or start one for the next page that changed.
    pub fn tick(&mut self, store: &Store, files: PathBuf, persist: bool, now: f64) {
        if let Some((id, hash, rx)) = &self.job {
            match rx.try_recv() {
                Ok(text) => { self.map.insert(*id, (*hash, text)); self.dirty = true; self.job = None; }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(_) => { self.map.insert(*id, (*hash, String::new())); self.job = None; } // the reader failed: don't retry this version
            }
        }
        let pages: Vec<(&Page, bool)> = store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| s.pages.iter().map(move |p| (p, s.lock.is_some()))).collect();
        if pages.is_empty() { return; }
        // look at up to 30 pages per frame, round-robin
        for k in 0..pages.len().min(30) {
            let (p, locked) = pages[(self.cursor + k) % pages.len()];
            if locked {
                if self.map.remove(&p.id).is_some() { self.dirty = true; }
                continue;
            }
            let h = source_hash(p);
            if self.map.get(&p.id).is_some_and(|(old, _)| *old == h) { continue; }
            if p.strokes.is_empty() && pdfs(p).is_empty() { self.map.insert(p.id, (h, String::new())); continue; }
            let (strokes, pdfs, files) = (p.strokes.clone(), pdfs(p), files.clone());
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let text = std::panic::catch_unwind(|| {
                    let mut out = crate::hw::recognize_text(&strokes);
                    for f in pdfs.iter().filter(|f| safe_name(f)) { out.push('\n'); out.push_str(&pdf_text(&files.join(f))); }
                    out
                }).unwrap_or_default();
                let _ = tx.send(text);
            });
            self.job = Some((p.id, h, rx));
            self.cursor = (self.cursor + k + 1) % pages.len();
            break;
        }
        if self.job.is_none() { self.cursor = (self.cursor + 30) % pages.len(); }
        if persist && self.dirty && now - self.saved_at > 10.0 {
            let ids: std::collections::HashSet<u64> = pages.iter().map(|(p, _)| p.id).collect();
            self.map.retain(|id, _| ids.contains(id));
            let _ = save(FILE, &self.map);
            (self.dirty, self.saved_at) = (false, now);
        }
    }

    pub fn busy(&self) -> bool { self.job.is_some() }

    /// Drop these pages and write the file at once (their section was just locked).
    pub fn forget(&mut self, ids: &[u64], persist: bool) {
        if self.job.as_ref().is_some_and(|(id, ..)| ids.contains(id)) { self.job = None; }
        for id in ids { self.map.remove(id); }
        self.dirty = true;
        self.flush(persist);
    }

    /// Write what changed now (on the way out).
    pub fn flush(&mut self, persist: bool) {
        if persist && self.dirty { let _ = save(FILE, &self.map); self.dirty = false; }
    }
}

/// Where `q` occurs in `hay` (both lower case), as a byte range, and whether it was an exact match.
/// Without an exact match, each query word may be a little misspelled (one letter off, two for
/// long words) or the start of a longer word: typos, and handwriting the recognizer read slightly wrong.
pub fn fuzzy_find(hay: &str, q: &str) -> Option<(usize, usize, bool)> {
    if q.is_empty() { return None; }
    if let Some(at) = hay.find(q) { return Some((at, at + q.len(), true)); }
    // (byte offset, word); the offset comes from the slice itself, so separators of any width are fine
    let words: Vec<(usize, &str)> = hay.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| (w.as_ptr() as usize - hay.as_ptr() as usize, w)).collect();
    let mut first = None;
    for qw in q.split_whitespace() {
        let n = qw.chars().count();
        let allow = if n >= 8 { 2 } else if n >= 4 { 1 } else { 0 };
        let hit = words.iter().find(|(_, w)| w.starts_with(qw) || (allow > 0 && (w.chars().count() as i32 - n as i32).abs() <= allow as i32 && within(w, qw, allow)))?;
        first = first.or(Some((hit.0, hit.0 + hit.1.len(), false)));
    }
    first
}

/// Edit distance between `a` and `b` is at most `k` (letters added, dropped, changed, or two neighbours swapped).
fn within(a: &str, b: &str, k: usize) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 0..=a.len() { d[i][0] = i; }
    for j in 0..=b.len() { d[0][j] = j; }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            d[i][j] = (d[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1])).min(d[i - 1][j] + 1).min(d[i][j - 1] + 1);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] { d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1); }
        }
        if d[i].iter().min().is_some_and(|m| *m > k) { return false; }
    }
    d[a.len()][b.len()] <= k
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_pdf_text() {
        use lopdf::{content::{Content, Operation}, dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
        let content = Content { operations: vec![
            Operation::new("BT", vec![]), Operation::new("Tf", vec!["F1".into(), 24.into()]),
            Operation::new("Td", vec![100.into(), 600.into()]), Operation::new("Tj", vec![Object::string_literal("Convolution theorem")]), Operation::new("ET", vec![]),
        ] };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content_id, "Resources" => resources, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()] });
        doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }));
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let path = std::env::temp_dir().join(format!("ln-index-{}.pdf", new_id()));
        doc.save(&path).unwrap();
        assert!(pdf_text(&path).contains("Convolution theorem"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn indexes_changed_pages_and_skips_locked_ones() {
        let mut store = sample();
        let mut ix = Index::default();
        let files = std::env::temp_dir();
        for i in 0..200 {
            ix.tick(&store, files.clone(), false, i as f64);
            if !ix.busy() && store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).all(|p| ix.map.contains_key(&p.id)) { break; }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let inked = store.notebooks[0].sections[0].pages[0].id;
        assert!(ix.map.contains_key(&inked), "the page with ink was read");
        // locking a section drops its pages from the index
        let sec = &mut store.notebooks[0].sections[0];
        sec.lock = Some(Lock { salt: String::new(), nonce: String::new(), data: String::new() });
        for i in 0..20 { ix.tick(&store, files.clone(), false, 300.0 + i as f64); }
        assert!(!ix.map.contains_key(&inked));
    }

    #[test]
    fn fuzzy() {
        let t = "the fourier series of a square wave";
        assert_eq!(fuzzy_find(t, "series"), Some((12, 18, true)));
        assert_eq!(fuzzy_find(t, "fourir"), Some((4, 11, false))); // one letter missing
        assert_eq!(fuzzy_find(t, "sqaure wave"), Some((24, 30, false))); // swapped letters, two words
        assert_eq!(fuzzy_find(t, "squ"), Some((24, 27, true)));
        assert_eq!(fuzzy_find(t, "wav"), Some((31, 34, true)));
        assert_eq!(fuzzy_find(t, "cat"), None); // short words must match exactly
        assert_eq!(fuzzy_find(t, "fourier cosine"), None); // every word has to be there
        let t = "signals — week 3 # fourier";
        assert_eq!(fuzzy_find(t, "fourir").map(|(a, b, _)| &t[a..b]), Some("fourier")); // after a wide "—"
    }
}
