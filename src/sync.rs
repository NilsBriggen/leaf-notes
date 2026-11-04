//! Sync between devices through a shared folder (Dropbox, OneDrive, Syncthing…) or a WebDAV server (Nextcloud…).
//!
//! The shared copy is one `LeafNotes/notes.json` plus `LeafNotes/files/` for pictures, PDFs and recordings.
//! Each device keeps the last state it agreed on with the shared copy (the "base"), so changes merge
//! note by note: what changed here, what changed elsewhere, and both are kept. A page changed on two
//! devices at once is kept twice. Nothing is ever deleted because of a conflict.
use crate::tf;
use crate::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SYNC_FILE: &str = "sync.json";
const BASE_FILE: &str = "sync-base.json";
const NOTES: &str = "notes.json";
const FORMAT: u64 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum Via { #[default] Off, Folder, WebDav, Google }

#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct SyncSettings {
    pub via: Via,
    #[serde(default)] pub folder: String,
    #[serde(default)] pub url: String,
    #[serde(default)] pub user: String,
    #[serde(default)] pub password: String,
    /// Google Drive: the long-lived sign-in (refresh token) and whose Drive it is.
    #[serde(default)] pub google_token: String,
    #[serde(default)] pub google_email: String,
}

impl SyncSettings {
    pub fn on(&self) -> bool {
        match self.via { Via::Off => false, Via::Folder => !self.folder.trim().is_empty(), Via::WebDav => !self.url.trim().is_empty(), Via::Google => !self.google_token.is_empty() }
    }
    /// Where the notes go; the base only counts for the same place.
    fn target(&self) -> String {
        match self.via { Via::Folder => self.folder.trim().into(), Via::WebDav => format!("{}@{}", self.user.trim(), self.url.trim()), Via::Google => format!("google:{}", self.google_email), Via::Off => String::new() }
    }
}

/// The last state this device and the shared copy agreed on (`tag` identifies that version of the shared file).
#[derive(Serialize, Deserialize)]
struct Base { target: String, tag: String, #[serde(default)] files: Vec<String>, store: Store }

/// How a device that already has notes joins a sync location that already has notes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum First { Merge, UseShared }

pub enum Outcome {
    /// Done: `store` is now on both sides.
    Synced { store: Store, up: usize, down: usize },
    /// First sync here and both sides have notes: ask the person how to join (pages in the shared copy).
    AskFirst { pages: usize },
}

/// One sync round. `home` is this device's data folder. `quick` uses short timeouts (when closing the app).
pub fn run(s: &SyncSettings, home: &Path, local: &Store, first: Option<First>, quick: bool) -> Result<Outcome, String> {
    let remote = Remote::new(s, quick)?;
    let base: Option<Base> = std::fs::read(home.join(BASE_FILE)).ok().and_then(|b| serde_json::from_slice::<Base>(&b).ok()).filter(|b| b.target == s.target());
    let files = home.join("files");
    // Several tries: if another device writes in between, fetch its version and merge again.
    for _ in 0..4 {
        let known = base.as_ref().map(|b| b.tag.as_str()).filter(|t| !t.is_empty());
        let (shared, tag): (Option<(Store, Vec<String>)>, Option<String>) = match remote.get_notes(known)? {
            Got::Missing => (None, None),
            // unchanged since we last agreed: the shared store is our base
            Got::Same => { let b = base.as_ref().expect("Same only with a base"); (Some((b.store.clone(), b.files.clone())), Some(b.tag.clone())) }
            Got::Data(bytes, tag) => (Some(parse_shared(&bytes)?), Some(tag)),
        };
        let merged = match (&shared, &base, first) {
            (None, _, _) => local.clone(),
            (Some((sh, _)), _, Some(First::UseShared)) => sh.clone(),
            (Some((sh, _)), Some(b), _) => merge(&b.store, local, sh)?,
            (Some((sh, _)), None, Some(First::Merge)) => merge(&Store { notebooks: vec![], trash: vec![], study: HashMap::new() }, local, sh)?,
            (Some((sh, _)), None, None) => {
                if has_notes(local) { return Ok(Outcome::AskFirst { pages: sh.notebooks.iter().flat_map(|n| &n.sections).map(|s| s.pages.len()).sum() }); }
                sh.clone()
            }
        };
        // Files first, so other devices never see notes whose pictures aren't there yet.
        let mut have: Vec<String> = shared.as_ref().map(|(_, f)| f.clone()).unwrap_or_default();
        let (mut up, mut down) = (0, 0);
        for f in list_files(&files) {
            if !have.contains(&f) { remote.put_file(&f, &files.join(&f))?; have.push(f); up += 1; }
        }
        for f in &have {
            if safe_name(f) && !files.join(f).exists() && remote.get_file(f, &files.join(f))? { down += 1; }
        }
        let same = shared.as_ref().is_some_and(|(sh, f)| f.len() == have.len() && canon(sh) == canon(&merged));
        let tag = if same { tag.unwrap_or_default() } else {
            let bytes = serde_json::to_vec(&serde_json::json!({ "format": FORMAT, "store": &merged, "files": &have })).map_err(|e| e.to_string())?;
            match remote.put_notes(&bytes, tag.as_deref())? { Put::Done(t) => t, Put::Changed => continue }
        };
        let b = serde_json::to_vec(&Base { target: s.target(), tag, files: have, store: merged.clone() }).map_err(|e| e.to_string())?;
        write_atomic(&home.join(BASE_FILE), &b).map_err(|e| tf!("Couldn't save the sync state: {e}", e = e))?;
        return Ok(Outcome::Synced { store: merged, up, down });
    }
    Err(crate::tr("Another device kept syncing at the same moment — trying again shortly.").into())
}

fn parse_shared(bytes: &[u8]) -> Result<(Store, Vec<String>), String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| crate::tr("The synced notes file is damaged, so nothing was changed. Your notes on this device are safe.").to_string())?;
    if v.get("format").and_then(Value::as_u64).unwrap_or(0) > FORMAT { return Err(crate::tr("The synced notes come from a newer Leaf Notes — update this app to keep syncing.").into()); }
    let store: Store = serde_json::from_value(v.get("store").cloned().unwrap_or(Value::Null)).map_err(|e| tf!("The synced notes couldn't be read ({e}), so nothing was changed.", e = e))?;
    let files = v.get("files").and_then(Value::as_array).map(|a| a.iter().filter_map(|f| f.as_str().map(String::from)).collect()).unwrap_or_default();
    Ok((store, files))
}

