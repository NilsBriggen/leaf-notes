//! Recording transcripts: the audio goes to the AI service; the text comes back as paragraphs on
//! the page, each linked to its moment (tapping it plays from there).
use crate::*;

impl NotesApp {
    pub(crate) fn start_transcribe(&mut self, rec_id: u64) {
        if self.st.transcribe.is_some() { self.st.say("A transcript is already being made."); return; }
        if !self.st.ai.ready() { self.st.say("Add your AI key first — it stays on this device."); return; }
        let page = self.page();
        let Some(rec) = page.recordings.iter().find(|r| r.id == rec_id).cloned() else { return };
        let (pid, path, ai) = (page.id, data_dir().join("files").join(&rec.file), self.st.ai.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let r = audio::load_wav(&path).and_then(|samples| ai::transcribe(&ai, &samples, audio::RATE));
            let _ = tx.send(r);
        });
        self.st.transcribe = Some((pid, rec.start, rx));
        self.st.say("Making a transcript…");
    }

    pub(crate) fn poll_transcribe(&mut self) {
        let Some((_, _, rx)) = &self.st.transcribe else { return };
        let r = match rx.try_recv() { Ok(r) => r, Err(std::sync::mpsc::TryRecvError::Empty) => return, Err(_) => Err("the transcript stopped".into()) };
        let Some((pid, start, _)) = self.st.transcribe.take() else { return };
        let segs = match r { Ok(s) => s, Err(e) => { self.st.say(tf!("⚠ No transcript: {e}", e = e)); return; } };
        if segs.is_empty() { self.st.say("The recording had no speech to write down."); return; }
        let heights = std::mem::take(&mut self.st.heights);
        let width = self.st.p.note_width;
        let Some(page) = self.find_page(pid) else { self.st.heights = heights; return };
        let snap = page.clone();
        let mut y = page.blocks.iter().map(|b| b.pos[1] + heights.get(&b.id).copied().unwrap_or(40.0) + 18.0).fold(8.0, f32::max) + 20.0;
        let mut add = |kind: Kind, t: f64, lines: f32| {
            let mut b = block(24.0, y, width, kind);
            b.t = t;
            y += 26.0 * lines + 16.0;
            page.blocks.push(b);
        };
        add(Kind::Text { text: tf!("Transcript · {when}", when = chrome::format_date(start as u64, true)), heading: 2 }, start, 1.0);
        let paras = ai::paragraphs(&segs);
        let n = paras.len();
        for (t, text) in paras {
            let lines = 1.0 + text.len() as f32 / 70.0;
            add(Kind::Text { text, heading: 0 }, start + t as f64, lines);
        }
        self.st.heights = heights;
        self.st.snap(&snap);
        self.st.dirty = true;
        self.st.say(tf!("Added a transcript in {n} paragraphs — tap one to hear that moment.", n = n));
    }
}
