//! Sync between devices through a shared folder (Dropbox, OneDrive, Syncthing…),
//! a WebDAV server (Nextcloud…), or Google Drive.
//!
//! Incremental per-page format (`FORMAT = 2`):
//! - `LeafNotes/library.json` holds notebook/section metadata, page stubs, locked sections,
//!   trash, study progress, `page_hashes`, and the synced `files` list.
//! - `LeafNotes/pages/{id}.json` holds each individual non-locked page (only changed pages are
//!   uploaded or downloaded).
//! - `LeafNotes/files/{name}` holds referenced pictures, PDFs, and recordings.
//!
//! Seamless one-time migration: if a remote only has legacy v1 `LeafNotes/notes.json`, it is read,
//! merged, and written out in v2 format.
use crate::model::*;
use crate::storage::{self, write_atomic};
use crate::tf;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SYNC_FILE: &str = "sync.json";
const BASE_FILE: &str = "sync-base.json";
const LIBRARY_FILE: &str = "library.json";
const LEGACY_NOTES: &str = "notes.json";
/// Format version:
/// - 1: monolithic `LeafNotes/notes.json`
/// - 2: incremental `LeafNotes/library.json` + `LeafNotes/pages/{id}.json`
const FORMAT: u64 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum Via {
    #[default]
    Off,
    Folder,
    WebDav,
    Google,
}

#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct SyncSettings {
    pub via: Via,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    /// Google Drive: the long-lived sign-in (refresh token) and whose Drive it is.
    #[serde(default)]
    pub google_token: String,
    #[serde(default)]
    pub google_email: String,
    /// Recordings bigger than this many MB stay on this device (0: no limit), for small data plans and quotas.
    #[serde(default)]
    pub max_recording_mb: u32,
    /// Explicit opt-in to allow unencrypted `http://` on a non-loopback private LAN / `.local` host.
    #[serde(default)]
    pub allow_lan_http: bool,
}

impl SyncSettings {
    pub fn on(&self) -> bool {
        match self.via {
            Via::Off => false,
            Via::Folder => !self.folder.trim().is_empty(),
            Via::WebDav => !self.url.trim().is_empty(),
            Via::Google => !self.google_token.is_empty(),
        }
    }
    /// Where the notes go; the base only counts for the same place.
    fn target(&self) -> String {
        match self.via {
            Via::Folder => self.folder.trim().into(),
            Via::WebDav => format!("{}@{}", self.user.trim(), self.url.trim()),
            Via::Google => format!("google:{}", self.google_email),
            Via::Off => String::new(),
        }
    }
}

/// The last state this device and the shared copy agreed on (`tag` identifies that version of `library.json`).
#[derive(Serialize, Deserialize)]
struct Base {
    target: String,
    tag: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    page_hashes: HashMap<u64, String>,
    #[serde(default)]
    legacy_locked: bool,
    store: Store,
}

/// Remote v2 manifest stored in `LeafNotes/library.json`.
#[derive(Serialize, Deserialize)]
struct RemoteManifest {
    format: u64,
    library: Store,
    #[serde(default)]
    page_hashes: HashMap<u64, String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    legacy_locked: bool,
}

/// How a device that already has notes joins a sync location that already has notes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum First {
    Merge,
    UseShared,
}

pub enum Outcome {
    /// Done: `store` is now on both sides. Commit `base` once `store` is saved on this device.
    /// `changed`: `store` differs from the notes this round started with (worked out here, off the window's thread).
    Synced {
        store: Store,
        up: usize,
        down: usize,
        base: PendingBase,
        problem: Option<String>,
        changed: bool,
    },
    /// First sync here and both sides have notes: ask the person how to join (pages in the shared copy).
    AskFirst { pages: usize },
}

