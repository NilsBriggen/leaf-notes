//! Password-locked sections (encryption in vault.rs). An opened section's key lives only in
//! memory for this session; everything written to disk, synced or backed up is the sealed form.
use crate::chrome::danger_color;
use crate::*;
use eframe::egui::RichText;
use zeroize::{Zeroize, Zeroizing};

/// A section being locked: its id and the password typed twice.
#[derive(Clone, Default)]
pub(crate) struct LockPrompt {
    pub section: u64,
    pub pw: String,
    pub again: String,
    pub error: String,
}

impl LockPrompt {
    pub(crate) fn for_section(section: u64) -> Self {
        Self { section, pw: String::new(), again: String::new(), error: String::new() }
    }
}

impl Drop for LockPrompt {
    fn drop(&mut self) {
        self.pw.zeroize();
        self.again.zeroize();
    }
}

fn pages_hash(pages: &[Page]) -> u64 {
    serde_json::to_vec(pages).unwrap_or_default().iter().fold(0xcbf29ce484222325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100000001b3))
}

/// Older backups, sync bases, and safety copies may still hold a newly locked section in plain text:
/// replace every unsealed occurrence (including inside `TrashItem::Notebook`) with the sealed form
/// and drop its deleted pages from their bins. Unreadable files stay untouched.
pub(crate) fn lock_in_backups(dir: &std::path::Path, sec: &Section) {
    const FIXED: &[&str] = &[
        "sync-base.json",
        "library.json.backup",
        "notebooks.json.before-split",
        "notes.prev.json",
    ];
    let named = |p: &std::path::Path| {
        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
            FIXED.contains(&n)
                || (n.ends_with(".json") && (n.starts_with("before-restore-") || n.starts_with("before-sync-")))
        })
    };
    let files = std::fs::read_dir(dir.join("backups")).into_iter().flatten().flatten().map(|e| e.path())
        .chain(std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| named(p)));
    for f in files {
        let Ok(mut s) = std::fs::read(&f).map_err(|_| ()).and_then(|b| serde_json::from_slice::<Store>(&b).map_err(|_| ())) else { continue };
        let mut changed = false;
        for x in s.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()) {
            if x.id == sec.id && !x.sealed() {
                *x = sec.clone();
                changed = true;
            }
        }
        for t in &mut s.trash {
            match &mut t.item {
                TrashItem::Section { section, .. } => {
                    if section.id == sec.id && !section.sealed() {
                        *section = sec.clone();
                        changed = true;
                    }
                }
                TrashItem::Notebook(nb) => {
                    for x in &mut nb.sections {
                        if x.id == sec.id && !x.sealed() {
                            *x = sec.clone();
                            changed = true;
                        }
                    }
                }
                TrashItem::Page { .. } => {}
            }
        }
        let n = s.trash.len();
        s.trash.retain(|t| !matches!(&t.item, TrashItem::Page { section, .. } if *section == sec.id));
        if changed || s.trash.len() != n {
            if let Ok(b) = serde_json::to_vec(&s) { let _ = crate::storage::write_atomic(&f, &b); }
        }
    }
}