fn has_notes(s: &Store) -> bool {
    s.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).any(|p| !p.blocks.is_empty() || !p.strokes.is_empty() || p.cards.iter().any(|c| !c.front.is_empty()) || !p.title.trim().is_empty() && p.title != "Untitled page")
}

/// Attachment names come from other devices: only plain names, never paths.
fn safe_name(n: &str) -> bool {
    !n.is_empty() && n.len() < 200 && !n.starts_with('.') && n.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

fn list_files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| safe_name(n) && !n.ends_with(".part") && !n.ends_with(".tmp")).collect();
    v.sort();
    v
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", new_id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| { let _ = std::fs::remove_file(&tmp); })
}

/// Floats and maps compare reliably only after a round trip through JSON text.
fn canon<T: Serialize>(v: &T) -> Value { serde_json::to_vec(v).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null) }

pub fn same(a: &Store, b: &Store) -> bool { canon(a) == canon(b) }

// ---------- merging ----------

/// Three-way merge of whole notebooks. Lists of things with an `id` merge item by item;
/// a page changed on both sides in the same spot is kept twice ("… (other device)").
pub fn merge(base: &Store, local: &Store, remote: &Store) -> Result<Store, String> {
    let (b, l, r) = (canon(base), canon(local), canon(remote));
    if l == b || l == r { return Ok(remote.clone()); }
    if r == b { return Ok(local.clone()); }
    let v = mv(&b, &l, &r, false).unwrap_or(l);
    let mut s: Store = serde_json::from_value(v).map_err(|e| tf!("Couldn't combine the changes ({e}); nothing was changed.", e = e))?;
    dedupe(&mut s);
    Ok(s)
}

/// `strict`: inside a page, where a clash means the page is kept twice (Err); elsewhere this device's value wins.
fn mv(b: &Value, l: &Value, r: &Value, strict: bool) -> Result<Value, ()> {
    if l == r || r == b { return Ok(l.clone()); }
    if l == b { return Ok(r.clone()); }
    match (l, r) {
        (Value::Object(lo), Value::Object(ro)) => {
            let empty = Map::new();
            let bo = b.as_object().unwrap_or(&empty);
            let mut out = Map::new();
            for k in lo.keys().chain(ro.keys().filter(|k| !lo.contains_key(*k))) {
                match (bo.get(k), lo.get(k), ro.get(k)) {
                    (bv, Some(lv), Some(rv)) => { out.insert(k.clone(), mv(bv.unwrap_or(&Value::Null), lv, rv, strict)?); }
                    // removed on one side: gone, unless the other side changed it (then keep the change)
                    (Some(bv), Some(v), None) | (Some(bv), None, Some(v)) => { if v != bv { if strict { return Err(()); } out.insert(k.clone(), v.clone()); } }
                    (None, Some(v), None) | (None, None, Some(v)) => { out.insert(k.clone(), v.clone()); }
                    _ => {}
                }
            }
            Ok(Value::Object(out))
        }
        (Value::Array(la), Value::Array(ra)) if ids(la) && ids(ra) => {
            let empty = vec![];
            Ok(Value::Array(merge_ids(b.as_array().filter(|a| ids(a)).unwrap_or(&empty), la, ra, strict)?))
        }
        // lists without ids (e.g. Recently deleted): keep what either side added, drop what either side removed
        (Value::Array(la), Value::Array(ra)) if !strict && la.iter().chain(ra).all(Value::is_object) => {
            let empty = vec![];
            let ba = b.as_array().unwrap_or(&empty);
            let mut out: Vec<Value> = la.iter().filter(|v| !ba.contains(v) || ra.contains(v)).cloned().collect();
            out.extend(ra.iter().filter(|v| !ba.contains(v) && !la.contains(v)).cloned());
            Ok(Value::Array(out))
        }
        _ => if strict { Err(()) } else { Ok(l.clone()) },
    }
}