/// Extract normalized host from the authority part of a URL (after `http://` or `https://`).
/// Rejects embedded credentials (`user@host`) to prevent spoofing like `127.0.0.1@evil.com`.
fn parse_host(rest: &str) -> Option<String> {
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    if auth.is_empty() || auth.contains('@') {
        return None;
    }
    let host = if auth.starts_with('[') {
        auth.split(']').next()?.trim_start_matches('[')
    } else {
        auth.rsplit_once(':').map_or(auth, |(h, _)| h)
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// True only for loopback addresses (`localhost`, `127.0.0.1`, `::1`, or `127.x.x.x`).
/// Auto-allowed over `http://` because traffic never leaves the local machine.
pub(crate) fn is_loopback_host(rest: &str) -> bool {
    let Some(host) = parse_host(rest) else { return false };
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => host == "localhost",
    }
}

/// True for non-loopback private LAN / link-local / `.local` hosts where `http://` requires
/// an explicit opt-in (`allow_lan_http`) and warning.
pub(crate) fn is_private_lan_host(rest: &str) -> bool {
    let Some(host) = parse_host(rest) else { return false };
    if is_loopback_host(rest) {
        return false;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_unique_local() || ip.is_unicast_link_local(),
        Err(_) => host.ends_with(".local") && host.len() > 6,
    }
}

/// True when `url` is a non-loopback `http://` address on a private LAN / `.local` host.
pub(crate) fn is_unencrypted_lan_url(url: &str) -> bool {
    let u = url.trim();
    u.strip_prefix("http://").is_some_and(is_private_lan_host)
}

/// Validate a remote HTTP(S) URL for WebDAV or AI servers:
/// - `https://` is always allowed.
/// - `http://` on loopback (`localhost`, `127.0.0.1`, `::1`) is auto-allowed.
/// - `http://` on a private LAN / `.local` host requires `allow_lan_http == true`.
/// - `http://` on any public internet host is always rejected.
pub(crate) fn validate_http_url(url: &str, allow_lan_http: bool) -> Result<(), String> {
    let u = url.trim();
    if u.starts_with("https://") {
        return Ok(());
    }
    if let Some(rest) = u.strip_prefix("http://") {
        if is_loopback_host(rest) {
            return Ok(());
        }
        if is_private_lan_host(rest) {
            if allow_lan_http {
                return Ok(());
            }
            return Err(crate::tr("Plain http:// on a local network sends your data unencrypted. Use https:// or enable “Allow unencrypted local network (http://)” in Settings.").into());
        }
    }
    Err(crate::tr("The server address must start with https://").into())
}

/// The new agreed state, written only after the notes it describes are saved here: if the app
/// stopped in between, a base ahead of the saved notes would make the next sync undo other devices' edits.
pub struct PendingBase(Vec<u8>);

impl PendingBase {
    pub fn commit(&self, home: &Path) -> Result<(), String> {
        write_atomic(&home.join(BASE_FILE), &self.0).map_err(|e| tf!("Couldn't save the sync state: {e}", e = e))
    }
}

fn page_stub(p: &Page) -> Page {
    Page {
        id: p.id,
        title: p.title.clone(),
        created: p.created,
        paper: p.paper,
        level: p.level,
        kind: p.kind,
        other_of: p.other_of,
        blocks: vec![],
        strokes: vec![],
        cards: vec![],
        recordings: vec![],
    }
}

type PagePayloads = HashMap<u64, (String, Vec<u8>)>;
type DriveFolderListing = HashMap<String, HashMap<String, (String, String)>>;

/// Split a full `Store` into its manifest library (page stubs) and a map of serialized non-locked pages.
fn split_store(store: &Store) -> Result<(Store, PagePayloads), String> {
    let mut pages = HashMap::new();
    let mut notebooks = Vec::with_capacity(store.notebooks.len());
    for n in &store.notebooks {
        let mut sections = Vec::with_capacity(n.sections.len());
        for s in &n.sections {
            let mut stubs = Vec::with_capacity(s.pages.len());
            for p in &s.pages {
                if p.kind == PageKind::Locked || s.sealed() {
                    stubs.push(p.clone());
                } else {
                    let bytes = serde_json::to_vec(p).map_err(|e| e.to_string())?;
                    pages.insert(p.id, (hash(&bytes), bytes));
                    stubs.push(page_stub(p));
                }
            }
            sections.push(Section { pages: stubs, ..s.clone_meta() });
        }
        notebooks.push(Notebook { sections, ..n.clone_meta() });
    }
    let library = Store { notebooks, trash: store.trash.clone(), study: store.study.clone() };
    Ok((library, pages))
}

struct SharedState {
    store: Store,
    files: Vec<String>,
    page_hashes: HashMap<u64, String>,
    legacy_locked: bool,
    tag: Option<String>,
    from_v1: bool,
}

/// Reassemble a full `Store` from a v2 `RemoteManifest`, reusing unchanged pages from `base` and
/// downloading only the pages whose hash changed.
fn hydrate_v2(
    remote: &Remote,
    manifest: RemoteManifest,
    base: Option<&Base>,
    tag: String,
) -> Result<SharedState, String> {
    let base_pages: HashMap<u64, &Page> = base
        .map(|b| {
            b.store
                .notebooks
                .iter()
                .flat_map(|n| &n.sections)
                .flat_map(|s| &s.pages)
                .filter(|p| p.kind != PageKind::Locked)
                .map(|p| (p.id, p))
                .collect()
        })
        .unwrap_or_default();
    let base_hashes = base.map(|b| &b.page_hashes);
    let mut store = manifest.library;
    for n in &mut store.notebooks {
        for s in &mut n.sections {
            if s.sealed() {
                continue;
            }
            for p in &mut s.pages {
                if p.kind == PageKind::Locked {
                    continue;
                }
                let remote_h = manifest.page_hashes.get(&p.id);
                if let (Some(rh), Some(bh), Some(bp)) = (
                    remote_h,
                    base_hashes.and_then(|m| m.get(&p.id)),
                    base_pages.get(&p.id),
                ) {
                    if rh == bh {
                        *p = (*bp).clone();
                        continue;
                    }
                }
                let bytes = remote.get_page(p.id)?.ok_or_else(|| {
                    crate::tr("The synced notes file is damaged, so nothing was changed. Your notes on this device are safe.")
                        .to_string()
                })?;
                let loaded: Page = serde_json::from_slice(&bytes).map_err(|e| {
                    tf!("The synced notes couldn't be read ({e}), so nothing was changed.", e = e)
                })?;
                *p = loaded;
            }
        }
    }
    Ok(SharedState {
        store,
        files: manifest.files,
        page_hashes: manifest.page_hashes,
        legacy_locked: manifest.legacy_locked,
        tag: Some(tag),
        from_v1: false,
    })
}

/// One sync round. `home` is this device's data folder. `quick` uses short timeouts (when closing the app).
pub fn run(s: &SyncSettings, home: &Path, local: &Store, first: Option<First>, quick: bool) -> Result<Outcome, String> {
    let remote = Remote::new(s, quick)?;
    // closing the app: the files get about 10 s in all, the rest go next time
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let base: Option<Base> = std::fs::read(home.join(BASE_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice::<Base>(&b).ok())
        .filter(|b| b.target == s.target());
    let files = home.join("files");
    // files that wouldn't go up or down this round: tried once, reported, and the notes still sync
    let mut failed: Vec<(String, String)> = vec![];
    // sent in an earlier try of this round: not sent again when another device wrote in between
    let mut sent_files: HashSet<String> = HashSet::new();
    let mut sent_pages: HashMap<u64, String> = HashMap::new();

    // Several tries: if another device writes in between, fetch its version and merge again.
    for _ in 0..4 {
        let known = base.as_ref().map(|b| b.tag.as_str()).filter(|t| !t.is_empty());
        let shared: Option<SharedState> = match remote.get_library(known)? {
            Got::Same => {
                let b = base.as_ref().expect("Same only with a base");
                Some(SharedState {
                    store: b.store.clone(),
                    files: b.files.clone(),
                    page_hashes: b.page_hashes.clone(),
                    legacy_locked: b.legacy_locked,
                    tag: Some(b.tag.clone()),
                    from_v1: false,
                })
            }
            Got::Data(bytes, tag) => {
                let manifest = parse_manifest_v2(&bytes)?;
                Some(hydrate_v2(&remote, manifest, base.as_ref(), tag)?)
            }
            Got::Missing => {
                // Seamless one-time migration from legacy v1 `notes.json` if present.
                match remote.get_legacy_notes(None)? {
                    Got::Missing => None,
                    Got::Same => None,
                    Got::Data(bytes, _) => {
                        let (store, v1_files) = parse_shared_v1(&bytes)?;
                        let legacy_locked = storage::has_unmanifested_locked_section(&store);
                        Some(SharedState {
                            store,
                            files: v1_files,
                            page_hashes: HashMap::new(),
                            legacy_locked,
                            tag: None, // library.json does not exist yet
                            from_v1: true,
                        })
                    }
                }
            }
        };

        let mut merged = match (&shared, &base, first) {
            (None, _, _) => local.clone(),
            (Some(sh), _, Some(First::UseShared)) => sh.store.clone(),
            (Some(sh), Some(b), _) => merge(&b.store, local, &sh.store)?,
            (Some(sh), None, Some(First::Merge)) => merge(
                &Store { notebooks: vec![], trash: vec![], study: HashMap::new() },
                local,
                &sh.store,
            )?,
            (Some(sh), None, None) => {
                // the untouched sample notebook isn't "this device's notes": the synced ones simply replace it
                if has_notes(local) && !untouched_sample(local) {
                    return Ok(Outcome::AskFirst {
                        pages: sh.store.notebooks.iter().flat_map(|n| &n.sections).map(|s| s.pages.len()).sum(),
                    });
                }
                sh.store.clone()
            }
        };
        tidy(&mut merged);

        // Compute referenced attachments so we never upload local orphan files.
        let referenced = storage::store_files(&merged);
        let merged_legacy_locked = storage::has_unmanifested_locked_section(&merged)
            || shared.as_ref().is_some_and(|sh| sh.legacy_locked && storage::has_unmanifested_locked_section(&sh.store));
        let mut have: Vec<String> = shared.as_ref().map(|sh| sh.files.clone()).unwrap_or_default();
        let mut have_set: HashSet<String> = have.iter().cloned().collect();
        let (mut up, mut down) = (0, 0);

        // Upload referenced attachments first so other devices never see pages pointing to missing files.
        for f in list_files(&files) {
            if !referenced.contains(&f) {
                continue;
            }
            if have_set.contains(&f) || failed.iter().any(|(n, _)| *n == f) {
                continue;
            }
            if s.max_recording_mb > 0
                && f.ends_with(".wav")
                && std::fs::metadata(files.join(&f)).is_ok_and(|m| m.len() > s.max_recording_mb as u64 * 1_000_000)
            {
                continue;
            }
            if sent_files.contains(&f) {
                have_set.insert(f.clone());
                have.push(f);
                continue;
            }
            match remote.put_file(&f, &files.join(&f)) {
                Ok(()) => {
                    sent_files.insert(f.clone());
                    have_set.insert(f.clone());
                    have.push(f);
                    up += 1;
                }
                Err(e) => failed.push((f, e)),
            }
            if quick && (!failed.is_empty() || std::time::Instant::now() > deadline) {
                break;
            }
        }

        // Download missing attachments (skip during quick exit sync).
        for f in have.iter().filter(|_| !quick) {
            if !safe_name(f) || files.join(f).exists() || failed.iter().any(|(n, _)| n == f) {
                continue;
            }
            match remote.get_file(f, &files.join(f)) {
                Ok(true) => down += 1,
                Ok(false) => {}
                Err(e) => failed.push((f.clone(), e)),
            }
        }

        // Safe remote attachment GC: only prune files that are unreferenced in `merged`,
        // already known in `base.files` (so never racing another device's fresh upload),
        // and only when no unmanifested legacy v0 locked section exists anywhere.
        let mut pruned_files: Vec<String> = vec![];
        if !quick && !merged_legacy_locked {
            if let Some(b) = &base {
                let base_files: HashSet<&str> = b.files.iter().map(String::as_str).collect();
                have.retain(|f| {
                    let drop_it = !referenced.contains(f) && base_files.contains(f.as_str());
                    if drop_it {
                        pruned_files.push(f.clone());
                    }
                    !drop_it
                });
            }
        }
        have.sort();
        have.dedup();

        // Split merged store into library stubs + per-page payloads and upload only changed pages.
        let (library, merged_pages) = split_store(&merged)?;
        let merged_page_hashes: HashMap<u64, String> = merged_pages.iter().map(|(&id, (h, _))| (id, h.clone())).collect();
        let remote_hashes = shared.as_ref().map(|sh| &sh.page_hashes);

        let need_write = shared.as_ref().is_none_or(|sh| {
            sh.from_v1
                || sh.files != have
                || sh.page_hashes != merged_page_hashes
                || sh.legacy_locked != merged_legacy_locked
                || canon(&sh.store) != canon(&merged)
        });

        let tag = if !need_write {
            shared.as_ref().and_then(|sh| sh.tag.clone()).unwrap_or_default()
        } else {
            for (&id, (h, bytes)) in &merged_pages {
                let already_remote = remote_hashes.and_then(|m| m.get(&id)) == Some(h);
                let already_sent = sent_pages.get(&id) == Some(h);
                if already_remote || already_sent {
                    continue;
                }
                remote.put_page(id, bytes)?;
                sent_pages.insert(id, h.clone());
            }
            let manifest = RemoteManifest {
                format: FORMAT,
                library,
                page_hashes: merged_page_hashes.clone(),
                files: have.clone(),
                legacy_locked: merged_legacy_locked,
            };
            let bytes = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
            let expect = shared.as_ref().and_then(|sh| sh.tag.as_deref());
            match remote.put_library(&bytes, expect)? {
                Put::Done(t) => t,
                Put::Changed => continue,
            }
        };

        // Best-effort remote cleanup AFTER `library.json` has been committed.
        if !quick {
            for f in &pruned_files {
                let _ = remote.delete_file(f);
            }
            if let Some(sh) = &shared {
                for &old_id in sh.page_hashes.keys() {
                    if !merged_page_hashes.contains_key(&old_id) {
                        let _ = remote.delete_page(old_id);
                    }
                }
            }
        }

        let b = serde_json::to_vec(&Base {
            target: s.target(),
            tag,
            files: have,
            page_hashes: merged_page_hashes,
            legacy_locked: merged_legacy_locked,
            store: merged.clone(),
        })
        .map_err(|e| e.to_string())?;
        let problem = failed
            .first()
            .map(|(f, e)| tf!("{n} file(s) couldn't be synced this time (first: {f}: {e}).", n = failed.len(), f = f, e = e));
        let changed = canon(&merged) != canon(local);
        return Ok(Outcome::Synced {
            store: merged,
            up,
            down,
            base: PendingBase(b),
            problem,
            changed,
        });
    }
    Err(crate::tr("Another device kept syncing at the same moment — trying again shortly.").into())
}

fn parse_manifest_v2(bytes: &[u8]) -> Result<RemoteManifest, String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| {
        crate::tr("The synced notes file is damaged, so nothing was changed. Your notes on this device are safe.").to_string()
    })?;
    if v.get("format").and_then(Value::as_u64).unwrap_or(0) > FORMAT {
        return Err(crate::tr("The synced notes come from a newer Leaf Notes — update this app to keep syncing.").into());
    }
    serde_json::from_value::<RemoteManifest>(v)
        .map_err(|e| tf!("The synced notes couldn't be read ({e}), so nothing was changed.", e = e))
}

