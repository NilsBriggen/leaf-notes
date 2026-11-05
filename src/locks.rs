//! Password-locked sections (encryption in vault.rs). An opened section's key lives only in
//! memory for this session; everything written to disk, synced or backed up is the sealed form.
use crate::*;
use eframe::egui::RichText;

/// A section being locked: its id and the password typed twice.
#[derive(Clone, Default)]
pub(crate) struct LockPrompt { pub section: u64, pub pw: String, pub again: String, pub error: String }

fn pages_hash(pages: &[Page]) -> u64 {
    serde_json::to_vec(pages).unwrap_or_default().iter().fold(0xcbf29ce484222325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100000001b3))
}

impl NotesApp {
    fn section_mut(&mut self, id: u64) -> Option<&mut Section> { self.store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).find(|s| s.id == id) }

    /// The notes as they may leave memory: opened locked sections sealed again (re-encrypted only
    /// when their pages changed). None when nothing is locked, so the live notes can be used as is.
    pub(crate) fn sealed(&mut self) -> Option<Store> {
        if !self.store.notebooks.iter().flat_map(|n| &n.sections).any(|s| s.lock.is_some()) { return None; }
        let mut out = self.store.clone();
        for s in out.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()) {
            let Some(lock) = &s.lock else { continue };
            match self.st.keys.get(&s.id) {
                Some(key) => {
                    let h = pages_hash(&s.pages);
                    let sealed = match self.st.seal_cache.get(&s.id) {
                        Some((ch, l)) if *ch == h => l.clone(),
                        _ => match vault::seal(key, &lock.salt, &s.pages) {
                            Ok(l) => { self.st.seal_cache.insert(s.id, (h, l.clone())); l }
                            Err(_) => lock.clone(), // keep the last sealed version rather than write plain text
                        },
                    };
                    s.lock = Some(sealed);
                }
                None => {} // still sealed: the lock is already the saved form
            }
            // nothing but the placeholder ever leaves a locked section in plain text
            s.pages = vec![s.placeholder()];
        }
        Some(out)
    }

    /// After sync: open again the locked sections that are open in this session.
    pub(crate) fn reopen(&mut self, mut store: Store) -> Store {
        for s in store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()) {
            let (Some(lock), Some(key)) = (&s.lock, self.st.keys.get(&s.id)) else { continue };
            match vault::open(key, lock) {
                Ok(pages) => { self.st.seal_cache.insert(s.id, (pages_hash(&pages), lock.clone())); s.pages = pages; }
                Err(_) => { self.st.keys.remove(&s.id); } // changed elsewhere with another password: stays locked
            }
        }
        store
    }

    /// Encrypt a section with a new password and close it.
    pub(crate) fn lock_section(&mut self, id: u64, password: &str) -> Result<(), String> {
        let salt = vault::new_salt()?;
        let key = vault::key(password, &salt)?;
        let s = self.section_mut(id).ok_or("no such section")?;
        let lock = vault::seal(&key, &salt, &s.pages)?;
        let ids: Vec<u64> = s.pages.iter().map(|p| p.id).collect();
        s.lock = Some(lock);
        s.pages = vec![s.placeholder()];
        // earlier plain-text versions of these pages shouldn't outlive the lock
        if self.st.persist { for p in ids { let _ = std::fs::remove_file(data_dir().join("history").join(format!("{p}.jsonl"))); } }
        self.st.keys.remove(&id);
        self.after_lock_change();
        Ok(())
    }

    pub(crate) fn unlock_section(&mut self, id: u64, password: &str) -> Result<(), String> {
        let lock = self.section_mut(id).and_then(|s| s.lock.clone()).ok_or("not locked")?;
        let key = vault::key(password, &lock.salt)?;
        let pages = vault::open(&key, &lock).map_err(|_| tr("Wrong password.").to_string())?;
        self.st.seal_cache.insert(id, (pages_hash(&pages), lock));
        self.st.keys.insert(id, key);
        if let Some(s) = self.section_mut(id) { s.pages = if pages.is_empty() { vec![Page::new("")] } else { pages }; }
        self.after_lock_change();
        Ok(())
    }

    /// Close an opened locked section again.
    pub(crate) fn relock(&mut self, id: u64) {
        if let Some(sealed) = self.sealed() {
            let lock = sealed.notebooks.iter().flat_map(|n| &n.sections).find(|s| s.id == id).and_then(|s| s.lock.clone());
            if let (Some(lock), Some(s)) = (lock, self.section_mut(id)) { s.lock = Some(lock); s.pages = vec![s.placeholder()]; }
        }
        self.st.keys.remove(&id);
        self.after_lock_change();
    }

    /// Take the lock off an opened section (its pages become ordinary pages again).
    pub(crate) fn remove_lock(&mut self, id: u64) {
        if !self.st.keys.contains_key(&id) { return; }
        if let Some(s) = self.section_mut(id) { s.lock = None; }
        self.st.keys.remove(&id);
        self.after_lock_change();
    }

    fn after_lock_change(&mut self) {
        self.st.selected.clear();
        self.st.editing = None;
        self.st.undo.clear(); // undo snapshots could hold pages of a section that is now closed
        self.st.redo.clear();
        self.clamp();
        self.st.dirty = true;
    }

    /// New pages can't go into a closed locked section.
    pub(crate) fn section_open_for_writing(&mut self) -> bool {
        if self.section().sealed() { self.st.say("Unlock this section first."); false } else { true }
    }

    pub(crate) fn lock_window(&mut self, ctx: &egui::Context) {
        let Some(mut lp) = self.st.lock_prompt.take() else { return };
        let (mut go, mut cancel) = (false, false);
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
        let modal = egui::Modal::new(egui::Id::unique("lock-section")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 420.0));
            ui.label(RichText::new(tr("Lock this section")).font(rich::serif(22.0)).color(ui.visuals().strong_text_color()));
            ui.add_space(6.0);
            ui.label(tr("Its pages are encrypted with this password. You'll need it to open them, also on your other devices."));
            ui.label(RichText::new(tr("If you forget the password, these pages can't be recovered. Nobody can reset it.")).color(Color32::from_rgb(200, 60, 50)));
            ui.add_space(8.0);
            ui.add(egui::TextEdit::singleline(&mut lp.pw).password(true).hint_text(tr("Password")).desired_width(f32::INFINITY));
            let r = ui.add(egui::TextEdit::singleline(&mut lp.again).password(true).hint_text(tr("The same password again")).desired_width(f32::INFINITY));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { go = true; }
            if !lp.error.is_empty() { ui.label(RichText::new(&lp.error).color(Color32::from_rgb(200, 60, 50))); }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(tr("🔒  Lock")).color(Color32::WHITE)).fill(accent_base())).clicked() { go = true; }
                if ui.button(tr("Cancel")).clicked() { cancel = true; }
            });
        });
        if cancel || modal.should_close() { return; }
        if go {
            lp.error = if lp.pw.chars().count() < 6 { tr("Use at least 6 characters.").into() } else if lp.pw != lp.again { tr("The two passwords don't match.").into() } else { String::new() };
            if lp.error.is_empty() {
                match self.lock_section(lp.section, &lp.pw) {
                    Ok(()) => { self.st.say("Section locked. Its pages are encrypted."); return; }
                    Err(e) => lp.error = e,
                }
            }
        }
        self.st.lock_prompt = Some(lp);
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
        assert!(!serde_json::to_string(&app.store).unwrap().contains(&secret), "nothing readable while locked");
        assert!(app.unlock_section(sec.id, "wrong").is_err());
        app.unlock_section(sec.id, "hunter22").unwrap();
        assert_eq!(app.store.notebooks[0].sections[0].pages[0].title, secret);
        // while open, what is saved or synced is still sealed, and unchanged pages aren't re-encrypted
        let a = app.sealed().unwrap();
        assert!(!serde_json::to_string(&a).unwrap().contains(&secret));
        let b = app.sealed().unwrap();
        assert_eq!(a.notebooks[0].sections[0].lock, b.notebooks[0].sections[0].lock);
        // an edit is sealed with it; after "sync" the section opens again
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
}