fn id_of(v: &Value) -> Option<u64> { v.get("id").and_then(Value::as_u64) }
fn ids(a: &[Value]) -> bool { a.iter().all(|v| id_of(v).is_some()) }
fn is_page(v: &Value) -> bool { v.get("blocks").is_some() && v.get("title").is_some() }

fn merge_ids(b: &[Value], l: &[Value], r: &[Value], strict: bool) -> Result<Vec<Value>, ()> {
    let bm: HashMap<u64, &Value> = b.iter().filter_map(|v| Some((id_of(v)?, v))).collect();
    let rm: HashMap<u64, &Value> = r.iter().filter_map(|v| Some((id_of(v)?, v))).collect();
    let lm: HashMap<u64, &Value> = l.iter().filter_map(|v| Some((id_of(v)?, v))).collect();
    let mut out = vec![];
    for lv in l {
        let i = id_of(lv).unwrap_or_default();
        match (bm.get(&i), rm.get(&i)) {
            (bv, Some(rv)) => {
                let bv = bv.copied().unwrap_or(&Value::Null);
                if is_page(lv) {
                    match mv(bv, lv, rv, true) {
                        Ok(v) => out.push(v),
                        Err(()) => { out.push(lv.clone()); out.push(conflict_copy(rv)); }
                    }
                } else { out.push(mv(bv, lv, rv, strict)?); }
            }
            (Some(bv), None) => { if lv != *bv { out.push(lv.clone()); } } // deleted there; kept if changed here
            (None, None) => out.push(lv.clone()), // new here
        }
    }
    for (k, rv) in r.iter().enumerate() {
        let i = id_of(rv).unwrap_or_default();
        if lm.contains_key(&i) { continue; }
        if bm.get(&i).is_some_and(|bv| *bv == rv) { continue; } // deleted here, unchanged there
        // new there (or changed there after being deleted here): place it after its neighbour
        let at = r[..k].iter().rev().find_map(|p| out.iter().position(|o| id_of(o) == id_of(p))).map_or(0, |p| p + 1);
        out.insert(at, rv.clone());
    }
    Ok(out)
}

/// The other device's version of a page that both changed, as its own page.
fn conflict_copy(v: &Value) -> Value {
    let mut c = v.clone();
    c["id"] = new_id().into();
    let t = c["title"].as_str().unwrap_or("").to_string();
    c["title"] = format!("{} (other device)", if t.trim().is_empty() { "Untitled page" } else { &t }).into();
    for list in ["blocks", "strokes"] {
        if let Some(a) = c[list].as_array_mut() { for x in a { x["id"] = new_id().into(); } }
    }
    c
}

/// A page moved on one device and edited on another ends up in two places: give the second its own id.
fn dedupe(s: &mut Store) {
    let mut seen = std::collections::HashSet::new();
    for n in &mut s.notebooks {
        if !seen.insert(n.id) { n.id = new_id(); }
        for sec in &mut n.sections {
            if !seen.insert(sec.id) { sec.id = new_id(); }
            for p in &mut sec.pages { if !seen.insert(p.id) { p.id = new_id(); } }
        }
    }
}

// ---------- where the shared copy lives ----------

enum Remote {
    Folder(PathBuf),
    Dav { agent: ureq::Agent, dir: String, auth: String },
    /// Google Drive: a "Leaf Notes" folder (and its "files" folder) in the person's Drive.
    Drive { agent: ureq::Agent, token: String, folder: String, files: String },
}

enum Got { Missing, Same, Data(Vec<u8>, String) }
enum Put { Done(String), Changed }

fn hash(b: &[u8]) -> String {
    // FNV-1a: stable across runs and Rust versions (the tag is saved)
    let h = b.iter().fold(0xcbf29ce484222325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100000001b3));
    format!("{h:016x}-{}", b.len())
}