fn parse_shared_v1(bytes: &[u8]) -> Result<(Store, Vec<String>), String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| {
        crate::tr("The synced notes file is damaged, so nothing was changed. Your notes on this device are safe.").to_string()
    })?;
    if v.get("format").and_then(Value::as_u64).unwrap_or(0) > FORMAT {
        return Err(crate::tr("The synced notes come from a newer Leaf Notes — update this app to keep syncing.").into());
    }
    let store: Store = serde_json::from_value(v.get("store").cloned().unwrap_or(Value::Null))
        .map_err(|e| tf!("The synced notes couldn't be read ({e}), so nothing was changed.", e = e))?;
    let files = v
        .get("files")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|f| f.as_str().map(String::from)).collect())
        .unwrap_or_default();
    Ok((store, files))
}

/// Still exactly the sample notebook a new install starts with.
fn untouched_sample(s: &Store) -> bool {
    let texts = |s: &Store| {
        let mut v: Vec<String> = s
            .notebooks
            .iter()
            .flat_map(|n| &n.sections)
            .flat_map(|x| &x.pages)
            .map(|p| format!("{}\n{}", p.plain_text(), p.strokes.len()))
            .collect();
        v.sort();
        v
    };
    s.trash.is_empty() && texts(s) == texts(&crate::model::sample())
}

fn has_notes(s: &Store) -> bool {
    s.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).any(|p| {
        !p.blocks.is_empty()
            || !p.strokes.is_empty()
            || p.cards.iter().any(|c| !c.front.is_empty())
            || (!p.title.trim().is_empty() && p.title != "Untitled page")
    })
}

fn list_files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| safe_name(n) && !n.ends_with(".part") && !n.ends_with(".tmp"))
        .collect();
    v.sort();
    v
}

/// Floats and maps compare reliably only after a round trip through JSON text.
fn canon<T: Serialize>(v: &T) -> Value {
    serde_json::to_vec(v).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

pub fn same(a: &Store, b: &Store) -> bool {
    canon(a) == canon(b)
}

// ---------- merging ----------

/// Three-way merge of whole notebooks. Lists of things with an `id` merge item by item in O(N);
/// a page changed on both sides in the same spot is kept twice ("… (other device)").
pub fn merge(base: &Store, local: &Store, remote: &Store) -> Result<Store, String> {
    let (b, l, r) = (canon(base), canon(local), canon(remote));
    if l == b || l == r {
        let mut s = remote.clone();
        tidy(&mut s);
        return Ok(s);
    }
    if r == b {
        let mut s = local.clone();
        tidy(&mut s);
        return Ok(s);
    }
    let v = mv(&b, &l, &r, false, None).unwrap_or(l);
    let mut s: Store = serde_json::from_value(v)
        .map_err(|e| tf!("Couldn't combine the changes ({e}); nothing was changed.", e = e))?;
    tidy(&mut s);
    dedupe(&mut s);
    Ok(s)
}

fn tidy(s: &mut Store) {
    rescue_locked(s);
    rekey_study(s);
}

fn reid_page_contents(p: &mut Page) {
    for b in &mut p.blocks {
        b.id = new_id();
    }
    for st in &mut p.strokes {
        st.id = new_id();
    }
    for c in &mut p.cards {
        c.id = new_id();
    }
    for r in &mut p.recordings {
        r.id = new_id();
    }
}

/// A section locked on one device while another still wrote in it comes out of the merge holding the
/// lock and plain pages, and saving would drop those pages (nobody has a key for them). Move them into
/// their own plain section next to it instead.
fn rescue_locked(s: &mut Store) {
    for n in &mut s.notebooks {
        let mut i = 0;
        while i < n.sections.len() {
            let sec = &mut n.sections[i];
            if sec.lock.is_some() && sec.pages.iter().any(|p| p.kind != PageKind::Locked) {
                let mut pages: Vec<Page> = std::mem::take(&mut sec.pages)
                    .into_iter()
                    .filter(|p| p.kind != PageKind::Locked)
                    .collect();
                sec.pages = vec![sec.placeholder()];
                for p in &mut pages {
                    p.id = new_id();
                    p.other_of = None;
                    reid_page_contents(p);
                }
                let copy = Section {
                    id: new_id(),
                    name: format!("{} (other device)", sec.name),
                    color: sec.color,
                    pages,
                    lock: None,
                    locked_files: vec![],
                };
                n.sections.insert(i + 1, copy);
                i += 1;
            }
            i += 1;
        }
    }
}

/// `strict`: inside a page, where a clash means the page is kept twice (`Err`); elsewhere this device's value wins.
/// `field`: the parent object key (used for explicit typed handling of `study`, `trash`, `pages`, `sections`).
fn mv(b: &Value, l: &Value, r: &Value, strict: bool, field: Option<&str>) -> Result<Value, ()> {
    if l == r || r == b {
        return Ok(l.clone());
    }
    if l == b {
        return Ok(r.clone());
    }
    match (l, r) {
        // Explicit handling for `Store::study` map: each entry is a `CardState { bx, due }`, atomic per card.
        (Value::Object(lo), Value::Object(ro)) if field == Some("study") => {
            let empty = Map::new();
            let bo = b.as_object().unwrap_or(&empty);
            let mut out = Map::new();
            for k in lo.keys().chain(ro.keys().filter(|k| !lo.contains_key(*k))) {
                match (bo.get(k), lo.get(k), ro.get(k)) {
                    (Some(bv), Some(lv), Some(rv)) => {
                        let pick = if lv == bv { rv } else { lv };
                        out.insert(k.clone(), pick.clone());
                    }
                    (None, Some(lv), Some(_)) => {
                        out.insert(k.clone(), lv.clone());
                    }
                    (Some(bv), Some(v), None) | (Some(bv), None, Some(v)) => {
                        if v != bv {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                    (None, Some(v), None) | (None, None, Some(v)) => {
                        out.insert(k.clone(), v.clone());
                    }
                    _ => {}
                }
            }
            Ok(Value::Object(out))
        }
        (Value::Object(lo), Value::Object(ro)) => {
            let empty = Map::new();
            let bo = b.as_object().unwrap_or(&empty);
            let mut out = Map::new();
            for k in lo.keys().chain(ro.keys().filter(|k| !lo.contains_key(*k))) {
                match (bo.get(k), lo.get(k), ro.get(k)) {
                    (bv, Some(lv), Some(rv)) => {
                        out.insert(k.clone(), mv(bv.unwrap_or(&Value::Null), lv, rv, strict, Some(k.as_str()))?);
                    }
                    // removed on one side: gone, unless the other side changed it (then keep the change)
                    (Some(bv), Some(v), None) | (Some(bv), None, Some(v)) => {
                        if v != bv {
                            if strict {
                                return Err(());
                            }
                            out.insert(k.clone(), v.clone());
                        }
                    }
                    (None, Some(v), None) | (None, None, Some(v)) => {
                        out.insert(k.clone(), v.clone());
                    }
                    _ => {}
                }
            }
            Ok(Value::Object(out))
        }
        (Value::Array(la), Value::Array(ra)) if ids(la) && ids(ra) => {
            let empty = vec![];
            Ok(Value::Array(merge_ids(
                b.as_array().filter(|a| ids(a)).unwrap_or(&empty),
                la,
                ra,
                strict,
                field,
            )?))
        }
        // `Store::trash` list (no numeric `id`s): keep what either side added, drop what either side removed.
        (Value::Array(la), Value::Array(ra)) if !strict && field == Some("trash") => {
            let empty = vec![];
            let ba = b.as_array().unwrap_or(&empty);
            let mut out: Vec<Value> = la.iter().filter(|v| !ba.contains(v) || ra.contains(v)).cloned().collect();
            out.extend(ra.iter().filter(|v| !ba.contains(v) && !la.contains(v)).cloned());
            Ok(Value::Array(out))
        }
        // `Section::locked_files` manifest (string set): union of both sides so no attachment is lost.
        (Value::Array(la), Value::Array(ra)) if !strict && field == Some("locked_files") => {
            let mut seen = HashSet::new();
            let mut out = vec![];
            for v in la.iter().chain(ra.iter()) {
                if let Some(s) = v.as_str() {
                    if seen.insert(s.to_string()) {
                        out.push(v.clone());
                    }
                }
            }
            Ok(Value::Array(out))
        }
        _ => {
            if strict {
                Err(())
            } else {
                Ok(l.clone())
            }
        }
    }
}

fn id_of(v: &Value) -> Option<u64> {
    v.get("id").and_then(Value::as_u64)
}
fn ids(a: &[Value]) -> bool {
    a.iter().all(|v| id_of(v).is_some())
}

/// Linear-time O(|l| + |r|) three-way merge for ID-keyed arrays (notebooks, sections, pages, blocks, strokes, cards, recordings).
fn merge_ids(
    b: &[Value],
    l: &[Value],
    r: &[Value],
    strict: bool,
    field: Option<&str>,
) -> Result<Vec<Value>, ()> {
    let bm: HashMap<u64, &Value> = b.iter().filter_map(|v| Some((id_of(v)?, v))).collect();
    let rm: HashMap<u64, &Value> = r.iter().filter_map(|v| Some((id_of(v)?, v))).collect();
    let lm: HashMap<u64, &Value> = l.iter().filter_map(|v| Some((id_of(v)?, v))).collect();
    let is_sections = field == Some("sections");
    let is_pages = field == Some("pages");

    let mut kept = Vec::with_capacity(l.len());
    let mut kept_ids = HashSet::with_capacity(l.len());
    for lv in l {
        let i = id_of(lv).unwrap_or_default();
        match (bm.get(&i), rm.get(&i)) {
            (bv, Some(rv)) => {
                let bv = bv.copied().unwrap_or(&Value::Null);
                // A locked section's pages are one encrypted blob: if changed on both sides, keep both.
                let lock = |v: &Value| v.get("lock").cloned().unwrap_or(Value::Null);
                if is_sections && lock(lv) != lock(bv) && lock(rv) != lock(bv) && lock(lv) != lock(rv) {
                    kept.push(lv.clone());
                    kept_ids.insert(i);
                    let copy = section_copy(rv);
                    if let Some(cid) = id_of(&copy) { kept_ids.insert(cid); }
                    kept.push(copy);
                    continue;
                }
                if is_pages {
                    match mv(bv, lv, rv, true, None) {
                        Ok(v) => {
                            kept.push(v);
                            kept_ids.insert(i);
                        }
                        Err(()) => {
                            kept.push(lv.clone());
                            kept_ids.insert(i);
                            let copy = conflict_copy(rv);
                            if let Some(cid) = id_of(&copy) { kept_ids.insert(cid); }
                            kept.push(copy);
                        }
                    }
                } else {
                    kept.push(mv(bv, lv, rv, strict, None)?);
                    kept_ids.insert(i);
                }
            }
            (Some(bv), None) => {
                if lv != *bv {
                    kept.push(lv.clone());
                    kept_ids.insert(i);
                }
            }
            (None, None) => {
                kept.push(lv.clone());
                kept_ids.insert(i);
            }
        }
    }

    // Collect remote-only additions/re-creations keyed by their nearest preceding item already in `kept`.
    let mut front: Vec<Value> = vec![];
    let mut after: HashMap<u64, Vec<Value>> = HashMap::new();
    let mut last_anchor: Option<u64> = None;
    for rv in r {
        let i = id_of(rv).unwrap_or_default();
        if kept_ids.contains(&i) {
            last_anchor = Some(i);
            continue;
        }
        if lm.contains_key(&i) {
            continue;
        }
        if bm.get(&i).is_some_and(|bv| *bv == rv) {
            continue;
        }
        match last_anchor {
            Some(anchor) => after.entry(anchor).or_default().push(rv.clone()),
            None => front.push(rv.clone()),
        }
    }

    let mut out = Vec::with_capacity(kept.len() + front.len() + after.values().map(Vec::len).sum::<usize>());
    out.extend(front);
    for v in kept {
        let i = id_of(&v).unwrap_or_default();
        out.push(v);
        if let Some(extra) = after.remove(&i) {
            out.extend(extra);
        }
    }
    Ok(out)
}

/// The other device's version of a locked section that both changed, as its own section.
fn section_copy(v: &Value) -> Value {
    let mut c = v.clone();
    let id = new_id();
    c["id"] = id.into();
    c["name"] = format!("{} (other device)", c["name"].as_str().unwrap_or("")).into();
    if let Some(p) = c["pages"].as_array_mut().and_then(|a| a.first_mut()) {
        p["id"] = id.into();
    }
    c
}

/// The other device's version of a page that both changed, as its own page with fresh inner IDs.
fn conflict_copy(v: &Value) -> Value {
    let mut c = v.clone();
    c["other_of"] = c["id"].clone();
    c["id"] = new_id().into();
    let t = c["title"].as_str().unwrap_or("").to_string();
    c["title"] = format!("{} (other device)", if t.trim().is_empty() { "Untitled page" } else { &t }).into();
    for list in ["blocks", "strokes", "cards", "recordings"] {
        if let Some(a) = c[list].as_array_mut() {
            for x in a {
                x["id"] = new_id().into();
            }
        }
    }
    c
}

/// Give every notebook, section, page, and duplicated page's inner items their own unique ID.
pub(crate) fn dedupe(s: &mut Store) {
    let mut seen = HashSet::new();
    for n in &mut s.notebooks {
        if !seen.insert(n.id) {
            n.id = new_id();
        }
        for sec in &mut n.sections {
            if !seen.insert(sec.id) {
                sec.id = new_id();
            }
            let sid = sec.id;
            for p in &mut sec.pages {
                if p.kind == PageKind::Locked {
                    p.id = sid;
                } else if !seen.insert(p.id) {
                    p.id = new_id();
                    reid_page_contents(p);
                }
            }
        }
    }
}

// ---------- where the shared copy lives ----------

enum Remote {
    Folder(PathBuf),
    Dav {
        agent: ureq::Agent,
        dir: String,
        auth: String,
    },
    /// Google Drive: "Leaf Notes" root folder, plus its "pages" and "files" subfolders,
    /// with a per-folder listing cache so a sync round uses ~1 list query per folder instead of N+1.
    Drive {
        agent: ureq::Agent,
        token: String,
        folder: String,
        pages: String,
        files: String,
        listing: RefCell<DriveFolderListing>,
    },
}

enum Got {
    Missing,
    Same,
    Data(Vec<u8>, String),
}
enum Put {
    Done(String),
    Changed,
}

fn hash(b: &[u8]) -> String {
    // FNV-1a: stable across runs and Rust versions (the tag is saved)
    let h = b
        .iter()
        .fold(0xcbf29ce484222325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100000001b3));
    format!("{h:016x}-{}", b.len())
}

/// Exclusive lockfile guard for Folder sync to eliminate check-then-rename TOCTOU races.
struct FolderLock(PathBuf);

impl FolderLock {
    fn acquire(dir: &Path) -> Result<Self, String> {
        let path = dir.join(".sync.lock");
        for _ in 0..10 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Clean up stale lock left by a crashed process (> 120s old).
                    let stale = std::fs::metadata(&path)
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
                        .is_some_and(|age| age.as_secs() > 120);
                    if stale {
                        let _ = std::fs::remove_file(&path);
                    } else {
                        std::thread::sleep(Duration::from_millis(80));
                    }
                }
                Err(e) => return Err(tf!("Can't lock sync folder: {e}", e = e)),
            }
        }
        Err(crate::tr("Another device kept syncing at the same moment — trying again shortly.").into())
    }
}

