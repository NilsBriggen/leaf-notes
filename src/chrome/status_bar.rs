//! Bottom status bar, focus timer, and cross-page global audio mini-player bar.

use super::{ago, danger_color};
use crate::*;
use eframe::egui::{Align, Layout, RichText};

impl NotesApp {
    /// Compact global mini-player shown when audio continues playing after leaving its source page.
    pub(crate) fn mini_player_bar(&mut self, ui: &mut egui::Ui) {
        let cur_id = self.store.notebooks[self.nb].sections[self.sec].pages[self.pg].id;
        let Some((pid, ref title)) = self.st.audio.player_page else { return };
        if pid == cur_id || self.st.audio.player.is_none() { return; }
        let title = title.clone();
        let mut jump_back = false;
        let mut stop_close = false;
        egui::Panel::bottom("mini-player")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(ui.visuals().faint_bg_color).stroke(ui.visuals().widgets.noninteractive.bg_stroke).inner_margin(egui::Margin::symmetric(14, 4)))
            .show(ui, |ui| {
                ui.spacing_mut().interact_size.y = 24.0;
                ui.horizontal(|ui| {
                    let Some(pl) = &self.st.audio.player else { return };
                    let playing = pl.playing();
                    if ui.add(egui::Button::new(if playing { "⏸" } else { "▶" }).frame_when_inactive(false)).tip(if playing { "Pause" } else { "Play" }).clicked() {
                        pl.set_playing(!playing);
                    }
                    let mut t = pl.time();
                    let dur = pl.len.max(0.1);
                    let fmt = |s: f32| format!("{:02}:{:02}", s as u64 / 60, s as u64 % 60);
                    ui.label(RichText::new(format!("{} / {}", fmt(t), fmt(dur))).size(11.5).monospace());
                    let sw = (ui.available_width() - 180.0).clamp(60.0, 220.0);
                    if ui.available_width() > 210.0 {
                        ui.spacing_mut().slider_width = sw;
                        if ui.add(egui::Slider::new(&mut t, 0.0..=dur).show_value(false)).changed() {
                            pl.seek(t);
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new("✕").frame_when_inactive(false)).tip("Close").clicked() {
                            stop_close = true;
                        }
                        let label = if title.trim().is_empty() { tr("Untitled page").to_string() } else { title.clone() };
                        if ui.add(egui::Button::new(RichText::new(format!("🔊  {label}")).size(12.0).color(accent(ui))).truncate().frame_when_inactive(false)).tip("Open this page").clicked() {
                            jump_back = true;
                        }
                    });
                });
            });
        if stop_close {
            self.st.audio.player = None;
            self.st.audio.player_page = None;
        } else if jump_back {
            if let Some((a, b, c)) = self.locate(pid) {
                self.go_to(a, b, c);
            }
        }
    }

    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        self.mini_player_bar(ui);
        egui::Panel::bottom("status")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin { left: 18, right: 18, top: 0, bottom: 3 }))
            .show(ui, |ui| {
                ui.spacing_mut().interact_size.y = 18.0;
                ui.spacing_mut().button_padding = Vec2::new(6.0, 1.0);
                let full_w = ui.available_width();
                let msg = !self.st.status.is_empty() && self.st.status_age < 10.0;
                // On narrow phones with an active toast/error, prioritize showing the toast cleanly.
                let phone_toast = msg && full_w < 500.0;
                ui.horizontal(|ui| {
                    let page = &self.store.notebooks[self.nb].sections[self.sec].pages[self.pg];
                    let is_deck = page.kind == PageKind::Deck;
                    let words: usize = page.blocks.iter().map(|b| b.kind.summary().split_whitespace().count().max(match &b.kind { Kind::Text { text, .. } => text.split_whitespace().count(), _ => 0 })).sum();
                    let info = if is_deck {
                        let cards = crate::i18n::fill(tr(if page.cards.len() == 1 { "{n} card" } else { "{n} cards" }), &[("n", &page.cards.len())]);
                        tf!("{cards} · {ready} ready to study", cards = cards, ready = page.deck().len())
                    } else if full_w < 440.0 {
                        // Compact phone stats so word/note/stroke counts never truncate mid-word on 412 px screens.
                        format!("{}w · {}n · {}✎", words, page.blocks.len(), page.strokes.len())
                    } else {
                        [(words, tr("{n} word"), tr("{n} words")), (page.blocks.len(), tr("{n} note"), tr("{n} notes")), (page.strokes.len(), tr("{n} ink stroke"), tr("{n} ink strokes"))]
                            .iter()
                            .map(|(n, one, many)| crate::i18n::fill(if *n == 1 { one } else { many }, &[("n", n)]))
                            .collect::<Vec<_>>()
                            .join(" · ")
                    };
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !phone_toast {
                            ui.label(RichText::new(tr(if !self.st.persist { "⚠ Not saved" } else if self.st.dirty { "Saving…" } else { "✓ Saved" })).size(11.5).weak());
                        }
                        if !phone_toast {
                            if let Some((v, url, notes)) = self.st.jobs.update.clone() {
                                let tip = if notes.is_empty() { tr("Download it").to_string() } else { format!("{notes}\n\n{}", tr("Click to download it. Right-click: not this version.")) };
                                let b = ui.add(egui::Button::new(RichText::new(tf!("⬆ Leaf Notes {v} is available", v = v.trim_start_matches('v'))).size(11.5).color(accent(ui))).frame_when_inactive(false)).tip(tip);
                                if b.clicked() { ui.ctx().open_url(egui::OpenUrl::new_tab(url)); }
                                b.context_menu(|ui| {
                                    if ui.button(tr("Don't tell me about this version again")).clicked() {
                                        self.st.p.skip_version = v.clone();
                                        self.st.jobs.update = None;
                                        self.st.dirty = true;
                                        ui.close();
                                    }
                                });
                            }
                        }
                        if !phone_toast && self.st.persist && self.st.sync_st.settings.on() {
                            let err = !self.st.sync_st.err.is_empty();
                            let retry = (self.st.sync_st.at + 40.0 - now_f()).max(0.0);
                            let (t, tip) = if self.st.sync_st.rx.is_some() {
                                (tr("🔄 Syncing…").to_string(), tr("Syncing with your other devices").to_string())
                            } else if err {
                                (tr("⚠ Not synced").to_string(), tf!("{sync_err}\nYour notes are safe on this device. Trying again in about {s} s. Click for sync settings.", sync_err = self.st.sync_st.err, s = format!("{retry:.0}")))
                            } else if self.st.sync_st.pending && self.st.sync_st.ok > 0 {
                                (tr("🔄 Changes waiting").to_string(), tf!("Synced {x}. The latest changes go out shortly; click to sync now.", x = ago(self.st.sync_st.ok)))
                            } else if self.st.sync_st.ok > 0 {
                                (tf!("🔄 Synced {x}", x = ago(self.st.sync_st.ok)), tr("Click to sync now").to_string())
                            } else {
                                (tr("🔄 Sync").to_string(), tr("Click to sync now").to_string())
                            };
                            let col = if err { danger_color(ui) } else { ui.visuals().weak_text_color() };
                            if ui.add(egui::Button::new(RichText::new(t).size(11.5).color(col)).frame_when_inactive(false)).tip(tip).clicked() {
                                if err { self.st.modals.open_settings(5); } else { self.st.sync_st.now = true; }
                            }
                        }
                        if !phone_toast {
                            self.focus_timer(ui);
                        }
                        if self.st.nav.undo_delete.is_some_and(|t| now_f() - t < 10.0) && !self.store.trash.is_empty() {
                            if ui.add(egui::Button::new(RichText::new(tr("↶ Undo delete")).size(11.5).color(accent(ui))).frame_when_inactive(false)).clicked() {
                                self.st.nav.undo_delete = None;
                                let i = self.store.trash.len() - 1;
                                self.restore(i);
                            }
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                        }
                        if !phone_toast && !is_deck && (self.st.canvas.zoom_now - 1.0).abs() > 0.02
                            && ui.add(egui::Button::new(RichText::new(format!("{:.0}%", self.st.canvas.zoom_now * 100.0)).size(11.5)).frame_when_inactive(false)).tip("Page zoom — click for actual size").clicked()
                        {
                            self.st.canvas.zoom_req = Some(ZoomReq::To(1.0));
                        }
                        if self.st.jobs.ai_busy() {
                            ui.label(RichText::new(tr("AI working…")).size(11.5).color(accent(ui)));
                            ui.spinner();
                        } else if let Some((_, _, cancel, _)) = &self.st.jobs.transcribe {
                            let cancel = cancel.clone();
                            if ui.small_button(tr("Cancel")).clicked() {
                                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                            let label = match self.st.jobs.transcribe_progress {
                                Some((done, total)) if total > 1 => tf!("Transcribing {done}/{total}…", done = done.min(total), total = total),
                                _ => tr("Making a transcript…").to_string(),
                            };
                            ui.label(RichText::new(label).size(11.5).color(accent(ui)));
                            ui.spinner();
                        } else if self.st.jobs.bg_busy() {
                            ui.spinner();
                        }
                        if !phone_toast && self.st.canvas.tool != Tool::Type {
                            let name = tr(match self.st.canvas.tool {
                                Tool::Pen => "✏ Pen",
                                Tool::Highlighter => "Highlighter",
                                Tool::Eraser => "Eraser",
                                Tool::Lasso => "Select ink",
                                Tool::Shape(_) => "Shape",
                                Tool::Type => "",
                            });
                            ui.menu_button(RichText::new(format!("{name} ▴")).size(11.5).weak(), |ui| {
                                for (t, label) in [
                                    (Tool::Type, "Aa  Type"), (Tool::Pen, "✏  Pen"), (Tool::Highlighter, "▮  Highlighter"),
                                    (Tool::Eraser, "⌫  Eraser"), (Tool::Lasso, "➰  Select ink"),
                                    (Tool::Shape(Shape::Line), "╱  Line"), (Tool::Shape(Shape::Arrow), "→  Arrow"),
                                    (Tool::Shape(Shape::Rect), "▭  Box"), (Tool::Shape(Shape::Ellipse), "◯  Circle"),
                                ] {
                                    if ui.selectable_label(self.st.canvas.tool == t, tr(label)).clicked() { self.st.canvas.tool = t; ui.close(); }
                                }
                            });
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            if !msg || ui.available_width() > 520.0 {
                                ui.add(egui::Label::new(RichText::new(info).size(11.5).weak()).truncate());
                            }
                            if msg {
                                if ui.available_width() > 520.0 { ui.add_space(12.0); }
                                let col = if self.st.status.starts_with('⚠') { danger_color(ui) } else { accent(ui) };
                                ui.add(egui::Label::new(RichText::new(&self.st.status).size(11.5).color(col)).truncate()).tip(&self.st.status);
                            }
                        });
                    });
                });
            });
    }

    /// A study timer: 25 minutes of focus, then a 5-minute break.
    fn focus_timer(&mut self, ui: &mut egui::Ui) {
        let now = now_f();
        if let Some((end, brk)) = self.st.focus {
            if now >= end {
                if brk {
                    self.st.focus = None;
                    self.st.say("Break's over — start another focus round when you're ready.");
                } else {
                    self.st.focus = Some((now + 5.0 * 60.0, true));
                    self.st.say("Nice work! Take a 5-minute break.");
                }
                return;
            }
            let left = (end - now) as u64;
            let label = format!("{}  {:02}:{:02}  ■", tr(if brk { "☕ Break" } else { "⏱ Focus" }), left / 60, left % 60);
            if ui.add(egui::Button::new(RichText::new(label).size(11.5).color(accent(ui))).frame_when_inactive(false)).tip("Stop the timer").clicked() {
                self.st.focus = None;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        } else if ui.add(egui::Button::new(RichText::new(tr("⏱ 25 min")).size(11.5)).frame_when_inactive(false)).tip("25 minutes of focused study, then a 5-minute break").clicked() {
            self.st.focus = Some((now + 25.0 * 60.0, false));
            self.st.say("Focus time: 25 minutes. You've got this.");
        }
    }
}