impl Remote {
    fn new(s: &SyncSettings, quick: bool) -> Result<Remote, String> {
        match s.via {
            Via::Off => Err(crate::tr("Sync is off.").into()),
            Via::Folder => {
                let root = PathBuf::from(s.folder.trim());
                // never create the chosen folder itself: a half-typed path must not litter the disk
                if !root.is_dir() { return Err(tf!("The folder {root} doesn't exist.", root = root.display())); }
                let d = root.join("LeafNotes");
                std::fs::create_dir_all(d.join("files")).map_err(|e| tf!("Can't write to {d}: {e}", d = d.display(), e = e))?;
                Ok(Remote::Folder(d))
            }
            Via::Google => {
                let agent = agent(quick);
                let token = google_access(&agent, &s.google_token)?;
                let mut r = Remote::Drive { agent, token, folder: String::new(), files: String::new() };
                let folder = r.drive_folder("Leaf Notes", "root")?;
                let files = r.drive_folder("files", &folder)?;
                if let Remote::Drive { folder: f, files: fl, .. } = &mut r { (*f, *fl) = (folder, files); }
                Ok(r)
            }
            Via::WebDav => {
                use base64::Engine;
                let url = s.url.trim().trim_end_matches('/');
                let lan = ["http://localhost", "http://127.", "http://192.168.", "http://10."].iter().any(|p| url.starts_with(p)) || url.split('/').nth(2).is_some_and(|h| h.split(':').next().unwrap_or("").ends_with(".local"));
                // the password travels with every request, so only over an encrypted connection (or the home network)
                if !url.starts_with("https://") && !(url.starts_with("http://") && lan) { return Err(crate::tr("The server address must start with https://").into()); }
                let agent = agent(quick);
                let auth = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", s.user.trim(), s.password)));
                let r = Remote::Dav { agent, dir: format!("{url}/LeafNotes/"), auth };
                r.mkcol("")?;
                r.mkcol("files/")?;
                Ok(r)
            }
        }
    }

    fn dav(&self, method: &str, path: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> Result<ureq::http::Response<ureq::Body>, String> {
        let Remote::Dav { agent, dir, auth } = self else { unreachable!() };
        let mut b = ureq::http::Request::builder().method(method).uri(format!("{dir}{path}")).header("authorization", auth.as_str());
        for (k, v) in headers { b = b.header(*k, *v); }
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
            200..=299 | 301 | 405 => Ok(()), // 405: already there
            404 | 409 => Err(crate::tr("The server address doesn't point to a folder on the server.").into()),
            _ => Err(tf!("The sync server answered {s} when creating its folder.", s = s)),
        }
    }