impl Drop for FolderLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Remote {
    fn new(s: &SyncSettings, quick: bool) -> Result<Remote, String> {
        match s.via {
            Via::Off => Err(crate::tr("Sync is off.").into()),
            Via::Folder => {
                let root = PathBuf::from(s.folder.trim());
                if !root.is_dir() {
                    return Err(tf!("The folder {root} doesn't exist.", root = root.display()));
                }
                let d = root.join("LeafNotes");
                std::fs::create_dir_all(d.join("files"))
                    .and_then(|_| std::fs::create_dir_all(d.join("pages")))
                    .map_err(|e| tf!("Can't write to {d}: {e}", d = d.display(), e = e))?;
                Ok(Remote::Folder(d))
            }
            Via::Google => {
                let agent = agent(quick);
                let token = google_access(&agent, &s.google_token)?;
                let mut r = Remote::Drive {
                    agent,
                    token,
                    folder: String::new(),
                    pages: String::new(),
                    files: String::new(),
                    listing: RefCell::new(HashMap::new()),
                };
                let folder = r.drive_folder("Leaf Notes", "root")?;
                let files = r.drive_folder("files", &folder)?;
                let pages = r.drive_folder("pages", &folder)?;
                if let Remote::Drive { folder: f, pages: pg, files: fl, .. } = &mut r {
                    (*f, *pg, *fl) = (folder, pages, files);
                }
                Ok(r)
            }
            Via::WebDav => {
                use base64::Engine;
                let url = s.url.trim().trim_end_matches('/');
                validate_http_url(url, s.allow_lan_http)?;
                let agent = agent(quick);
                let auth = format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", s.user.trim(), s.password))
                );
                let r = Remote::Dav { agent, dir: format!("{url}/LeafNotes/"), auth };
                r.mkcol("")?;
                r.mkcol("files/")?;
                r.mkcol("pages/")?;
                Ok(r)
            }
        }
    }

    fn dav(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<Vec<u8>>,
    ) -> Result<ureq::http::Response<ureq::Body>, String> {
        let Remote::Dav { agent, dir, auth } = self else { unreachable!() };
        let mut b = ureq::http::Request::builder()
            .method(method)
            .uri(format!("{dir}{path}"))
            .header("authorization", auth.as_str());
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        let r = match body {
            Some(data) => agent.run(b.body(data).map_err(|e| e.to_string())?),
            None => agent.run(b.body(()).map_err(|e| e.to_string())?),
        };
        let resp = r.map_err(|e| tf!("Couldn't reach the sync server: {e}", e = e))?;
        match resp.status().as_u16() {
            401 | 403 => Err(crate::tr("The sync server refused the user name or password.").into()),
            507 => Err(crate::tr("The sync server is full.").into()),
            _ => Ok(resp),
        }
    }

    fn mkcol(&self, path: &str) -> Result<(), String> {
        let s = self.dav("MKCOL", path, &[], None)?.status().as_u16();
        match s {
            200..=299 | 301 | 405 => Ok(()),
            404 | 409 => Err(crate::tr("The server address doesn't point to a folder on the server.").into()),
            _ => Err(tf!("The sync server answered {s} when creating its folder.", s = s)),
        }
    }

    fn get_named_root(&self, name: &str, known: Option<&str>) -> Result<Got, String> {
        match self {
            Remote::Folder(d) => match std::fs::read(d.join(name)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Got::Missing),
                Err(e) => Err(tf!("Couldn't read the synced notes: {e}", e = e)),
                Ok(b) => {
                    let t = hash(&b);
                    Ok(if known == Some(t.as_str()) { Got::Same } else { Got::Data(b, t) })
                }
            },
            Remote::Drive { folder, .. } => match self.drive_find_live(name, folder)? {
                None => Ok(Got::Missing),
                Some((_, v)) if known == Some(v.as_str()) => Ok(Got::Same),
                Some((id, v)) => {
                    let mut resp = self.gd("GET", &format!("{API}/files/{id}?alt=media"), None)?;
                    if resp.status().as_u16() != 200 {
                        return Err(tf!("Google Drive answered {status} when reading the notes.", status = resp.status()));
                    }
                    let b = resp
                        .body_mut()
                        .with_config()
                        .limit(1 << 31)
                        .read_to_vec()
                        .map_err(|e| tf!("The download of the notes broke off: {e}", e = e))?;
                    Ok(Got::Data(b, v))
                }
            },
            Remote::Dav { .. } => {
                let h: Vec<(&str, &str)> = known.map(|t| vec![("if-none-match", t)]).unwrap_or_default();
                let mut resp = self.dav("GET", name, &h, None)?;
                match resp.status().as_u16() {
                    404 => Ok(Got::Missing),
                    304 => Ok(Got::Same),
                    200 => {
                        let tag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                        let b = resp
                            .body_mut()
                            .with_config()
                            .limit(1 << 31)
                            .read_to_vec()
                            .map_err(|e| tf!("The download of the notes broke off: {e}", e = e))?;
                        Ok(Got::Data(b, tag))
                    }
                    s => Err(tf!("The sync server answered {s} when reading the notes.", s = s)),
                }
            }
        }
    }

    fn get_library(&self, known: Option<&str>) -> Result<Got, String> {
        self.get_named_root(LIBRARY_FILE, known)
    }

    fn get_legacy_notes(&self, known: Option<&str>) -> Result<Got, String> {
        self.get_named_root(LEGACY_NOTES, known)
    }

    /// Write `library.json` only if the shared copy is still at version `expect` (`None`: missing).
    fn put_library(&self, bytes: &[u8], expect: Option<&str>) -> Result<Put, String> {
        match self {
            Remote::Folder(d) => {
                let _guard = FolderLock::acquire(d)?;
                let path = d.join(LIBRARY_FILE);
                let now = std::fs::read(&path).ok();
                if now.as_deref().map(hash).as_deref() != expect {
                    return Ok(Put::Changed);
                }
                if let Some(prev) = &now {
                    let _ = write_atomic(&d.join("notes.prev.json"), prev);
                }
                write_atomic(&path, bytes).map_err(|e| tf!("Couldn't write the synced notes: {e}", e = e))?;
                Ok(Put::Done(hash(bytes)))
            }
            Remote::Drive { folder, .. } => {
                let now = self.drive_find_live(LIBRARY_FILE, folder)?;
                if now.as_ref().map(|(_, v)| v.as_str()) != expect {
                    return Ok(Put::Changed);
                }
                let meta = self.drive_upload(now.as_ref().map(|(id, _)| id.as_str()), LIBRARY_FILE, folder, bytes.to_vec())?;
                Ok(Put::Done(meta["version"].as_str().unwrap_or("").to_string()))
            }
            Remote::Dav { .. } => {
                let h: Vec<(&str, &str)> = match expect {
                    None => vec![("if-none-match", "*")],
                    Some("") => vec![],
                    Some(t) => vec![("if-match", t)],
                };
                let resp = self.dav("PUT", LIBRARY_FILE, &h, Some(bytes.to_vec()))?;
                match resp.status().as_u16() {
                    200..=299 => Ok(Put::Done(
                        resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string(),
                    )),
                    412 => Ok(Put::Changed),
                    s => Err(tf!("The sync server answered {s} when saving the notes.", s = s)),
                }
            }
        }
    }

    fn get_page(&self, id: u64) -> Result<Option<Vec<u8>>, String> {
        let name = format!("{id}.json");
        match self {
            Remote::Folder(d) => match std::fs::read(d.join("pages").join(&name)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(tf!("Couldn't read the synced notes: {e}", e = e)),
                Ok(b) => Ok(Some(b)),
            },
            Remote::Drive { pages, .. } => {
                let Some((fid, _)) = self.drive_find_cached(&name, pages)? else { return Ok(None) };
                let mut resp = self.gd("GET", &format!("{API}/files/{fid}?alt=media"), None)?;
                if resp.status().as_u16() != 200 {
                    return Err(tf!("Google Drive answered {status} when reading the notes.", status = resp.status()));
                }
                let b = resp
                    .body_mut()
                    .with_config()
                    .limit(1 << 28)
                    .read_to_vec()
                    .map_err(|e| tf!("The download of the notes broke off: {e}", e = e))?;
                Ok(Some(b))
            }
            Remote::Dav { .. } => {
                let mut resp = self.dav("GET", &format!("pages/{name}"), &[], None)?;
                match resp.status().as_u16() {
                    404 => Ok(None),
                    200 => {
                        let b = resp
                            .body_mut()
                            .with_config()
                            .limit(1 << 28)
                            .read_to_vec()
                            .map_err(|e| tf!("The download of the notes broke off: {e}", e = e))?;
                        Ok(Some(b))
                    }
                    s => Err(tf!("The sync server answered {s} when reading the notes.", s = s)),
                }
            }
        }
    }

    fn put_page(&self, id: u64, bytes: &[u8]) -> Result<(), String> {
        let name = format!("{id}.json");
        match self {
            Remote::Folder(d) => write_atomic(&d.join("pages").join(&name), bytes)
                .map_err(|e| tf!("Couldn't write the synced notes: {e}", e = e)),
            Remote::Drive { pages, .. } => {
                let existing = self.drive_find_cached(&name, pages)?;
                self.drive_upload(existing.as_ref().map(|(fid, _)| fid.as_str()), &name, pages, bytes.to_vec())
                    .map(|_| ())
            }
            Remote::Dav { .. } => {
                let s = self.dav("PUT", &format!("pages/{name}"), &[], Some(bytes.to_vec()))?.status().as_u16();
                if (200..300).contains(&s) {
                    Ok(())
                } else {
                    Err(tf!("The sync server answered {s} when saving the notes.", s = s))
                }
            }
        }
    }

    fn delete_page(&self, id: u64) -> Result<(), String> {
        let name = format!("{id}.json");
        match self {
            Remote::Folder(d) => {
                let _ = std::fs::remove_file(d.join("pages").join(name));
                Ok(())
            }
            Remote::Drive { pages, .. } => self.drive_delete_cached(&name, pages),
            Remote::Dav { .. } => {
                let _ = self.dav("DELETE", &format!("pages/{name}"), &[], None);
                Ok(())
            }
        }
    }

    fn put_file(&self, name: &str, from: &Path) -> Result<(), String> {
        match self {
            Remote::Folder(d) => {
                let to = d.join("files").join(name);
                if to.exists() {
                    return Ok(());
                }
                let tmp = PathBuf::from(format!("{}.part", to.display()));
                std::fs::copy(from, &tmp)
                    .and_then(|_| std::fs::rename(&tmp, &to))
                    .map_err(|e| tf!("Couldn't copy {name}: {e}", name = name, e = e))
            }
            Remote::Drive { files, .. } => {
                if self.drive_find_cached(name, files)?.is_some() {
                    return Ok(());
                }
                let data = std::fs::read(from).map_err(|e| tf!("Couldn't read {name}: {e}", name = name, e = e))?;
                self.drive_upload(None, name, files, data).map(|_| ())
            }
            Remote::Dav { .. } => {
                let data = std::fs::read(from).map_err(|e| tf!("Couldn't read {name}: {e}", name = name, e = e))?;
                let s = self.dav("PUT", &format!("files/{name}"), &[], Some(data))?.status().as_u16();
                if (200..300).contains(&s) {
                    Ok(())
                } else {
                    Err(tf!("The sync server answered {s} when uploading {name}.", s = s, name = name))
                }
            }
        }
    }

    fn delete_file(&self, name: &str) -> Result<(), String> {
        if !safe_name(name) {
            return Ok(());
        }
        match self {
            Remote::Folder(d) => {
                let _ = std::fs::remove_file(d.join("files").join(name));
                Ok(())
            }
            Remote::Drive { files, .. } => self.drive_delete_cached(name, files),
            Remote::Dav { .. } => {
                let _ = self.dav("DELETE", &format!("files/{name}"), &[], None);
                Ok(())
            }
        }
    }

    /// Download into place via a temporary file; false if the shared copy doesn't have it (yet).
    fn get_file(&self, name: &str, to: &Path) -> Result<bool, String> {
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = PathBuf::from(format!("{}.part", to.display()));
        let r = match self {
            Remote::Folder(d) => {
                let from = d.join("files").join(name);
                if !from.exists() {
                    return Ok(false);
                }
                std::fs::copy(&from, &tmp)
                    .map(|_| ())
                    .map_err(|e| tf!("Couldn't copy {name}: {e}", name = name, e = e))
            }
            Remote::Drive { files, .. } => {
                let Some((id, _)) = self.drive_find_cached(name, files)? else { return Ok(false) };
                let resp = self.gd("GET", &format!("{API}/files/{id}?alt=media"), None)?;
                if resp.status().as_u16() != 200 {
                    return Err(tf!("Google Drive answered {status} when downloading {name}.", status = resp.status(), name = name));
                }
                let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
                std::io::copy(&mut resp.into_body().into_reader().take(1 << 33), &mut f)
                    .map(|_| ())
                    .map_err(|e| tf!("The download of {name} broke off: {e}", name = name, e = e))
            }
            Remote::Dav { .. } => {
                let resp = self.dav("GET", &format!("files/{name}"), &[], None)?;
                match resp.status().as_u16() {
                    404 => return Ok(false),
                    200 => {
                        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
                        std::io::copy(&mut resp.into_body().into_reader().take(1 << 33), &mut f)
                            .map(|_| ())
                            .map_err(|e| tf!("The download of {name} broke off: {e}", name = name, e = e))
                    }
                    s => Err(tf!("The sync server answered {s} when downloading {name}.", s = s, name = name)),
                }
            }
        };
        match r.and_then(|_| {
            storage::set_private_perms(&tmp);
            std::fs::rename(&tmp, to).map_err(|e| e.to_string())
        }) {
            Ok(()) => Ok(true),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(e)
            }
        }
    }
}

