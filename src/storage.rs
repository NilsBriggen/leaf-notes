//! Where notes live on disk: `library.json` holds notebooks, sections, page titles and Recently
//! deleted; each page's content is its own file in `pages/`. A save only rewrites pages that
//! changed, so big notebooks stay fast, and a damaged file costs one page, not everything.
//! Before a changed page is overwritten, its previous version goes to `history/` (at most every
//! 10 minutes per page, the last 40 kept), so older versions can be brought back.
use crate::model::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const LIBRARY: &str = "library.json";
/// The single-file format of earlier versions (read once, then kept as a backup).
const OLD_STORE: &str = "notebooks.json";
const HISTORY_EVERY: u64 = 600;
const HISTORY_KEEP: usize = 40;

/// What loading found.
pub enum Loaded {
    /// Nothing saved yet: a new install.
    Fresh,
    /// The notes, plus the titles of pages whose file couldn't be read (kept as empty pages).
    Store(Store, Vec<String>),
}

fn page_path(dir: &Path, id: u64) -> PathBuf { dir.join("pages").join(format!("{id}.json")) }
fn history_path(dir: &Path, id: u64) -> PathBuf { dir.join("history").join(format!("{id}.jsonl")) }

/// Write a file so that a crash, a dead battery or a pulled plug leaves either the old or the new
/// version, never an empty file: a temporary file, flushed to the disk, then renamed over the old
/// one (and the folder flushed, so the rename itself is kept).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    // a unique name: two writers (this app, a sync folder) never share a temporary file
    let tmp = path.with_extension(format!("{}.tmp", crate::new_id()));
    let done = (|| {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        // notes and settings (which hold secrets where there's no keyring) are for this user only
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if done.is_err() { let _ = std::fs::remove_file(&tmp); }
    done?;
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) { let _ = d.sync_all(); }
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> { serde_json::from_slice(&std::fs::read(path).ok()?).ok() }

/// A page without its content (what `library.json` keeps).
fn stub(p: &Page) -> Page {
    Page { id: p.id, title: p.title.clone(), created: p.created, level: p.level, paper: p.paper, blocks: vec![], strokes: vec![], recordings: vec![], kind: p.kind, cards: vec![], other_of: p.other_of }
}

/// Load the notes. An unreadable or missing library falls back to its backup, then to the page
/// files themselves; if a library exists but nothing can be read this is an error, so the caller
/// never starts over on top of real notes.
pub fn load(dir: &Path) -> Result<Loaded, String> {
    let lib = dir.join(LIBRARY);
    let backup = dir.join(format!("{LIBRARY}.backup"));
    if lib.exists() || backup.exists() {
        let mut store: Store = match read_json(&lib).or_else(|| read_json(&backup)) {
            Some(s) => s,
            // A missing or empty file holds nothing to protect (older versions could leave an empty one
            // after a sudden power-off): bring back the pages themselves, or start fresh if there are none.
            None if std::fs::metadata(&lib).map_or(true, |m| m.len() == 0) => return Ok(recover_pages(dir, &Default::default()).map_or(Loaded::Fresh, |s| Loaded::Store(s, vec![]))),
            None => return Err(format!("{} is damaged and has no readable backup.", lib.display())),
        };
        let mut damaged = vec![];
        for p in store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).flat_map(|s| s.pages.iter_mut()) {
            let path = page_path(dir, p.id);
            match read_json::<Page>(&path) {
                Some(mut full) if full.id == p.id => {
                    full.strokes.retain(|s| !s.points.is_empty()); // a stroke with no points is nothing to keep
                    *p = Page { title: p.title.clone(), level: p.level, ..full }
                }
                _ => {
                    // keep the damaged bytes for repair by hand; the empty page saved in their place must not overwrite them
                    if path.exists() { let _ = std::fs::rename(&path, path.with_extension("json.damaged")); }
                    damaged.push(p.title.clone());
                }
            }
        }
        // page files the library doesn't list (it came from the backup, or lost them): gathered, never deleted
        let mut known: std::collections::HashSet<u64> = store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).map(|p| p.id).collect();
        known.extend(trashed_page_ids(&store));
        if let Some(found) = recover_pages(dir, &known) { store.notebooks.extend(found.notebooks); }
        return Ok(Loaded::Store(store, damaged));
    }
    let old = dir.join(OLD_STORE);
    if old.exists() {
        let store: Store = read_json(&old).or_else(|| read_json(&dir.join(format!("{OLD_STORE}.backup"))))
            .ok_or_else(|| format!("{} is damaged and has no readable backup.", old.display()))?;
        return Ok(Loaded::Store(store, vec![]));
    }
    Ok(recover_pages(dir, &Default::default()).map_or(Loaded::Fresh, |s| Loaded::Store(s, vec![])))
}