    fn get_notes(&self, known: Option<&str>) -> Result<Got, String> {
        match self {
            Remote::Folder(d) => match std::fs::read(d.join(NOTES)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Got::Missing),
                Err(e) => Err(tf!("Couldn't read the synced notes: {e}", e = e)),
                Ok(b) => { let t = hash(&b); Ok(if known == Some(t.as_str()) { Got::Same } else { Got::Data(b, t) }) }
            },
            Remote::Drive { folder, .. } => match self.drive_find(NOTES, folder)? {
                None => Ok(Got::Missing),
                Some((_, v)) if known == Some(v.as_str()) => Ok(Got::Same),
                Some((id, v)) => {
                    let mut resp = self.gd("GET", &format!("{API}/files/{id}?alt=media"), None)?;
                    if resp.status().as_u16() != 200 { return Err(tf!("Google Drive answered {status} when reading the notes.", status = resp.status())); }
                    let b = resp.body_mut().with_config().limit(1 << 31).read_to_vec().map_err(|e| tf!("The download of the notes broke off: {e}", e = e))?;
                    Ok(Got::Data(b, v))
                }
            },
            Remote::Dav { .. } => {
                let h: Vec<(&str, &str)> = known.map(|t| vec![("if-none-match", t)]).unwrap_or_default();
                let mut resp = self.dav("GET", NOTES, &h, None)?;
                match resp.status().as_u16() {
                    404 => Ok(Got::Missing),
                    304 => Ok(Got::Same),
                    200 => {
                        let tag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                        let b = resp.body_mut().with_config().limit(1 << 31).read_to_vec().map_err(|e| tf!("The download of the notes broke off: {e}", e = e))?;
                        Ok(Got::Data(b, tag))
                    }
                    s => Err(tf!("The sync server answered {s} when reading the notes.", s = s)),
                }
            }
        }
    }

    /// Write the notes only if the shared copy is still the version `expect` (None: there is none yet).
    fn put_notes(&self, bytes: &[u8], expect: Option<&str>) -> Result<Put, String> {
        match self {
            Remote::Folder(d) => {
                let path = d.join(NOTES);
                let now = std::fs::read(&path).ok();
                if now.as_deref().map(hash).as_deref() != expect { return Ok(Put::Changed); }
                // shortcut: a device writing between this check and the rename can still be overwritten; the folder app keeps versions
                if now.is_some() { let _ = std::fs::copy(&path, d.join("notes.prev.json")); }
                write_atomic(&path, bytes).map_err(|e| tf!("Couldn't write the synced notes: {e}", e = e))?;
                Ok(Put::Done(hash(bytes)))
            }
            Remote::Drive { folder, .. } => {
                // Drive v3 has no conditional write: check the version right before writing.
                // shortcut: a device writing within that moment can still be overwritten; Drive keeps the old revision.
                let now = self.drive_find(NOTES, folder)?;
                if now.as_ref().map(|(_, v)| v.as_str()) != expect { return Ok(Put::Changed); }
                let meta = self.drive_upload(now.as_ref().map(|(id, _)| id.as_str()), NOTES, folder, bytes.to_vec())?;
                Ok(Put::Done(meta["version"].as_str().unwrap_or("").to_string()))
            }
            Remote::Dav { .. } => {
                // shortcut: servers without ETags get an unconditional write; Nextcloud, ownCloud and Apache all send them
                let h: Vec<(&str, &str)> = match expect { None => vec![("if-none-match", "*")], Some("") => vec![], Some(t) => vec![("if-match", t)] };
                let resp = self.dav("PUT", NOTES, &h, Some(bytes.to_vec()))?;
                match resp.status().as_u16() {
                    200..=299 => Ok(Put::Done(resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string())),
                    412 => Ok(Put::Changed),
                    s => Err(tf!("The sync server answered {s} when saving the notes.", s = s)),
                }
            }
        }
    }

    fn put_file(&self, name: &str, from: &Path) -> Result<(), String> {
        match self {
            Remote::Folder(d) => {
                let to = d.join("files").join(name);
                if to.exists() { return Ok(()); }
                let tmp = to.with_extension("part");
                std::fs::copy(from, &tmp).and_then(|_| std::fs::rename(&tmp, &to)).map_err(|e| tf!("Couldn't copy {name}: {e}", name = name, e = e))
            }
            Remote::Drive { files, .. } => {
                if self.drive_find(name, files)?.is_some() { return Ok(()); }
                let data = std::fs::read(from).map_err(|e| tf!("Couldn't read {name}: {e}", name = name, e = e))?;
                self.drive_upload(None, name, files, data).map(|_| ())
            }
            Remote::Dav { .. } => {
                let data = std::fs::read(from).map_err(|e| tf!("Couldn't read {name}: {e}", name = name, e = e))?;
                let s = self.dav("PUT", &format!("files/{name}"), &[], Some(data))?.status().as_u16();
                if (200..300).contains(&s) { Ok(()) } else { Err(tf!("The sync server answered {s} when uploading {name}.", s = s, name = name)) }
            }
        }
    }

    /// Download into place via a temporary file; false if the shared copy doesn't have it (yet).
    fn get_file(&self, name: &str, to: &Path) -> Result<bool, String> {
        if let Some(dir) = to.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
        let tmp = to.with_extension("part");
        let r = match self {
            Remote::Folder(d) => {
                let from = d.join("files").join(name);
                if !from.exists() { return Ok(false); }
                std::fs::copy(&from, &tmp).map(|_| ()).map_err(|e| tf!("Couldn't copy {name}: {e}", name = name, e = e))
            }
            Remote::Drive { files, .. } => {
                let Some((id, _)) = self.drive_find(name, files)? else { return Ok(false) };
                let resp = self.gd("GET", &format!("{API}/files/{id}?alt=media"), None)?;
                if resp.status().as_u16() != 200 { return Err(tf!("Google Drive answered {status} when downloading {name}.", status = resp.status(), name = name)); }
                let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
                std::io::copy(&mut resp.into_body().into_reader().take(1 << 33), &mut f).map(|_| ()).map_err(|e| tf!("The download of {name} broke off: {e}", name = name, e = e))
            }
            Remote::Dav { .. } => {
                let resp = self.dav("GET", &format!("files/{name}"), &[], None)?;
                match resp.status().as_u16() {
                    404 => return Ok(false),
                    200 => {
                        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
                        std::io::copy(&mut resp.into_body().into_reader().take(1 << 33), &mut f).map(|_| ()).map_err(|e| tf!("The download of {name} broke off: {e}", name = name, e = e))
                    }
                    s => Err(tf!("The sync server answered {s} when downloading {name}.", s = s, name = name)),
                }
            }
        };
        match r.and_then(|_| std::fs::rename(&tmp, to).map_err(|e| e.to_string())) {
            Ok(()) => Ok(true),
            Err(e) => { let _ = std::fs::remove_file(&tmp); Err(e) }
        }
    }
}

fn agent(quick: bool) -> ureq::Agent {
    ureq::Agent::config_builder().http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(if quick { 5 } else { 20 })))
        .timeout_global(Some(Duration::from_secs(if quick { 15 } else { 600 }))).build().into()
}

// ---------- Google Drive ----------

/// Leaf Notes' Google sign-in (a "Desktop app" OAuth client; for installed apps the secret isn't confidential).
const GOOGLE_ID: &str = "304608478286-2n9iukrh6bg19fa0r7v21fhk3ru3kqm4.apps.googleusercontent.com";
// Comes from the build (.cargo/config.toml locally, a repository secret in CI), so it never sits in the code.
const GOOGLE_SECRET: &str = match option_env!("LEAFNOTES_GOOGLE_SECRET") { Some(s) => s, None => "" };
/// Only files Leaf Notes made itself: it can't see anything else in the Drive.
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file openid email";
const API: &str = "https://www.googleapis.com/drive/v3";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