fn agent(quick: bool) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(if quick { 5 } else { 20 })))
        .timeout_global(Some(Duration::from_secs(if quick { 15 } else { 600 })))
        .build()
        .into()
}

// ---------- Google Drive ----------

const GOOGLE_ID: &str = "304608478286-2n9iukrh6bg19fa0r7v21fhk3ru3kqm4.apps.googleusercontent.com";
const GOOGLE_SECRET: &str = match option_env!("LEAFNOTES_GOOGLE_SECRET") {
    Some(s) => s,
    None => "",
};
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file openid email";
const API: &str = "https://www.googleapis.com/drive/v3";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

pub fn google_ready() -> bool {
    !GOOGLE_SECRET.is_empty()
}

impl Remote {
    fn gd(&self, method: &str, url: &str, body: Option<(&str, Vec<u8>)>) -> Result<ureq::http::Response<ureq::Body>, String> {
        let Remote::Drive { agent, token, .. } = self else { unreachable!() };
        let b = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("authorization", format!("Bearer {token}", token = token));
        let r = match body {
            Some((ct, data)) => agent.run(b.header("content-type", ct).body(data).map_err(|e| e.to_string())?),
            None => agent.run(b.body(()).map_err(|e| e.to_string())?),
        };
        let resp = r.map_err(|e| tf!("Couldn't reach Google Drive: {e}", e = e) + offline_hint(&e))?;
        match resp.status().as_u16() {
            401 => Err(crate::tr("Google Drive refused the sign-in — sign in again in Settings ▸ Sync.").into()),
            403 => Err(crate::tr("Google Drive refused access (is the Drive API on for this app, or is the Drive full?).").into()),
            429 => Err(crate::tr("Google Drive asks to slow down — trying again shortly.").into()),
            _ => Ok(resp),
        }
    }