/// Pages waiting in Recently deleted (their content is in the library, so their old files may go).
fn trashed_page_ids(store: &Store) -> Vec<u64> {
    store.trash.iter().flat_map(|t| match &t.item {
        TrashItem::Page { page, .. } => vec![page.id],
        TrashItem::Section { section, .. } => section.pages.iter().map(|p| p.id).collect(),
        TrashItem::Notebook(n) => n.sections.iter().flat_map(|s| &s.pages).map(|p| p.id).collect(),
    }).collect()
}

/// Every readable page file not in `known`, gathered into one notebook (when the list of notebooks lost them).
fn recover_pages(dir: &Path, known: &std::collections::HashSet<u64>) -> Option<Store> {
    let mut pages: Vec<Page> = std::fs::read_dir(dir.join("pages")).ok()?.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        // a known page's file (its id is its name) isn't read a second time
        .filter(|e| e.path().file_stem().and_then(|n| n.to_str()).and_then(|n| n.parse::<u64>().ok()).is_none_or(|id| !known.contains(&id)))
        .filter_map(|e| read_json::<Page>(&e.path())).filter(|p| !known.contains(&p.id)).collect();
    if pages.is_empty() { return None; }
    pages.sort_by_key(|p| p.created);
    let section = Section { id: new_id(), name: crate::i18n::tr("Recovered pages").into(), color: crate::model::PALETTE[1], pages, lock: None };
    Some(Store { notebooks: vec![Notebook { id: new_id(), name: crate::i18n::tr("Recovered notes").into(), color: crate::model::PALETTE[0], sections: vec![section] }], trash: vec![], study: Default::default() })
}

fn hash(b: &[u8]) -> u64 { b.iter().fold(0xcbf29ce484222325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100000001b3)) }

/// A quick fingerprint of a page: everything but the inside of its ink strokes (their count, ends
/// and looks are in). Ink only changes on the page in front, which is always fully compared.
fn quick_sig(p: &Page) -> u64 {
    let meta = serde_json::to_vec(&(&p.title, p.level, p.paper, p.kind, p.created, p.other_of, &p.blocks, &p.cards, &p.recordings)).unwrap_or_default();
    let mut h = hash(&meta);
    let mut add = |x: u64| h = (h ^ x).wrapping_mul(0x100000001b3);
    for s in &p.strokes {
        add(s.id);
        add(s.points.len() as u64);
        for q in s.points.first().into_iter().chain(s.points.last()) { for v in q { add(v.to_bits() as u64); } }
        add(u32::from_be_bytes(s.color) as u64);
        add(s.width.to_bits() as u64);
    }
    h
}

/// Saves only what changed since the last save of this session.
#[derive(Default)]
pub struct Saver {
    written: HashMap<u64, u64>,
    /// quick fingerprint of each page as last written (see `quick_sig`)
    sigs: HashMap<u64, u64>,
    history_at: HashMap<u64, u64>,
    started: bool,
}

impl Saver {
    pub fn save(&mut self, dir: &Path, store: &Store) -> std::io::Result<()> { self.save_with(dir, store, None) }