pub fn google_ready() -> bool { !GOOGLE_SECRET.is_empty() }

impl Remote {
    fn gd(&self, method: &str, url: &str, body: Option<(&str, Vec<u8>)>) -> Result<ureq::http::Response<ureq::Body>, String> {
        let Remote::Drive { agent, token, .. } = self else { unreachable!() };
        let b = ureq::http::Request::builder().method(method).uri(url).header("authorization", format!("Bearer {token}", token = token));
        let r = match body {
            Some((ct, data)) => agent.run(b.header("content-type", ct).body(data).map_err(|e| e.to_string())?),
            None => agent.run(b.body(()).map_err(|e| e.to_string())?),
        };
        let resp = r.map_err(|e| tf!("Couldn't reach Google Drive: {e}", e = e))?;
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
        let v: Value = resp.body_mut().read_json().map_err(|e| tf!("Google Drive sent an unexpected answer ({st}): {e}", st = st, e = e))?;
        if (200..300).contains(&st) { Ok(v) } else { Err(tf!("Google Drive answered {st}: {x}", st = st, x = v["error"]["message"].as_str().unwrap_or(""))) }
    }

    /// (id, version) of the oldest file with this name in a folder: every device picks the same one.
    fn drive_find(&self, name: &str, parent: &str) -> Result<Option<(String, String)>, String> {
        let q = format!("name='{name}' and '{parent}' in parents and trashed=false");
        let v = self.json("GET", &format!("{API}/files?q={}&orderBy=createdTime&fields=files(id,version)&pageSize=10&spaces=drive", enc(&q)), None)?;
        Ok(v["files"].get(0).map(|f| (f["id"].as_str().unwrap_or("").to_string(), f["version"].as_str().unwrap_or("").to_string())))
    }

    fn drive_folder(&self, name: &str, parent: &str) -> Result<String, String> {
        if let Some((id, _)) = self.drive_find(name, parent)? { return Ok(id); }
        let v = self.json("POST", &format!("{API}/files?fields=id"), Some(serde_json::json!({ "name": name, "mimeType": "application/vnd.google-apps.folder", "parents": [parent] })))?;
        v["id"].as_str().map(String::from).ok_or(crate::tr("Google Drive didn't create the folder.").into())
    }

    /// Resumable upload (works for any size): a new file, or new contents for `existing`.
    fn drive_upload(&self, existing: Option<&str>, name: &str, parent: &str, data: Vec<u8>) -> Result<Value, String> {
        let up = "https://www.googleapis.com/upload/drive/v3/files";
        let (method, url, meta) = match existing {
            Some(id) => ("PATCH", format!("{up}/{id}?uploadType=resumable&fields=id,version", up = up, id = id), serde_json::json!({})),
            None => ("POST", format!("{up}?uploadType=resumable&fields=id,version", up = up), serde_json::json!({ "name": name, "parents": [parent] })),
        };
        let resp = self.gd(method, &url, Some(("application/json; charset=UTF-8", meta.to_string().into_bytes())))?;
        let st = resp.status().as_u16();
        let loc = resp.headers().get("location").and_then(|v| v.to_str().ok()).map(String::from);
        let Some(loc) = loc.filter(|_| st == 200) else { return Err(tf!("Google Drive answered {st} when starting the upload of {name}.", st = st, name = name)) };
        let mut resp = self.gd("PUT", &loc, Some(("application/octet-stream", data)))?;
        let st = resp.status().as_u16();
        if !(200..300).contains(&st) { return Err(tf!("Google Drive answered {st} when uploading {name}.", st = st, name = name)); }
        resp.body_mut().read_json().map_err(|e| e.to_string())
    }
}

/// A short-lived access token from the saved sign-in.
fn google_access(agent: &ureq::Agent, refresh: &str) -> Result<String, String> {
    let mut resp = agent.post(TOKEN_URL).send_form([("client_id", GOOGLE_ID), ("client_secret", GOOGLE_SECRET), ("refresh_token", refresh), ("grant_type", "refresh_token")])
        .map_err(|e| tf!("Couldn't reach Google: {e}", e = e))?;
    let v: Value = resp.body_mut().read_json().map_err(|e| tf!("Google sent an unexpected answer: {e}", e = e))?;
    match v["access_token"].as_str() {
        Some(t) => Ok(t.to_string()),
        None if v["error"] == "invalid_grant" => Err(crate::tr("Your Google sign-in expired or was removed — sign in again in Settings ▸ Sync.").into()),
        None => Err(tf!("Google refused the sign-in: {x}", x = v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("unknown error"))),
    }
}

/// A sign-in in progress: the browser comes back to this little local web server.
pub struct SignIn { listener: std::net::TcpListener, verifier: String, state: String, redirect: String }

/// Start signing in: returns the address to open in the browser.
pub fn google_start() -> Result<(String, SignIn), String> {
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
    let url = format!("https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}&access_type=offline&prompt=consent",
        enc(GOOGLE_ID), enc(&redirect), enc(SCOPE));
    Ok((url, SignIn { listener, verifier, state, redirect }))
}