    fn json(&self, method: &str, url: &str, body: Option<Value>) -> Result<Value, String> {
        let mut resp = self.gd(method, url, body.map(|v| ("application/json; charset=UTF-8", v.to_string().into_bytes())))?;
        let st = resp.status().as_u16();
        let v: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| tf!("Google Drive sent an unexpected answer ({st}): {e}", st = st, e = e))?;
        if (200..300).contains(&st) {
            Ok(v)
        } else {
            Err(tf!("Google Drive answered {st}: {x}", st = st, x = v["error"]["message"].as_str().unwrap_or("")))
        }
    }

    /// Live lookup for a single file (used for optimistic concurrency check on `library.json`).
    fn drive_find_live(&self, name: &str, parent: &str) -> Result<Option<(String, String)>, String> {
        let q = format!("name='{name}' and '{parent}' in parents and trashed=false");
        let v = self.json(
            "GET",
            &format!("{API}/files?q={}&orderBy=createdTime&fields=files(id,version)&pageSize=10&spaces=drive", enc(&q)),
            None,
        )?;
        Ok(v["files"].get(0).map(|f| {
            (
                f["id"].as_str().unwrap_or("").to_string(),
                f["version"].as_str().unwrap_or("").to_string(),
            )
        }))
    }

    /// Batch-list all files in a Drive folder once (paginated) and cache them by name.
    fn drive_ensure_listed(&self, parent: &str) -> Result<(), String> {
        let Remote::Drive { listing, .. } = self else { unreachable!() };
        if listing.borrow().contains_key(parent) {
            return Ok(());
        }
        let mut map = HashMap::new();
        let mut page_token: Option<String> = None;
        let q = format!("'{parent}' in parents and trashed=false");
        loop {
            let mut url = format!(
                "{API}/files?q={}&orderBy=createdTime&fields=nextPageToken,files(id,name,version)&pageSize=1000&spaces=drive",
                enc(&q)
            );
            if let Some(tok) = &page_token {
                url.push_str("&pageToken=");
                url.push_str(&enc(tok));
            }
            let v = self.json("GET", &url, None)?;
            for f in v["files"].as_array().into_iter().flatten() {
                let (Some(name), Some(id)) = (f["name"].as_str(), f["id"].as_str()) else { continue };
                let ver = f["version"].as_str().unwrap_or("").to_string();
                // Keep oldest matching entry so every device picks the same file if duplicates ever exist.
                map.entry(name.to_string()).or_insert((id.to_string(), ver));
            }
            page_token = v["nextPageToken"].as_str().map(String::from).filter(|s| !s.is_empty());
            if page_token.is_none() {
                break;
            }
        }
        listing.borrow_mut().insert(parent.to_string(), map);
        Ok(())
    }

    fn drive_find_cached(&self, name: &str, parent: &str) -> Result<Option<(String, String)>, String> {
        self.drive_ensure_listed(parent)?;
        let Remote::Drive { listing, .. } = self else { unreachable!() };
        Ok(listing.borrow().get(parent).and_then(|m| m.get(name)).cloned())
    }

    fn drive_delete_cached(&self, name: &str, parent: &str) -> Result<(), String> {
        let Some((id, _)) = self.drive_find_cached(name, parent)? else { return Ok(()) };
        let _ = self.gd("DELETE", &format!("{API}/files/{id}"), None);
        if let Remote::Drive { listing, .. } = self {
            if let Some(m) = listing.borrow_mut().get_mut(parent) {
                m.remove(name);
            }
        }
        Ok(())
    }

    fn drive_folder(&self, name: &str, parent: &str) -> Result<String, String> {
        if let Some((id, _)) = self.drive_find_live(name, parent)? {
            return Ok(id);
        }
        let v = self.json(
            "POST",
            &format!("{API}/files?fields=id"),
            Some(serde_json::json!({ "name": name, "mimeType": "application/vnd.google-apps.folder", "parents": [parent] })),
        )?;
        v["id"].as_str().map(String::from).ok_or(crate::tr("Google Drive didn't create the folder.").into())
    }

    /// Resumable upload (works for any size): a new file, or new contents for `existing`.
    fn drive_upload(&self, existing: Option<&str>, name: &str, parent: &str, data: Vec<u8>) -> Result<Value, String> {
        let up = "https://www.googleapis.com/upload/drive/v3/files";
        let (method, url, meta) = match existing {
            Some(id) => ("PATCH", format!("{up}/{id}?uploadType=resumable&fields=id,version"), serde_json::json!({})),
            None => ("POST", format!("{up}?uploadType=resumable&fields=id,version"), serde_json::json!({ "name": name, "parents": [parent] })),
        };
        let resp = self.gd(method, &url, Some(("application/json; charset=UTF-8", meta.to_string().into_bytes())))?;
        let st = resp.status().as_u16();
        let loc = resp.headers().get("location").and_then(|v| v.to_str().ok()).map(String::from);
        let Some(loc) = loc.filter(|_| st == 200) else {
            return Err(tf!("Google Drive answered {st} when starting the upload of {name}.", st = st, name = name));
        };
        let mut resp = self.gd("PUT", &loc, Some(("application/octet-stream", data)))?;
        let st = resp.status().as_u16();
        if !(200..300).contains(&st) {
            return Err(tf!("Google Drive answered {st} when uploading {name}.", st = st, name = name));
        }
        let v: Value = resp.body_mut().read_json().map_err(|e| e.to_string())?;
        if let Remote::Drive { listing, .. } = self {
            if let (Some(id), Some(m)) = (v["id"].as_str(), listing.borrow_mut().get_mut(parent)) {
                m.insert(name.to_string(), (id.to_string(), v["version"].as_str().unwrap_or("").to_string()));
            }
        }
        Ok(v)
    }
}

/// A short-lived access token from the saved sign-in.
fn google_access(agent: &ureq::Agent, refresh: &str) -> Result<String, String> {
    let mut resp = agent
        .post(TOKEN_URL)
        .send_form([
            ("client_id", GOOGLE_ID),
            ("client_secret", GOOGLE_SECRET),
            ("refresh_token", refresh),
            ("grant_type", "refresh_token"),
        ])
        .map_err(|e| tf!("Couldn't reach Google: {e}", e = e))?;
    let v: Value = resp.body_mut().read_json().map_err(|e| tf!("Google sent an unexpected answer: {e}", e = e))?;
    match v["access_token"].as_str() {
        Some(t) => Ok(t.to_string()),
        None if v["error"] == "invalid_grant" => {
            Err(crate::tr("Your Google sign-in expired or was removed — sign in again in Settings ▸ Sync.").into())
        }
        None => Err(tf!(
            "Google refused the sign-in: {x}",
            x = v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("unknown error")
        )),
    }
}

fn offline_hint(e: &impl std::fmt::Display) -> &'static str {
    if cfg!(target_os = "android") && e.to_string().contains("lookup address") {
        crate::tr(" Android may be keeping Leaf Notes off the internet: check Settings ▸ Apps ▸ Leaf Notes ▸ Mobile data & Wi-Fi, and Data Saver.")
    } else {
        ""
    }
}

pub struct SignIn {
    listener: std::net::TcpListener,
    verifier: String,
    state: String,
    redirect: String,
    paste: std::sync::mpsc::Receiver<String>,
}

/// Start signing in: returns the address to open in the browser, and where a pasted address goes.
pub fn google_start() -> Result<(String, SignIn, std::sync::mpsc::Sender<String>), String> {
    use base64::Engine;
    use sha2::Digest;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| tf!("Couldn't start the sign-in: {e}", e = e))?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr().map_err(|e| e.to_string())?.port());
    let rand = |n: usize| -> Result<String, String> {
        let mut b = vec![0u8; n];
        getrandom::fill(&mut b).map_err(|e| e.to_string())?;
        Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b))
    };
    let (verifier, state) = (rand(48)?, rand(16)?);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}&access_type=offline&prompt=consent",
        enc(GOOGLE_ID),
        enc(&redirect),
        enc(SCOPE)
    );
    let (tx, paste) = std::sync::mpsc::channel();
    Ok((url, SignIn { listener, verifier, state, redirect, paste }, tx))
}