impl NotesApp {
    fn section_mut(&mut self, id: u64) -> Option<&mut Section> {
        self.store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).find(|s| s.id == id)
    }

    /// The notes as they may leave memory: opened locked sections sealed again (re-encrypted only
    /// when their pages changed). None when nothing is locked, so the live notes can be used as is.
    pub(crate) fn sealed(&mut self) -> Option<Store> {
        if !self.store.notebooks.iter().flat_map(|n| &n.sections).any(|s| s.lock.is_some()) { return None; }
        let mut out = self.store.clone();
        for s in out.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()) {
            let Some(lock) = &s.lock else { continue };
            if let Some((salt, key)) = self.st.locks.keys.get(&s.id) {
                s.locked_files = collect_page_files(&s.pages);
                let h = pages_hash(&s.pages);
                let sealed = match self.st.locks.seal_cache.get(&s.id) {
                    Some((ch, l)) if *ch == h && l.salt == *salt && l.v == vault::V_ARGON2 => l.clone(),
                    _ => match vault::seal(key, s.id, salt, &s.pages) {
                        Ok(l) => { self.st.locks.seal_cache.insert(s.id, (h, l.clone())); l }
                        Err(_) => {
                            self.st.say("⚠ A locked section couldn't be encrypted, so its latest changes aren't saved yet.");
                            self.st.dirty = true;
                            lock.clone()
                        }
                    },
                };
                s.lock = Some(sealed);
            }
            s.pages = vec![s.placeholder()];
        }
        Some(out)
    }

    /// After sync: open again the locked sections that are open in this session.
    pub(crate) fn reopen(&mut self, mut store: Store) -> Store {
        for s in store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()) {
            let (Some(lock), Some((salt, key))) = (&s.lock, self.st.locks.keys.get(&s.id)) else { continue };
            if *salt != lock.salt {
                // Salt changed (e.g. re-keyed on another device): require password again.
                self.st.locks.keys.remove(&s.id);
                self.st.locks.seal_cache.remove(&s.id);
                continue;
            }
            match vault::open(key, s.id, lock) {
                Ok(pages) => {
                    s.locked_files = collect_page_files(&pages);
                    self.st.locks.seal_cache.insert(s.id, (pages_hash(&pages), lock.clone()));
                    s.pages = pages;
                }
                Err(_) => {
                    self.st.locks.keys.remove(&s.id);
                    self.st.locks.seal_cache.remove(&s.id);
                }
            }
        }
        store
    }

    /// Finish locking a section once the key and sealed blob have been derived.
    fn finish_lock_section(&mut self, id: u64, salt: String, key: vault::Key, lock: Lock) -> Result<(), String> {
        let s = self.section_mut(id).ok_or("no such section")?;
        let mut ids: Vec<u64> = s.pages.iter().map(|p| p.id).collect();
        s.locked_files = collect_page_files(&s.pages);
        s.lock = Some(lock);
        s.pages = vec![s.placeholder()];
        let (name, color, locked) = (s.name.clone(), s.color, s.clone());
        let nb_name = self.store.notebooks.iter().find(|n| n.sections.iter().any(|s| s.id == id)).map(|n| n.name.clone()).unwrap_or_default();
        for t in self.store.trash.iter_mut() {
            let TrashItem::Page { notebook, section, page } = &t.item else { continue };
            if *section != id { continue; }
            let bin_id = new_id();
            let Ok(l) = vault::seal(&key, bin_id, &salt, std::slice::from_ref(page)) else { continue };
            ids.push(page.id);
            let mut sec = Section {
                id: bin_id,
                name: tf!("{name} (deleted page)", name = name),
                color,
                pages: vec![],
                lock: Some(l),
                locked_files: collect_page_files(std::slice::from_ref(page)),
            };
            sec.pages = vec![sec.placeholder()];
            t.item = TrashItem::Section { notebook: *notebook, section: sec };
            t.label = tf!("A page from {nb} › {name} (locked)", nb = nb_name, name = name);
            t.at = None;
        }
        if self.st.persist {
            // Flush any queued background save before scrubbing disk copies so a stale write can't resurrect plaintext.
            if let Some(w) = &self.st.save_worker { w.flush(); }
            let dir = data_dir();
            for p in &ids {
                let _ = std::fs::remove_file(dir.join("pages").join(format!("{p}.json")));
                let _ = std::fs::remove_file(dir.join("history").join(format!("{p}.jsonl")));
            }
            lock_in_backups(&dir, &locked);
        }
        self.st.index.forget(&ids, self.st.persist);
        self.st.locks.keys.remove(&id);
        self.st.locks.seal_cache.remove(&id);
        self.after_lock_change();
        Ok(())
    }

    /// Encrypt a section with a new password and close it (synchronous — used in tests).
    #[cfg(test)]
    pub(crate) fn lock_section(&mut self, id: u64, password: &str) -> Result<(), String> {
        let pages = self.section_mut(id).ok_or("no such section")?.pages.clone();
        let salt = vault::new_salt()?;
        let key = vault::key(password, &salt)?;
        let lock = vault::seal(&key, id, &salt, &pages)?;
        self.finish_lock_section(id, salt, key, lock)
    }

    /// Start encrypting a section on a background thread so Argon2id never stalls the UI.
    pub(crate) fn start_lock_section(&mut self, id: u64, password: String) {
        if self.st.locks.pending.is_some() { return; }
        let Some(pages) = self.section_mut(id).map(|s| s.pages.clone()) else { return };
        let pw = Zeroizing::new(password);
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let res = (|| {
                let salt = vault::new_salt()?;
                let key = vault::key(&pw, &salt)?;
                let lock = vault::seal(&key, id, &salt, &pages)?;
                Ok((salt, key, lock))
            })();
            let _ = tx.send(res);
        });
        self.st.locks.pending = Some(PendingLockOp::Lock { section: id, rx });
    }

    fn finish_unlock_section(&mut self, id: u64, salt: String, key: vault::Key, pages: Vec<Page>, upgraded: Option<Lock>) {
        let locked_files = collect_page_files(&pages);
        let pages_nonempty = if pages.is_empty() { vec![Page::new("")] } else { pages };
        let h = pages_hash(&pages_nonempty);
        let cached_lock = if let Some(s) = self.section_mut(id) {
            let lock_for_cache = if let Some(new_lock) = upgraded {
                s.lock = Some(new_lock.clone());
                Some(new_lock)
            } else {
                s.lock.clone()
            };
            s.locked_files = locked_files;
            s.pages = pages_nonempty;
            lock_for_cache
        } else {
            None
        };
        if let Some(l) = cached_lock {
            self.st.locks.seal_cache.insert(id, (h, l));
        }
        self.st.locks.keys.insert(id, (salt, key));
        self.st.locks.unlock_err.clear();
        self.after_lock_change();
    }

    /// Decrypt and open a locked section synchronously (used in tests; UI uses `start_unlock_section`).
    #[cfg(test)]
    pub(crate) fn unlock_section(&mut self, id: u64, password: &str) -> Result<(), String> {
        let lock = self.section_mut(id).and_then(|s| s.lock.clone()).ok_or("not locked")?;
        let (salt, key, pages, upgraded) = unlock_worker(id, &lock, password)?;
        self.finish_unlock_section(id, salt, key, pages, upgraded);
        Ok(())
    }

    /// Start decrypting a locked section on a background thread so Argon2id never stalls the UI.
    pub(crate) fn start_unlock_section(&mut self, id: u64, password: String) {
        if self.st.locks.pending.is_some() { return; }
        let Some(lock) = self.section_mut(id).and_then(|s| s.lock.clone()) else { return };
        let pw = Zeroizing::new(password);
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(unlock_worker(id, &lock, &pw));
        });
        self.st.locks.unlock_err.clear();
        self.st.locks.pending = Some(PendingLockOp::Unlock { section: id, rx });
    }

    /// Poll background lock/unlock worker.
    pub(crate) fn poll_locks(&mut self, ctx: &egui::Context) {
        let Some(op) = self.st.locks.pending.take() else { return };
        match op {
            PendingLockOp::Unlock { section, rx } => match rx.try_recv() {
                Ok(Ok((salt, key, pages, upgraded))) => {
                    self.finish_unlock_section(section, salt, key, pages, upgraded);
                    self.st.say("Section unlocked until you lock it again or close Leaf Notes.");
                }
                Ok(Err(e)) => {
                    self.st.locks.unlock_err = e;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.st.locks.pending = Some(PendingLockOp::Unlock { section, rx });
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                }
                Err(_) => {
                    self.st.locks.unlock_err = tr("That stopped unexpectedly — please try again.").to_string();
                }
            },
            PendingLockOp::Lock { section, rx } => match rx.try_recv() {
                Ok(Ok((salt, key, lock))) => {
                    match self.finish_lock_section(section, salt, key, lock) {
                        Ok(()) => {
                            if matches!(self.st.modals.top(), Some(ActiveModal::LockPrompt(_))) {
                                self.st.modals.pop();
                            }
                            self.st.say("Section locked. Its pages are encrypted.");
                        }
                        Err(e) => {
                            self.st.say(format!("⚠ {e}"));
                        }
                    }
                }
                Ok(Err(e)) => {
                    self.st.say(format!("⚠ {e}"));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.st.locks.pending = Some(PendingLockOp::Lock { section, rx });
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                }
                Err(_) => {
                    self.st.say("That stopped unexpectedly — please try again.");
                }
            },
        }
    }

    /// A locked section on its way to Recently deleted: sealed now, so the bin never holds its pages in plain text.
    pub(crate) fn close_for_bin(&mut self, s: &mut Section) {
        if s.lock.is_none() { return; }
        if let Some((salt, key)) = self.st.locks.keys.remove(&s.id) {
            s.locked_files = collect_page_files(&s.pages);
            if let Ok(l) = vault::seal(&key, s.id, &salt, &s.pages) { s.lock = Some(l); }
            self.st.locks.seal_cache.remove(&s.id);
        }
        s.pages = vec![s.placeholder()];
    }

    /// A page deleted from an opened locked section, as a one-page locked section with the same password.
    #[allow(clippy::result_large_err)]
    pub(crate) fn sealed_page_for_bin(&mut self, from: &Section, page: Page) -> Result<Section, Page> {
        let (Some(_), Some((salt, key))) = (&from.lock, self.st.locks.keys.get(&from.id)) else { return Err(page) };
        let bin_id = new_id();
        let Ok(l) = vault::seal(key, bin_id, salt, std::slice::from_ref(&page)) else { return Err(page) };
        let mut s = Section {
            id: bin_id,
            name: tf!("{name} (deleted page)", name = from.name),
            color: from.color,
            pages: vec![],
            lock: Some(l),
            locked_files: collect_page_files(std::slice::from_ref(&page)),
        };
        s.pages = vec![s.placeholder()];
        Ok(s)
    }

    /// Close an opened locked section again.
    pub(crate) fn relock(&mut self, id: u64) {
        if let Some(sealed) = self.sealed() {
            let found = sealed.notebooks.iter().flat_map(|n| &n.sections).find(|s| s.id == id)
                .map(|s| (s.lock.clone(), s.locked_files.clone()));
            if let (Some((Some(lock), lf)), Some(s)) = (found, self.section_mut(id)) {
                s.lock = Some(lock);
                s.locked_files = lf;
                s.pages = vec![s.placeholder()];
            }
        }
        self.st.locks.keys.remove(&id);
        self.st.locks.seal_cache.remove(&id);
        self.after_lock_change();
    }

    /// Take the lock off an opened section (its pages become ordinary pages again).
    pub(crate) fn remove_lock(&mut self, id: u64) {
        if !self.st.locks.keys.contains_key(&id) { return; }
        if let Some(s) = self.section_mut(id) {
            s.lock = None;
            s.locked_files.clear();
        }
        self.st.locks.keys.remove(&id);
        self.st.locks.seal_cache.remove(&id);
        self.after_lock_change();
    }

    fn after_lock_change(&mut self) {
        crate::sync::dedupe(&mut self.store);
        self.st.canvas.selected.clear();
        self.st.canvas.editing = None;
        self.st.undo.clear();
        self.st.redo.clear();
        // Scrub all in-memory plaintext caches so locked section snippets/titles never linger.
        self.st.cache.search_cache = None;
        self.st.cache.switch_cache = None;
        self.st.cache.backlinks.clear();
        self.st.cache.page_titles.clear();
        self.st.locks.unlock_pw.zeroize();
        self.st.cache.derived_dirty = true;
        self.clamp();
        self.st.dirty = true;
    }

    /// New pages can't go into a closed locked section.
    pub(crate) fn section_open_for_writing(&mut self) -> bool {
        if self.section().sealed() { self.st.say("Unlock this section first."); false } else { true }
    }

    pub(crate) fn lock_window(&mut self, ctx: &egui::Context, mut lp: LockPrompt) -> Option<LockPrompt> {
        let busy = matches!(self.st.locks.pending, Some(PendingLockOp::Lock { .. }));
        let (mut go, mut cancel) = (false, false);
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
        let modal = egui::Modal::new(egui::Id::unique("lock-section")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 420.0));
            ui.label(RichText::new(tr("Lock this section")).font(rich::serif(22.0)).color(ui.visuals().strong_text_color()));
            ui.add_space(6.0);
            ui.label(tr("Its pages are encrypted with this password. You'll need it to open them, also on your other devices."));
            ui.label(RichText::new(tr("If you forget the password, these pages can't be recovered. Nobody can reset it.")).color(danger_color(ui)));
            ui.label(RichText::new(tr("Pictures, PDFs and recordings on these pages are not encrypted.")).weak());
            ui.add_space(8.0);
            ui.add_enabled_ui(!busy, |ui| {
                ui.add(egui::TextEdit::singleline(&mut lp.pw).password(true).hint_text(tr("Password")).desired_width(f32::INFINITY));
                let r = ui.add(egui::TextEdit::singleline(&mut lp.again).password(true).hint_text(tr("The same password again")).desired_width(f32::INFINITY));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { go = true; }
            });
            if !lp.error.is_empty() { ui.label(RichText::new(&lp.error).color(danger_color(ui))); }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if busy {
                    ui.spinner();
                    ui.label(tr("Encrypting…"));
                } else {
                    if ui.add(egui::Button::new(RichText::new(tr("🔒  Lock")).color(Color32::WHITE)).fill(accent_base())).clicked() { go = true; }
                    if ui.button(tr("Cancel")).clicked() { cancel = true; }
                }
            });
        });
        if !busy && (cancel || modal.should_close()) { return None; }
        if !busy && go {
            lp.error = if lp.pw.chars().count() < 6 {
                tr("Use at least 6 characters.").into()
            } else if lp.pw != lp.again {
                tr("The two passwords don't match.").into()
            } else {
                String::new()
            };
            if lp.error.is_empty() {
                let pw = std::mem::take(&mut lp.pw);
                lp.again.zeroize();
                self.start_lock_section(lp.section, pw);
            }
        }
        Some(lp)
    }
}