/// Wait (up to 5 minutes) for the browser to come back, then trade the code for a lasting sign-in: (refresh token, email).
pub fn google_finish(s: SignIn) -> Result<(String, String), String> {
    use std::io::Write;
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    s.listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let code = loop {
        if std::time::Instant::now() > deadline { return Err(crate::tr("Sign-in timed out — try again.").into()); }
        let (mut conn, _) = match s.listener.accept() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => { std::thread::sleep(Duration::from_millis(200)); continue; }
            Err(e) => return Err(e.to_string()),
        };
        let _ = conn.set_nonblocking(false);
        let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = vec![0u8; 8192];
        let n = conn.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let path = req.split_whitespace().nth(1).unwrap_or("");
        let q: HashMap<String, String> = path.split_once('?').map(|(_, q)| q).unwrap_or("").split('&')
            .filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), dec(v))).collect();
        if q.is_empty() { let _ = conn.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"); continue; } // favicon etc.
        let ok = q.get("state") == Some(&s.state) && q.contains_key("code");
        let msg = if ok { "Signed in. You can close this tab and go back to Leaf Notes." } else { "Sign-in didn't finish. Go back to Leaf Notes and try again." };
        let page = format!("<!doctype html><meta charset=utf-8><title>Leaf Notes</title><body style=\"font:18px sans-serif;margin:15vh auto;max-width:30em;text-align:center\"><h2>Leaf Notes</h2><p>{msg}</p>");
        let _ = conn.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len()).as_bytes());
        if q.get("state") != Some(&s.state) { return Err(crate::tr("Sign-in didn't match this request — try again.").into()); }
        match q.get("code") { Some(c) => break c.clone(), None => return Err(if q.get("error").map(String::as_str) == Some("access_denied") { crate::tr("Sign-in was cancelled.").into() } else { crate::tr("Google didn't sign you in — try again.").into() }) }
    };
    let mut resp = agent(false).post(TOKEN_URL).send_form([("client_id", GOOGLE_ID), ("client_secret", GOOGLE_SECRET), ("code", code.as_str()), ("code_verifier", s.verifier.as_str()), ("redirect_uri", s.redirect.as_str()), ("grant_type", "authorization_code")])
        .map_err(|e| tf!("Couldn't reach Google: {e}", e = e))?;
    let v: Value = resp.body_mut().read_json().map_err(|e| tf!("Google sent an unexpected answer: {e}", e = e))?;
    let Some(refresh) = v["refresh_token"].as_str() else { return Err(tf!("Google didn't sign you in: {x}", x = v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("no lasting sign-in was given"))) };
    Ok((refresh.to_string(), id_email(v["id_token"].as_str().unwrap_or(""))))
}

/// Forget the sign-in at Google too (best effort).
pub fn google_revoke(refresh: &str) { let _ = agent(true).post("https://oauth2.googleapis.com/revoke").send_form([("token", refresh)]); }