    /// With `front`: a quick save. That page is compared in full; the others only by their quick
    /// fingerprint, which saves turning every page of a big library into text a few times a minute.
    pub fn save_with(&mut self, dir: &Path, store: &Store, front: Option<u64>) -> std::io::Result<()> {
        std::fs::create_dir_all(dir.join("pages"))?;
        if !self.started && read_json::<Store>(&dir.join(LIBRARY)).is_some() {
            // once per session: keep the library from before, so a bad session can be rolled back by hand
            // (only a readable one: a damaged library must never replace the good backup)
            let _ = std::fs::copy(dir.join(LIBRARY), dir.join(format!("{LIBRARY}.backup")));
        }
        let now = now_secs();
        let mut seen = std::collections::HashSet::new();
        for p in store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages) {
            seen.insert(p.id);
            let sig = quick_sig(p);
            if self.started && front.is_some_and(|f| f != p.id) && self.sigs.get(&p.id) == Some(&sig) { continue; }
            self.sigs.insert(p.id, sig);
            let bytes = serde_json::to_vec(p)?;
            let h = hash(&bytes);
            if self.written.get(&p.id) == Some(&h) { continue; }
            let path = page_path(dir, p.id);
            // the first save of a session can't know what's on disk: compare before rewriting
            let old = std::fs::read(&path).ok();
            if old.as_deref() == Some(&bytes[..]) { self.written.insert(p.id, h); continue; }
            if let Some(old) = old.filter(|_| self.history_at.get(&p.id).is_none_or(|t| now.saturating_sub(*t) >= HISTORY_EVERY)) {
                self.add_history(dir, p.id, now, &old);
            }
            write_atomic(&path, &bytes)?;
            self.written.insert(p.id, h);
        }
        let library = Store {
            notebooks: store.notebooks.iter().map(|n| Notebook { sections: n.sections.iter().map(|s| Section { pages: s.pages.iter().map(stub).collect(), ..s.clone_meta() }).collect(), ..n.clone_meta() }).collect(),
            trash: store.trash.clone(),
            study: store.study.clone(),
        };
        write_atomic(&dir.join(LIBRARY), &serde_json::to_vec(&library)?)?;
        if !self.started {
            self.started = true;
            // the old single file has been split up: keep it next to the new files as a backup
            let old = dir.join(OLD_STORE);
            if old.exists() { let _ = std::fs::rename(&old, dir.join(format!("{OLD_STORE}.before-split"))); }
            // page files of deleted pages (they live on in the library's Recently deleted). Any other
            // unknown page file is left alone: loading gathers those into Recovered pages.
            let trashed: std::collections::HashSet<u64> = trashed_page_ids(store).into_iter().collect();
            for e in std::fs::read_dir(dir.join("pages")).into_iter().flatten().flatten() {
                let id = e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).and_then(|n| n.parse::<u64>().ok());
                if id.is_some_and(|id| !seen.contains(&id) && trashed.contains(&id)) { let _ = std::fs::remove_file(e.path()); }
            }
        } else {
            // a page restored later (undo delete, the bin) must be written again even if unchanged
            self.sigs.retain(|id, _| seen.contains(id));
            for id in self.written.keys().filter(|id| !seen.contains(id)).copied().collect::<Vec<_>>() {
                self.written.remove(&id);
                let _ = std::fs::remove_file(page_path(dir, id));
            }
        }
        Ok(())
    }

    fn add_history(&mut self, dir: &Path, id: u64, now: u64, old_page: &[u8]) {
        self.history_at.insert(id, now);
        let path = history_path(dir, id);
        let mut lines: Vec<String> = std::fs::read_to_string(&path).map(|s| s.lines().map(String::from).collect()).unwrap_or_default();
        lines.push(format!("{{\"t\":{now},\"page\":{}}}", String::from_utf8_lossy(old_page)));
        let keep = lines.len().saturating_sub(HISTORY_KEEP);
        let _ = write_atomic(&path, (lines[keep..].join("\n") + "\n").as_bytes());
    }
}

/// Earlier versions of a page, newest first: (when it was replaced, the page then).
pub fn history(dir: &Path, id: u64) -> Vec<(u64, Page)> {
    #[derive(serde::Deserialize)]
    struct Entry { t: u64, page: Page }
    let mut v: Vec<(u64, Page)> = std::fs::read_to_string(history_path(dir, id)).unwrap_or_default().lines()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok()).map(|e| (e.t, e.page)).collect();
    v.reverse();
    v
}