fn unlock_worker(
    section_id: u64,
    lock: &Lock,
    password: &str,
) -> Result<(String, vault::Key, Vec<Page>, Option<Lock>), String> {
    let key = vault::derive_key(password, &lock.salt, lock.v)?;
    let pages = vault::open(&key, section_id, lock).map_err(|_| tr("Wrong password.").to_string())?;
    if lock.v == vault::V_PBKDF2 {
        // Transparently upgrade legacy PBKDF2 v0 sections to Argon2id + AAD v1 on unlock.
        let new_salt = vault::new_salt()?;
        let new_key = vault::derive_key(password, &new_salt, vault::V_ARGON2)?;
        let new_lock = vault::seal(&new_key, section_id, &new_salt, &pages)?;
        Ok((new_salt, new_key, pages, Some(new_lock)))
    } else {
        Ok((lock.salt.clone(), key, pages, None))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn locked_sections_never_leave_memory_in_plain_text() {
        let mut app = crate::safety::tests::app();
        let sec = app.store.notebooks[0].sections[0].clone();
        let secret = sec.pages[0].title.clone();
        app.lock_section(sec.id, "hunter22").unwrap();
        let s = &app.store.notebooks[0].sections[0];
        assert!(s.sealed() && s.pages.len() == 1);
        assert_eq!(s.lock.as_ref().unwrap().v, crate::vault::V_ARGON2);
        assert!(!serde_json::to_string(&app.store).unwrap().contains(&secret), "nothing readable while locked");
        assert!(app.unlock_section(sec.id, "wrong").is_err());
        app.unlock_section(sec.id, "hunter22").unwrap();
        assert_eq!(app.store.notebooks[0].sections[0].pages[0].title, secret);
        let a = app.sealed().unwrap();
        assert!(!serde_json::to_string(&a).unwrap().contains(&secret));
        let b = app.sealed().unwrap();
        assert_eq!(a.notebooks[0].sections[0].lock, b.notebooks[0].sections[0].lock);
        app.store.notebooks[0].sections[0].pages[0].title = "Changed".into();
        let c = app.sealed().unwrap();
        assert_ne!(c.notebooks[0].sections[0].lock, a.notebooks[0].sections[0].lock);
        let back = app.reopen(c);
        assert_eq!(back.notebooks[0].sections[0].pages[0].title, "Changed");
        app.relock(sec.id);
        assert!(app.store.notebooks[0].sections[0].sealed());
        app.unlock_section(sec.id, "hunter22").unwrap();
        assert_eq!(app.store.notebooks[0].sections[0].pages[0].title, "Changed");
        app.remove_lock(sec.id);
        assert!(app.store.notebooks[0].sections[0].lock.is_none() && app.sealed().is_none());
    }

    #[test]
    fn legacy_v0_pbkdf2_section_upgrades_to_v1_argon2_on_unlock() {
        let mut app = crate::safety::tests::app();
        let sec = &mut app.store.notebooks[0].sections[0];
        let sec_id = sec.id;
        let secret = sec.pages[0].title.clone();
        let salt = crate::vault::new_salt().unwrap();
        let old_key = crate::vault::derive_key("legacy99", &salt, crate::vault::V_PBKDF2).unwrap();
        let v0_lock = crate::vault::seal_v0_for_test(&old_key, &salt, &sec.pages).unwrap();
        sec.lock = Some(v0_lock);
        sec.pages = vec![sec.placeholder()];

        app.unlock_section(sec_id, "legacy99").unwrap();
        let s = &app.store.notebooks[0].sections[0];
        assert_eq!(s.pages[0].title, secret);
        assert_eq!(s.lock.as_ref().unwrap().v, crate::vault::V_ARGON2, "upgraded to Argon2id v1");
        app.relock(sec_id);
        app.unlock_section(sec_id, "legacy99").unwrap();
        assert_eq!(app.store.notebooks[0].sections[0].pages[0].title, secret);
    }

    #[test]
    fn deleting_from_an_opened_locked_section_keeps_it_encrypted() {
        let mut app = crate::safety::tests::app();
        let sec = app.store.notebooks[0].sections[0].clone();
        let (page, secret) = (sec.pages[0].id, sec.pages[0].title.clone());
        app.lock_section(sec.id, "hunter22").unwrap();
        app.unlock_section(sec.id, "hunter22").unwrap();
        app.delete(page);
        assert!(!serde_json::to_string(&app.store.trash).unwrap().contains(&secret), "the bin holds it encrypted");
        let crate::TrashItem::Section { section, .. } = &app.store.trash.last().unwrap().item else { panic!("kept as a locked section") };
        assert!(section.sealed());
        let (bin_id, lock) = (section.id, section.lock.clone().unwrap());
        let k = crate::vault::derive_key("hunter22", &lock.salt, lock.v).unwrap();
        assert_eq!(crate::vault::open(&k, bin_id, &lock).unwrap()[0].id, page, "same password opens it");
        let i = app.store.trash.len() - 1;
        app.restore(i);
        app.unlock_section(bin_id, "hunter22").unwrap();
        let other = app.store.notebooks[0].sections[0].pages.iter().map(|p| p.title.clone()).find(|t| !t.is_empty()).unwrap_or_default();
        app.delete(sec.id);
        let bin = serde_json::to_string(&app.store.trash).unwrap();
        assert!(other.is_empty() || !bin.contains(&other));
        let crate::TrashItem::Section { section, .. } = &app.store.trash.last().unwrap().item else { panic!() };
        assert!(section.sealed());
    }

    #[test]
    fn locking_also_locks_older_backups_and_sync_base() {
        let mut app = crate::safety::tests::app();
        let sec = app.store.notebooks[0].sections[0].clone();
        let secret = sec.pages[0].title.clone();
        let dir = std::env::temp_dir().join(format!("ln-lockbk-{}", crate::new_id()));
        std::fs::create_dir_all(dir.join("backups")).unwrap();
        let mut old = app.store.clone();
        old.trash.push(crate::Trashed { label: "x".into(), deleted: 1, item: crate::TrashItem::Page { notebook: 0, section: sec.id, page: sec.pages[0].clone() }, at: None });
        old.trash.push(crate::Trashed { label: "nb".into(), deleted: 2, item: crate::TrashItem::Notebook(app.store.notebooks[0].clone()), at: None });
        let bytes = serde_json::to_vec(&old).unwrap();
        let targets = [
            "backups/2026-01-01.json",
            "before-sync-1.json",
            "before-restore-1.json",
            "sync-base.json",
            "library.json.backup",
            "notebooks.json.before-split",
            "notes.prev.json",
        ];
        for f in targets { std::fs::write(dir.join(f), &bytes).unwrap(); }
        std::fs::write(dir.join("backups/broken.json"), b"{").unwrap();
        app.lock_section(sec.id, "hunter22").unwrap();
        super::lock_in_backups(&dir, &app.store.notebooks[0].sections[0]);
        for f in targets {
            let t = std::fs::read_to_string(dir.join(f)).unwrap();
            assert!(!t.contains(&secret), "{f} still readable");
            let s: crate::Store = serde_json::from_str(&t).unwrap();
            assert!(s.notebooks[0].sections[0].sealed());
            assert_eq!(s.trash.len(), 1);
            let crate::TrashItem::Notebook(nb) = &s.trash[0].item else { panic!() };
            assert!(nb.sections[0].sealed());
        }
        assert_eq!(std::fs::read(dir.join("backups/broken.json")).unwrap(), b"{");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locking_seals_pages_deleted_from_it_earlier() {
        let mut app = crate::safety::tests::app();
        let sec = app.store.notebooks[0].sections[0].clone();
        let (page, secret) = (sec.pages[0].id, sec.pages[0].title.clone());
        assert!(!secret.is_empty());
        app.delete(page);
        assert!(serde_json::to_string(&app.store.trash).unwrap().contains(&secret));
        app.lock_section(sec.id, "hunter22").unwrap();
        assert!(!serde_json::to_string(&app.store.trash).unwrap().contains(&secret), "the bin holds it encrypted too");
        let crate::TrashItem::Section { section, .. } = &app.store.trash.last().unwrap().item else { panic!("kept as a locked section") };
        assert!(section.sealed());
    }

    #[test]
    fn closed_locked_sections_take_no_pages() {
        let mut app = crate::safety::tests::app();
        let target = app.store.notebooks[0].sections[0].id;
        let page = app.store.notebooks[0].sections[1].pages[0].id;
        app.delete(page);
        app.lock_section(target, "hunter22").unwrap();
        (app.nb, app.sec, app.pg) = (0, 0, 0);
        let n = app.store.trash.len();
        if let crate::TrashItem::Page { section, .. } = &mut app.store.trash[n - 1].item { *section = target; }
        app.restore(n - 1);
        assert_eq!(app.store.trash.len(), n, "stays in the bin");
        assert!(app.store.notebooks[0].sections[0].sealed() && app.store.notebooks[0].sections[0].pages.len() == 1);
        assert!(!app.new_page(None));
    }
}
