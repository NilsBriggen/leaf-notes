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
        let mut f = std::fs::File::create(&tmp)?;
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

/// Load the notes. An unreadable library falls back to its backup; if both are unreadable this is
/// an error, so the caller never starts over on top of real notes.
pub fn load(dir: &Path) -> Result<Loaded, String> {
    let lib = dir.join(LIBRARY);
    if lib.exists() {
        let mut store: Store = match read_json(&lib).or_else(|| read_json(&dir.join(format!("{LIBRARY}.backup")))) {
            Some(s) => s,
            // An empty file holds nothing to protect (older versions could leave one after a sudden
            // power-off): bring back the pages themselves, or start fresh if there are none.
            None if std::fs::metadata(&lib).is_ok_and(|m| m.len() == 0) => return Ok(recover_pages(dir).map_or(Loaded::Fresh, |s| Loaded::Store(s, vec![]))),
            None => return Err(format!("{} is damaged and has no readable backup.", lib.display())),
        };
        let mut damaged = vec![];
        for p in store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).flat_map(|s| s.pages.iter_mut()) {
            match read_json::<Page>(&page_path(dir, p.id)) {
                Some(full) if full.id == p.id => *p = Page { title: p.title.clone(), level: p.level, ..full },
                _ => damaged.push(p.title.clone()),
            }
        }
        return Ok(Loaded::Store(store, damaged));
    }
    let old = dir.join(OLD_STORE);
    if old.exists() {
        let store: Store = read_json(&old).or_else(|| read_json(&dir.join(format!("{OLD_STORE}.backup"))))
            .ok_or_else(|| format!("{} is damaged and has no readable backup.", old.display()))?;
        return Ok(Loaded::Store(store, vec![]));
    }
    Ok(Loaded::Fresh)
}

/// Every readable page file, gathered into one notebook (when the list of notebooks is lost).
fn recover_pages(dir: &Path) -> Option<Store> {
    let mut pages: Vec<Page> = std::fs::read_dir(dir.join("pages")).ok()?.flatten().filter_map(|e| read_json::<Page>(&e.path())).collect();
    if pages.is_empty() { return None; }
    pages.sort_by_key(|p| p.created);
    let section = Section { id: new_id(), name: crate::i18n::tr("Recovered pages").into(), color: crate::model::PALETTE[1], pages, lock: None };
    Some(Store { notebooks: vec![Notebook { id: new_id(), name: crate::i18n::tr("Recovered notes").into(), color: crate::model::PALETTE[0], sections: vec![section] }], trash: vec![], study: Default::default() })
}

fn hash(b: &[u8]) -> u64 { b.iter().fold(0xcbf29ce484222325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100000001b3)) }

/// Saves only what changed since the last save of this session.
#[derive(Default)]
pub struct Saver {
    written: HashMap<u64, u64>,
    history_at: HashMap<u64, u64>,
    started: bool,
}

impl Saver {
    pub fn save(&mut self, dir: &Path, store: &Store) -> std::io::Result<()> {
        std::fs::create_dir_all(dir.join("pages"))?;
        if !self.started {
            // once per session: keep the library from before, so a bad session can be rolled back by hand
            let _ = std::fs::copy(dir.join(LIBRARY), dir.join(format!("{LIBRARY}.backup")));
        }
        let now = now_secs();
        let mut seen = std::collections::HashSet::new();
        for p in store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages) {
            seen.insert(p.id);
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
            // page files of pages that no longer exist (deleted pages live on in the library's Recently deleted)
            for e in std::fs::read_dir(dir.join("pages")).into_iter().flatten().flatten() {
                let id = e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).and_then(|n| n.parse::<u64>().ok());
                if id.is_some_and(|id| !seen.contains(&id)) { let _ = std::fs::remove_file(e.path()); }
            }
        } else {
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

impl Notebook { fn clone_meta(&self) -> Notebook { Notebook { id: self.id, name: self.name.clone(), color: self.color, sections: vec![] } } }
impl Section { fn clone_meta(&self) -> Section { Section { id: self.id, name: self.name.clone(), color: self.color, pages: vec![], lock: self.lock.clone() } } }

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

    fn same(a: &Store, b: &Store) -> bool { serde_json::to_string(a).unwrap() == serde_json::to_string(b).unwrap() }
}