/// The code in an address Google sent back: Some(code) for this sign-in, None when it belongs to
/// another (a stray request, an old tab), an error when Google said no.
fn redirect_code(address: &str, state: &str) -> Result<Option<String>, String> {
    let q: HashMap<String, String> = address
        .split_once('?')
        .map(|(_, q)| q)
        .unwrap_or("")
        .split(['&', '#'])
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), dec(v)))
        .collect();
    if q.get("state").map(String::as_str) != Some(state) {
        return Ok(None);
    }
    match q.get("code") {
        Some(c) => Ok(Some(c.clone())),
        None => Err(if q.get("error").map(String::as_str) == Some("access_denied") {
            crate::tr("Sign-in was cancelled.").into()
        } else {
            crate::tr("Google didn't sign you in — try again.").into()
        }),
    }
}

/// Read an HTTP request header block completely (looping until `\r\n\r\n` or limit),
/// avoiding truncation on fragmented loopback reads.
fn read_http_request(conn: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 2048];
    while buf.len() < 16_384 {
        match conn.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Wait (up to 5 minutes) for the browser to come back, then trade the code for a lasting sign-in: (refresh token, email).
pub fn google_finish(s: SignIn) -> Result<(String, String), String> {
    use std::io::Write;
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    s.listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let code = loop {
        if std::time::Instant::now() > deadline {
            return Err(crate::tr("Sign-in timed out — try again.").into());
        }
        let (mut conn, _) = match s.listener.accept() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                match s.paste.try_recv() {
                    Ok(address) => match redirect_code(address.trim(), &s.state)? {
                        Some(c) => break c,
                        None => return Err(crate::tr("That address is from another sign-in — sign in again.").into()),
                    },
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return Err(crate::tr("Sign-in was cancelled.").into()),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = conn.set_nonblocking(false);
        let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
        let req = read_http_request(&mut conn);
        let path = req.split_whitespace().nth(1).unwrap_or("");
        if !path.contains('?') {
            let _ = conn.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        }
        let found = redirect_code(path, &s.state);
        let ok = matches!(found, Ok(Some(_)));
        let msg = if ok {
            "Signed in. You can close this tab and go back to Leaf Notes."
        } else {
            "Sign-in didn't finish. Go back to Leaf Notes and try again."
        };
        let page = format!(
            "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>Leaf Notes</title><body style=\"font:18px sans-serif;margin:15vh auto;max-width:30em;text-align:center\"><h2>Leaf Notes</h2><p>{msg}</p>"
        );
        let _ = conn.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            )
            .as_bytes(),
        );
        if let Some(c) = found? {
            break c;
        }
    };
    let until = std::time::Instant::now() + Duration::from_secs(180);
    let ag = agent(false);
    let mut resp = loop {
        match ag.post(TOKEN_URL).send_form([
            ("client_id", GOOGLE_ID),
            ("client_secret", GOOGLE_SECRET),
            ("code", code.as_str()),
            ("code_verifier", s.verifier.as_str()),
            ("redirect_uri", s.redirect.as_str()),
            ("grant_type", "authorization_code"),
        ]) {
            Ok(r) => break r,
            Err(e) if std::time::Instant::now() >= until => {
                return Err(tf!("Couldn't reach Google: {e}", e = e) + offline_hint(&e));
            }
            Err(_) => {
                if matches!(s.paste.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)) {
                    return Err(crate::tr("Sign-in was cancelled.").into());
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    };
    let v: Value = resp.body_mut().read_json().map_err(|e| tf!("Google sent an unexpected answer: {e}", e = e))?;
    let Some(refresh) = v["refresh_token"].as_str() else {
        return Err(tf!(
            "Google didn't sign you in: {x}",
            x = v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("no lasting sign-in was given")
        ));
    };
    // Verify identity via Google's userinfo endpoint using the fresh access token (fallback to id_token email).
    let email = v["access_token"]
        .as_str()
        .and_then(|at| google_verified_email(&ag, at))
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| id_email(v["id_token"].as_str().unwrap_or("")));
    Ok((refresh.to_string(), email))
}

fn google_verified_email(agent: &ureq::Agent, access_token: &str) -> Option<String> {
    let req = ureq::http::Request::builder()
        .method("GET")
        .uri("https://openidconnect.googleapis.com/v1/userinfo")
        .header("authorization", format!("Bearer {access_token}"))
        .body(())
        .ok()?;
    let mut resp = agent.run(req).ok()?;
    if resp.status().as_u16() != 200 {
        return None;
    }
    let v: Value = resp.body_mut().read_json().ok()?;
    v["email"].as_str().map(String::from)
}

/// Forget the sign-in at Google too (best effort).
pub fn google_revoke(refresh: &str) {
    let _ = agent(true)
        .post("https://oauth2.googleapis.com/revoke")
        .send_form([("token", refresh)]);
}

/// The email inside Google's id token (fallback if userinfo request fails).
fn id_email(jwt: &str) -> String {
    use base64::Engine;
    jwt.split('.')
        .nth(1)
        .and_then(|p| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p.trim_end_matches('=')).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["email"].as_str().map(String::from))
        .unwrap_or_default()
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                Ok(x) => {
                    out.push(x);
                    i += 3;
                    continue;
                }
                Err(_) => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_code_from_a_pasted_address() {
        let a = "http://127.0.0.1:33669/?state=abc&code=4%2F0Ab_x-y&scope=email%20openid";
        assert_eq!(redirect_code(a, "abc"), Ok(Some("4/0Ab_x-y".into())));
        assert_eq!(redirect_code(a, "other"), Ok(None), "another sign-in's address");
        assert_eq!(redirect_code("http://127.0.0.1:1/favicon.ico", "abc"), Ok(None));
        assert!(redirect_code("http://127.0.0.1:1/?error=access_denied&state=abc", "abc").is_err());
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ln-sync-{tag}-{}", new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn page_mut<'a>(s: &'a mut Store, title: &str) -> &'a mut Page {
        s.notebooks.iter_mut().flat_map(|n| &mut n.sections).flat_map(|s| &mut s.pages).find(|p| p.title == title).unwrap()
    }
    fn titles(s: &Store) -> Vec<String> {
        s.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).map(|p| p.title.clone()).collect()
    }
    fn text(p: &Page) -> String {
        p.plain_text()
    }

    #[test]
    fn merges_note_by_note() {
        let base = sample();
        let (mut l, mut r) = (base.clone(), base.clone());
        page_mut(&mut l, "Week 1 — Intro").title = crate::tr("Week 1 — Introduction").into();
        l.notebooks[1].sections[0].pages.clear();
        page_mut(&mut r, "Week 2 — Convolution").blocks.push(Block {
            id: 7,
            pos: [0.0, 0.0],
            width: 300.0,
            tag: Default::default(),
            kind: Kind::Text { text: "flip and slide".into(), heading: 0 },
            t: 0.0,
        });
        r.notebooks[0].sections[0].pages.push(Page::new("Week 3"));
        r.study.insert("card1".into(), CardState { bx: 2, due: 5 });
        let m = merge(&base, &l, &r).unwrap();
        let t = titles(&m);
        assert!(
            t.contains(&crate::tr("Week 1 — Introduction").to_string()) && t.contains(&"Week 3".to_string()) && !t.contains(&"Groceries".to_string()),
            "{t:?}"
        );
        assert!(text(page_mut(&mut m.clone(), "Week 2 — Convolution")).contains("flip and slide"));
        assert_eq!(m.study.get("card1"), Some(&CardState { bx: 2, due: 5 }));

        // The same text block edited on both sides: the page is kept twice, nothing lost, and inner IDs regenerated.
        let (mut l2, mut r2) = (m.clone(), m.clone());
        for (s, w) in [(&mut l2, "here"), (&mut r2, "there")] {
            if let Kind::Text { text, .. } = &mut page_mut(s, "Week 2 — Convolution").blocks.last_mut().unwrap().kind {
                *text = w.into();
            }
        }
        let m2 = merge(&m, &l2, &r2).unwrap();
        let t2 = titles(&m2);
        assert!(t2.contains(&crate::tr("Week 2 — Convolution (other device)").to_string()), "{t2:?}");
        let all: String = m2.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).map(text).collect();
        assert!(all.contains("here") && all.contains("there"));

        // Deleted here but edited there: kept.
        let (mut l3, mut r3) = (m.clone(), m.clone());
        l3.notebooks[0].sections[0].pages.retain(|p| p.title != "Week 3");
        page_mut(&mut r3, "Week 3").title = crate::tr("Week 3 — Sampling").into();
        assert!(titles(&merge(&m, &l3, &r3).unwrap()).contains(&crate::tr("Week 3 — Sampling").to_string()));

        // Both sides added to Recently deleted: both kept.
        let (mut l4, mut r4) = (m.clone(), m.clone());
        l4.trash.push(Trashed { label: "a".into(), deleted: 1, item: TrashItem::Notebook(l4.notebooks[1].clone()), at: None });
        r4.trash.push(Trashed { label: "b".into(), deleted: 2, item: TrashItem::Notebook(r4.notebooks[1].clone()), at: None });
        assert_eq!(merge(&m, &l4, &r4).unwrap().trash.len(), 2);

        // A locked section changed on both devices: both encrypted versions are kept.
        let mut b5 = m.clone();
        let sec = &mut b5.notebooks[1].sections[0];
        sec.lock = Some(Lock { v: 1, salt: "s".into(), nonce: "n0".into(), data: "d0".into() });
        sec.pages = vec![sec.placeholder()];
        let (mut l5, mut r5) = (b5.clone(), b5.clone());
        l5.notebooks[1].sections[0].lock.as_mut().unwrap().data = "here".into();
        r5.notebooks[1].sections[0].lock.as_mut().unwrap().data = "there".into();
        let m5 = merge(&b5, &l5, &r5).unwrap();
        let secs = &m5.notebooks[1].sections;
        assert_eq!(secs.len(), 2);
        assert!(secs[1].name.ends_with("(other device)") && secs[1].pages[0].id == secs[1].id);
        assert_eq!(
            (secs[0].lock.as_ref().unwrap().data.as_str(), secs[1].lock.as_ref().unwrap().data.as_str()),
            ("here", "there")
        );
    }

    #[test]
    fn edits_survive_a_lock_from_another_device() {
        let base = sample();
        let locked = |s: &mut Store| {
            let sec = &mut s.notebooks[0].sections[0];
            sec.lock = Some(Lock { v: 1, salt: "s".into(), nonce: "n".into(), data: "d".into() });
            sec.pages = vec![sec.placeholder()];
        };
        let mut edited = base.clone();
        let t = edited.notebooks[0].sections[0].pages[0].title.clone();
        page_mut(&mut edited, &t).title = "written meanwhile".into();
        let mut l = base.clone();
        locked(&mut l);
        for m in [merge(&base, &l, &edited).unwrap(), merge(&base, &edited, &l).unwrap()] {
            let secs = &m.notebooks[0].sections;
            assert!(secs[0].sealed() && secs[0].pages.len() == 1, "the locked section holds only its placeholder");
            assert!(secs[1].lock.is_none() && secs[1].name.ends_with("(other device)"));
            assert!(secs[1].pages.iter().any(|p| p.title == "written meanwhile"));
        }
        let mut bad = l.clone();
        bad.notebooks[0].sections[0].pages.push(Page::new("stray"));
        let m = merge(&l, &l, &bad).unwrap();
        assert!(m.notebooks[0].sections[0].sealed() && m.notebooks[0].sections[1].pages[0].title == "stray");
    }

    fn run(s: &SyncSettings, home: &Path, local: &Store, first: Option<First>, quick: bool) -> Result<Outcome, String> {
        let r = super::run(s, home, local, first, quick);
        if let Ok(Outcome::Synced { base, .. }) = &r {
            base.commit(home).unwrap();
        }
        r
    }

    #[test]
    fn syncs_two_devices_through_a_folder() {
        let shared = tmp("shared");
        let (a, b) = (tmp("a"), tmp("b"));
        let cfg = SyncSettings { via: Via::Folder, folder: shared.display().to_string(), ..Default::default() };

        // Device A starts with a referenced picture AND an unreferenced local orphan.
        std::fs::create_dir_all(a.join("files")).unwrap();
        std::fs::write(a.join("files/123.png"), b"png").unwrap();
        std::fs::write(a.join("files/orphan.png"), b"junk").unwrap();
        let mut sa = sample();
        sa.notebooks[0].sections[0].pages[0].blocks.push(block(0.0, 0.0, 200.0, Kind::Image { file: "123.png".into() }));
        let Ok(Outcome::Synced { store: s1, up: 1, .. }) = run(&cfg, &a, &sa, None, false) else {
            panic!("first sync should upload only the 1 referenced file")
        };
        sa = s1;
        assert!(shared.join("LeafNotes/library.json").exists(), "v2 manifest written");
        assert!(!shared.join("LeafNotes/files/orphan.png").exists(), "unreferenced local orphan never uploaded");

        // A new device with only the untouched sample notebook takes the synced notes without asking.
        let c = tmp("c");
        let Ok(Outcome::Synced { store: sc, .. }) = run(&cfg, &c, &sample(), None, false) else {
            panic!("the sample shouldn't ask")
        };
        assert!(same(&sc, &sa));
        let _ = std::fs::remove_dir_all(c);

        // Device B has its own notes: asked first, then joins.
        let mut sb = sample();
        sb.notebooks[0].sections[0].pages[0].title = "My own page".into();
        assert!(matches!(run(&cfg, &b, &sb, None, false), Ok(Outcome::AskFirst { .. })));
        let Ok(Outcome::Synced { store: mut sb, down: 1, .. }) = run(&cfg, &b, &sb, Some(First::UseShared), false) else {
            panic!("join")
        };
        assert_eq!(std::fs::read(b.join("files/123.png")).unwrap(), b"png");
        assert!(same(&sa, &sb));

        // Incremental per-page check: editing one page only rewrites that page file on remote.
        let untouched_id = sa.notebooks[1].sections[0].pages[0].id;
        let untouched_path = shared.join("LeafNotes/pages").join(format!("{untouched_id}.json"));
        let before_mtime = std::fs::metadata(&untouched_path).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(20));

        page_mut(&mut sa, "Week 1 — Intro").title = "Intro (A)".into();
        page_mut(&mut sb, "Groceries").title = "Groceries (B)".into();
        let Ok(Outcome::Synced { store: sa2, .. }) = run(&cfg, &a, &sa, None, false) else { panic!() };
        assert_eq!(
            std::fs::metadata(&untouched_path).unwrap().modified().unwrap(),
            before_mtime,
            "unchanged page was not rewritten"
        );
        let Ok(Outcome::Synced { store: sb2, .. }) = run(&cfg, &b, &sb, None, false) else { panic!() };
        let Ok(Outcome::Synced { store: sa3, .. }) = run(&cfg, &a, &sa2, None, false) else { panic!() };
        assert!(same(&sa3, &sb2));
        let t = titles(&sa3);
        assert!(t.contains(&"Intro (A)".into()) && t.contains(&"Groceries (B)".into()), "{t:?}");

        // Safe remote attachment GC: removing the image block and syncing prunes 123.png remotely.
        let mut sa4 = sa3.clone();
        sa4.notebooks[0].sections[0].pages[0].blocks.retain(|bl| !matches!(&bl.kind, Kind::Image { file } if file == "123.png"));
        let Ok(Outcome::Synced { store: sa5, .. }) = run(&cfg, &a, &sa4, None, false) else { panic!() };
        assert!(!shared.join("LeafNotes/files/123.png").exists(), "orphaned remote attachment pruned");

        // A damaged shared manifest is never overwritten.
        std::fs::write(shared.join("LeafNotes/library.json"), b"{broken").unwrap();
        assert!(run(&cfg, &a, &sa5, None, false).is_err());
        assert_eq!(std::fs::read(shared.join("LeafNotes/library.json")).unwrap(), b"{broken");

        // Copies get their own IDs (including inner IDs), and locked placeholders keep their section's ID.
        let mut twice = sa5.clone();
        let mut copy = twice.notebooks[0].clone();
        copy.sections[0].lock = Some(Lock { v: 1, salt: "s".into(), nonce: "n".into(), data: "d".into() });
        copy.sections[0].pages = vec![copy.sections[0].placeholder()];
        twice.notebooks.push(copy);
        dedupe(&mut twice);
        let sec = &twice.notebooks[2].sections[0];
        assert!(sec.id != twice.notebooks[0].sections[0].id && sec.pages[0].id == sec.id);

        // Network URL security policy: loopback auto-allowed, LAN requires opt-in, public rejected.
        assert!(validate_http_url("https://cloud.example.com/dav", false).is_ok());
        assert!(validate_http_url("http://localhost:11434/v1", false).is_ok());
        assert!(validate_http_url("http://127.0.0.1:8080/dav", false).is_ok());
        assert!(validate_http_url("http://[::1]:80/dav", false).is_ok());
        assert!(validate_http_url("http://192.168.1.5:8080/dav", false).is_err());
        assert!(validate_http_url("http://192.168.1.5:8080/dav", true).is_ok());
        assert!(validate_http_url("http://nas.local/dav", false).is_err());
        assert!(validate_http_url("http://nas.local/dav", true).is_ok());
        assert!(validate_http_url("http://10.evil.com", true).is_err());
        assert!(validate_http_url("http://127.0.0.1@evil.com", true).is_err());
        assert!(validate_http_url("http://8.8.8.8/dav", true).is_err());

        // Names from other devices can't escape the files folder.
        assert!(!safe_name("../x") && !safe_name("a/b") && !safe_name(".hidden") && safe_name("123.png"));
        assert_eq!((dec("4%2F0Ab%3D+x"), dec("bad%2"), enc("a b/'")), ("4/0Ab= x".to_string(), "bad%2".to_string(), "a%20b%2F%27".to_string()));
        assert_eq!(id_email("x.eyJlbWFpbCI6ImFAYi5jIn0.y"), "a@b.c");

        // A folder that doesn't exist is refused, not created.
        let missing = SyncSettings { folder: shared.join("nope").display().to_string(), ..cfg.clone() };
        assert!(run(&missing, &a, &sa5, None, false).is_err() && !shared.join("nope").exists());
        for d in [shared, a, b] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn migrates_legacy_v1_notes_json_to_v2_per_page_sync() {
        let shared = tmp("v1-shared");
        let a = tmp("v1-a");
        std::fs::create_dir_all(shared.join("LeafNotes/files")).unwrap();
        let mut legacy_store = sample();
        legacy_store.notebooks[0].sections[0].pages[0].title = "From legacy v1".into();
        let v1_bytes = serde_json::to_vec(&serde_json::json!({
            "format": 1,
            "store": &legacy_store,
            "files": []
        }))
        .unwrap();
        std::fs::write(shared.join("LeafNotes/notes.json"), &v1_bytes).unwrap();

        let cfg = SyncSettings { via: Via::Folder, folder: shared.display().to_string(), ..Default::default() };
        let Ok(Outcome::Synced { store, .. }) = run(&cfg, &a, &sample(), None, false) else {
            panic!("migration should succeed")
        };
        assert_eq!(store.notebooks[0].sections[0].pages[0].title, "From legacy v1");
        assert!(shared.join("LeafNotes/library.json").exists(), "upgraded to v2 library.json");
        let p0_id = store.notebooks[0].sections[0].pages[0].id;
        assert!(shared.join("LeafNotes/pages").join(format!("{p0_id}.json")).exists(), "per-page file written");
        for d in [shared, a] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