impl Notebook { pub(crate) fn clone_meta(&self) -> Notebook { Notebook { id: self.id, name: self.name.clone(), color: self.color, sections: vec![] } } }
impl Section { pub(crate) fn clone_meta(&self) -> Section { Section { id: self.id, name: self.name.clone(), color: self.color, pages: vec![], lock: self.lock.clone() } } }

#[cfg(test)]
mod tests {
    use super::*;
    fn tmp() -> PathBuf { let d = std::env::temp_dir().join(format!("ln-store-{}", new_id())); std::fs::create_dir_all(&d).unwrap(); d }
    fn titles(s: &Store) -> Vec<String> { s.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).map(|p| p.title.clone()).collect() }

    #[test]
    fn empty_library_file_recovers() {
        // nothing at all: an empty library file means a fresh start, not "can't read, never save"
        let d = tmp();
        std::fs::write(d.join(LIBRARY), b"").unwrap();
        assert!(matches!(load(&d), Ok(Loaded::Fresh)));
        // with page files next to it: they come back
        let p = Page::new("Kept");
        write_atomic(&page_path(&d, p.id), &serde_json::to_vec(&p).unwrap()).unwrap();
        match load(&d) { Ok(Loaded::Store(s, _)) => assert_eq!(titles(&s), vec!["Kept".to_string()]), _ => panic!("pages not recovered") }
        // a library with real but unreadable content is still never treated as empty
        std::fs::write(d.join(LIBRARY), b"{ not json").unwrap();
        assert!(load(&d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn splits_saves_and_reloads() {
        let dir = tmp();
        // an old single-file store is read, split up on the first save, and kept as a backup
        let mut store = sample();
        std::fs::write(dir.join(OLD_STORE), serde_json::to_vec(&store).unwrap()).unwrap();
        let Ok(Loaded::Store(loaded, damaged)) = load(&dir) else { panic!() };
        assert!(damaged.is_empty() && same(&loaded, &store));
        let mut saver = Saver::default();
        saver.save(&dir, &store).unwrap();
        assert!(dir.join(LIBRARY).exists() && dir.join(format!("{OLD_STORE}.before-split")).exists() && !dir.join(OLD_STORE).exists());
        let Ok(Loaded::Store(again, _)) = load(&dir) else { panic!() };
        assert!(same(&again, &store), "a round trip keeps everything");
        // only the changed page is rewritten, and its old version goes to history
        let id = store.notebooks[0].sections[0].pages[0].id;
        let other = page_path(&dir, store.notebooks[1].sections[0].pages[0].id);
        let before = std::fs::metadata(&other).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        store.notebooks[0].sections[0].pages[0].title = "Renamed".into();
        saver.save(&dir, &store).unwrap();
        assert_eq!(std::fs::metadata(&other).unwrap().modified().unwrap(), before);
        let h = history(&dir, id);
        assert_eq!(h.len(), 1);
        assert_ne!(h[0].1.title, "Renamed");
        // a new session keeps a backup of the library it started with
        Saver::default().save(&dir, &store).unwrap();
        assert!(dir.join(format!("{LIBRARY}.backup")).exists());
        // a damaged page file costs that page only; a damaged library falls back to its backup
        std::fs::write(page_path(&dir, id), b"{broken").unwrap();
        let Ok(Loaded::Store(s, damaged)) = load(&dir) else { panic!() };
        assert_eq!(damaged, vec!["Renamed".to_string()]);
        assert!(titles(&s).contains(&"Groceries".into()));
        std::fs::write(dir.join(LIBRARY), b"{broken").unwrap();
        assert!(matches!(load(&dir), Ok(Loaded::Store(..))), "the backup library is used");
        std::fs::write(dir.join(format!("{LIBRARY}.backup")), b"{broken").unwrap();
        assert!(load(&dir).is_err(), "never silently start over");
        // a deleted page's file goes away
        let mut store2 = store.clone();
        store2.notebooks[1].sections[0].pages.clear();
        saver.save(&dir, &store2).unwrap();
        assert!(!other.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_lost_or_damaged_library_never_costs_pages() {
        // the library is gone, its backup too: the page files come back instead of a fresh start that deletes them
        let d = tmp();
        let store = sample();
        Saver::default().save(&d, &store).unwrap();
        std::fs::remove_file(d.join(LIBRARY)).unwrap();
        let _ = std::fs::remove_file(d.join(format!("{LIBRARY}.backup")));
        let Ok(Loaded::Store(s, _)) = load(&d) else { panic!("pages not recovered") };
        assert_eq!(titles(&s).len(), titles(&store).len());
        // the library is gone but its backup isn't: the backup is used, and a page made after it is kept
        let d = tmp();
        let mut store = sample();
        let mut saver = Saver::default();
        saver.save(&d, &store).unwrap();
        Saver::default().save(&d, &store).unwrap(); // a second session: the backup exists now
        store.notebooks[0].sections[0].pages.push(Page::new("Made after the backup"));
        saver.save(&d, &store).unwrap();
        std::fs::write(d.join(LIBRARY), b"{broken").unwrap();
        let Ok(Loaded::Store(s, _)) = load(&d) else { panic!() };
        assert!(titles(&s).contains(&"Made after the backup".into()), "orphan page gathered");
        // the next session's first save neither overwrites the good backup nor deletes the page
        Saver::default().save(&d, &s).unwrap();
        assert!(read_json::<Store>(&d.join(format!("{LIBRARY}.backup"))).is_some());
        let Ok(Loaded::Store(again, _)) = load(&d) else { panic!() };
        assert_eq!(titles(&again).iter().filter(|t| *t == "Made after the backup").count(), 1);
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(d.join(LIBRARY)).unwrap().permissions().mode() & 0o777, 0o600); }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn quick_saves_still_catch_changes_elsewhere() {
        let d = tmp();
        let mut store = sample();
        let mut saver = Saver::default();
        saver.save(&d, &store).unwrap();
        let front = store.notebooks[0].sections[0].pages[0].id;
        let other = store.notebooks[1].sections[0].pages[0].id;
        let before = std::fs::metadata(page_path(&d, other)).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        // nothing changed elsewhere: not rewritten
        store.notebooks[0].sections[0].pages[0].title = "Front".into();
        saver.save_with(&d, &store, Some(front)).unwrap();
        assert_eq!(std::fs::metadata(page_path(&d, other)).unwrap().modified().unwrap(), before);
        // a change on another page (an AI job, a tick in the task list): written by a quick save too
        store.notebooks[1].sections[0].pages[0].blocks.push(block(0.0, 0.0, 100.0, Kind::Text { text: "elsewhere".into(), heading: 0 }));
        saver.save_with(&d, &store, Some(front)).unwrap();
        let Ok(Loaded::Store(s, _)) = load(&d) else { panic!() };
        assert!(s.notebooks[1].sections[0].pages[0].blocks.iter().any(|b| matches!(&b.kind, Kind::Text { text, .. } if text == "elsewhere")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_restored_page_is_written_again_at_once() {
        let d = tmp();
        let mut store = sample();
        let mut saver = Saver::default();
        saver.save(&d, &store).unwrap();
        let front = store.notebooks[0].sections[0].pages[0].id;
        let gone = store.notebooks[1].sections[0].pages.remove(0);
        saver.save_with(&d, &store, Some(front)).unwrap();
        assert!(!page_path(&d, gone.id).exists());
        // undo delete brings it back unchanged: a quick save writes it, not only the next full save
        store.notebooks[1].sections[0].pages.insert(0, gone.clone());
        saver.save_with(&d, &store, Some(front)).unwrap();
        assert!(page_path(&d, gone.id).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    fn same(a: &Store, b: &Store) -> bool { serde_json::to_string(a).unwrap() == serde_json::to_string(b).unwrap() }
}