/// The email inside Google's id token (only shown to the person, so it isn't verified).
fn id_email(jwt: &str) -> String {
    use base64::Engine;
    jwt.split('.').nth(1).and_then(|p| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(p.trim_end_matches('=')).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v["email"].as_str().map(String::from)).unwrap_or_default()
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) { Ok(x) => { out.push(x); i += 3; continue; } Err(_) => out.push(b'%') },
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

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ln-sync-{tag}-{}", new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn page_mut<'a>(s: &'a mut Store, title: &str) -> &'a mut Page { s.notebooks.iter_mut().flat_map(|n| &mut n.sections).flat_map(|s| &mut s.pages).find(|p| p.title == title).unwrap() }
    fn titles(s: &Store) -> Vec<String> { s.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).map(|p| p.title.clone()).collect() }
    fn text(p: &Page) -> String { p.plain_text() }

    #[test]
    fn merges_note_by_note() {
        let base = sample();
        let (mut l, mut r) = (base.clone(), base.clone());
        // here: edit one page's title, delete another; there: write on a third, add a page
        page_mut(&mut l, "Week 1 — Intro").title = crate::tr("Week 1 — Introduction").into();
        l.notebooks[1].sections[0].pages.clear();
        page_mut(&mut r, "Week 2 — Convolution").blocks.push(Block { id: 7, pos: [0.0, 0.0], width: 300.0, tag: Default::default(), kind: Kind::Text { text: "flip and slide".into(), heading: 0 }, t: 0.0 });
        r.notebooks[0].sections[0].pages.push(Page::new("Week 3"));
        r.study.insert("card1".into(), CardState { bx: 2, due: 5 });
        let m = merge(&base, &l, &r).unwrap();
        let t = titles(&m);
        assert!(t.contains(&crate::tr("Week 1 — Introduction").to_string()) && t.contains(&"Week 3".to_string()) && !t.contains(&"Groceries".to_string()), "{t:?}");
        assert!(text(page_mut(&mut m.clone(), "Week 2 — Convolution")).contains("flip and slide"));
        assert_eq!(m.study.get("card1"), Some(&CardState { bx: 2, due: 5 }));
        // the same text block edited on both sides: the page is kept twice, nothing lost
        let (mut l2, mut r2) = (m.clone(), m.clone());
        for (s, w) in [(&mut l2, "here"), (&mut r2, "there")] {
            if let Kind::Text { text, .. } = &mut page_mut(s, "Week 2 — Convolution").blocks.last_mut().unwrap().kind { *text = w.into(); }
        }
        let m2 = merge(&m, &l2, &r2).unwrap();
        let t2 = titles(&m2);
        assert!(t2.contains(&crate::tr("Week 2 — Convolution (other device)").to_string()), "{t2:?}");
        let all: String = m2.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).map(text).collect();
        assert!(all.contains("here") && all.contains("there"));
        // deleted here but edited there: kept
        let (mut l3, mut r3) = (m.clone(), m.clone());
        l3.notebooks[0].sections[0].pages.retain(|p| p.title != "Week 3");
        page_mut(&mut r3, "Week 3").title = crate::tr("Week 3 — Sampling").into();
        assert!(titles(&merge(&m, &l3, &r3).unwrap()).contains(&crate::tr("Week 3 — Sampling").to_string()));
        // both sides added to Recently deleted: both kept
        let (mut l4, mut r4) = (m.clone(), m.clone());
        l4.trash.push(Trashed { label: "a".into(), deleted: 1, item: TrashItem::Notebook(l4.notebooks[1].clone()) });
        r4.trash.push(Trashed { label: "b".into(), deleted: 2, item: TrashItem::Notebook(r4.notebooks[1].clone()) });
        assert_eq!(merge(&m, &l4, &r4).unwrap().trash.len(), 2);
    }

    #[test]
    fn syncs_two_devices_through_a_folder() {
        let shared = tmp("shared");
        let (a, b) = (tmp("a"), tmp("b"));
        let cfg = SyncSettings { via: Via::Folder, folder: shared.display().to_string(), ..Default::default() };
        // device A starts, with a picture
        std::fs::create_dir_all(a.join("files")).unwrap();
        std::fs::write(a.join("files/123.png"), b"png").unwrap();
        let mut sa = sample();
        let Ok(Outcome::Synced { store: s1, up: 1, .. }) = run(&cfg, &a, &sa, None, false) else { panic!("first sync") };
        sa = s1;
        // device B has its own notes: it's asked first, then merges
        let sb = sample();
        assert!(matches!(run(&cfg, &b, &sb, None, false), Ok(Outcome::AskFirst { .. })));
        let Ok(Outcome::Synced { store: mut sb, down: 1, .. }) = run(&cfg, &b, &sb, Some(First::UseShared), false) else { panic!("join") };
        assert_eq!(std::fs::read(b.join("files/123.png")).unwrap(), b"png");
        assert!(same(&sa, &sb));
        // both edit different pages, then sync in turn: both edits end up everywhere
        page_mut(&mut sa, "Week 1 — Intro").title = "Intro (A)".into();
        page_mut(&mut sb, "Groceries").title = "Groceries (B)".into();
        let Ok(Outcome::Synced { store: sa2, .. }) = run(&cfg, &a, &sa, None, false) else { panic!() };
        let Ok(Outcome::Synced { store: sb2, .. }) = run(&cfg, &b, &sb, None, false) else { panic!() };
        let Ok(Outcome::Synced { store: sa3, .. }) = run(&cfg, &a, &sa2, None, false) else { panic!() };
        assert!(same(&sa3, &sb2));
        let t = titles(&sa3);
        assert!(t.contains(&"Intro (A)".into()) && t.contains(&"Groceries (B)".into()), "{t:?}");
        // a damaged shared file is never overwritten
        std::fs::write(shared.join("LeafNotes/notes.json"), b"{broken").unwrap();
        assert!(run(&cfg, &a, &sa3, None, false).is_err());
        assert_eq!(std::fs::read(shared.join("LeafNotes/notes.json")).unwrap(), b"{broken");
        // names from other devices can't escape the files folder
        assert!(!safe_name("../x") && !safe_name("a/b") && !safe_name(".hidden") && safe_name("123.png"));
        assert_eq!((dec("4%2F0Ab%3D+x"), dec("bad%2"), enc("a b/'")), ("4/0Ab= x".to_string(), "bad%2".to_string(), "a%20b%2F%27".to_string()));
        assert_eq!(id_email("x.eyJlbWFpbCI6ImFAYi5jIn0.y"), "a@b.c");
        // a folder that doesn't exist is refused, not created
        let missing = SyncSettings { folder: shared.join("nope").display().to_string(), ..cfg.clone() };
        assert!(run(&missing, &a, &sa3, None, false).is_err() && !shared.join("nope").exists());
        for d in [shared, a, b] { let _ = std::fs::remove_dir_all(d); }
    }
}
