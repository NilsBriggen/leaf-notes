//! Audio playback bar under the page title and note-linked playback helpers.

use crate::*;
use eframe::egui::RichText;

pub(crate) fn mmss(s: f32) -> String {
    let s = s.max(0.0) as u32;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// Which recording on this page is "current": the one playing, else the newest.
fn current_recording(page: &Page, st: &St) -> Option<usize> {
    let playing = st.audio.player.as_ref().and_then(|pl| page.recordings.iter().position(|r| r.id == pl.id));
    playing.or(page.recordings.len().checked_sub(1))
}

/// Make sure the player holds recording `i` of this page.
pub(crate) fn ensure_player(page: &Page, st: &mut St, i: usize) -> bool {
    let rec = &page.recordings[i];
    if st.audio.player.as_ref().is_some_and(|p| p.id == rec.id) {
        return true;
    }
    match crate::audio::Player::open(&files_path(&rec.file), rec.id) {
        Ok(p) => {
            p.set_speed(st.audio.speed);
            st.audio.player = Some(p);
            let title = if page.title.trim().is_empty() { tr("Untitled page").to_string() } else { page.title.clone() };
            st.audio.player_page = Some((page.id, title));
            true
        }
        Err(e) => {
            st.say(tf!("⚠ Can't play the recording: {e}", e = e));
            false
        }
    }
}

/// Play from a few seconds before the moment `t` (Unix seconds) in whichever recording covers it.
pub(crate) fn jump_to(page: &Page, st: &mut St, t: f64) {
    let Some(i) = page.recordings.iter().position(|r| t >= r.start - 1.0 && t <= r.start + r.secs as f64 + 2.0) else { return };
    if ensure_player(page, st, i) {
        let p = st.audio.player.as_ref().unwrap();
        p.seek((t - page.recordings[i].start) as f32 - 2.0);
        p.set_playing(true);
    }
}

/// During playback: a test for "written later than the current moment" (those are shown faded).
pub(crate) fn playback_moment(page: &Page, st: &mut St) -> Option<impl Fn(f64) -> bool> {
    let span = page.recordings.iter().fold(None, |acc: Option<(f64, f64)>, r| {
        let z = r.start + r.secs as f64;
        Some(acc.map_or((r.start, z), |(a, b)| (a.min(r.start), b.max(z))))
    });
    st.audio.recordings_span = span;
    st.audio.fade_after = None;
    if let Some(t) = st.audio.jump.take() {
        jump_to(page, st, t);
    }
    let pl = st.audio.player.as_ref()?;
    let rec = page.recordings.iter().find(|r| r.id == pl.id)?;
    if !pl.playing() && pl.time() < 0.05 {
        st.audio.fade_after = None;
        return None;
    }
    let now = rec.start + pl.time() as f64;
    let (a, z) = (rec.start, rec.start + rec.secs as f64);
    st.audio.fade_after = Some((now, a, z));
    Some(move |t: f64| t > now && t >= a && t <= z)
}

/// Under the title: recording controls, or playback with a scrubber.
pub(crate) fn audio_bar(ui: &mut egui::Ui, page: &mut Page, st: &mut St) {
    let Some(mut i) = current_recording(page, st) else { return };
    ui.add_space(4.0);
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            let narrow = ui.available_width() < 560.0;
            let phone = ui.available_width() < 420.0;
            let label = |k: usize, r: &Recording| tf!("Recording {n} · {len}", n = k + 1, len = mmss(r.secs));
            let playing = st
                .audio
                .player
                .as_ref()
                .is_some_and(|p| p.id == page.recordings[i].id && p.playing() && p.time() < page.recordings[i].secs - 0.1);

            ui.horizontal(|ui| {
                if phone {
                    ui.spacing_mut().button_padding.x = 6.0;
                    ui.spacing_mut().item_spacing.x = 4.0;
                }
                if ui
                    .add(egui::Button::new(RichText::new(if playing { "⏸" } else { "▶" }).size(16.0)).min_size(Vec2::new(32.0, 26.0)).frame_when_inactive(false))
                    .tip(if playing { "Pause" } else { "Play" })
                    .clicked()
                    && ensure_player(page, st, i)
                {
                    st.audio.player.as_ref().unwrap().set_playing(!playing);
                }
                if page.recordings.len() > 1 && !narrow {
                    egui::ComboBox::from_id_salt(("recs", page.id))
                        .selected_text(label(i, &page.recordings[i]))
                        .show_ui(ui, |ui| {
                            for (k, r) in page.recordings.iter().enumerate() {
                                if ui.selectable_label(k == i, label(k, r)).clicked() {
                                    i = k;
                                    st.audio.player = None;
                                    st.audio.player_page = None;
                                    let _ = ensure_player(page, st, k);
                                }
                            }
                        });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if phone {
                        ui.spacing_mut().button_padding.x = 6.0;
                        ui.spacing_mut().item_spacing.x = 4.0;
                    }
                    let more = ui.add(egui::Button::new(RichText::new("⋯").size(17.0)).frame_when_inactive(false)).tip("More for this recording");
                    egui::Popup::menu(&more).show(|ui| {
                        if narrow && page.recordings.len() > 1 {
                            for (k, r) in page.recordings.iter().enumerate() {
                                if ui.selectable_label(k == i, label(k, r)).clicked() {
                                    i = k;
                                    st.audio.player = None;
                                    st.audio.player_page = None;
                                    let _ = ensure_player(page, st, k);
                                    ui.close();
                                }
                            }
                            ui.separator();
                        }
                        if phone {
                            for x in [0.75f32, 1.0, 1.25, 1.5, 2.0] {
                                if ui.selectable_label(st.audio.speed == x, format!("{x}×")).clicked() {
                                    st.audio.speed = x;
                                    st.p.speed = x;
                                    st.dirty = true;
                                    if let Some(p) = &st.audio.player {
                                        p.set_speed(x);
                                    }
                                    ui.close();
                                }
                            }
                            ui.separator();
                        }
                        let can = st.ai.provider == crate::ai::Provider::OpenAiCompatible;
                        let tip = if can {
                            "Turn this recording into text with your AI key; each paragraph is linked to its moment"
                        } else {
                            "Needs an OpenAI-compatible service (OpenAI, Groq or a local Whisper server) in Settings ▸ AI"
                        };
                        if ui.add_enabled(can, egui::Button::new(tr("✍  Turn into text"))).tip(tip).clicked() {
                            st.modals.push(ActiveModal::TranscribeAsk { rec_id: page.recordings[i].id });
                            ui.close();
                        }
                        if ui
                            .button(RichText::new(tr("🗑  Remove recording")).color(crate::chrome::danger_color(ui)))
                            .tip("Remove this recording from the page (Undo brings it back)")
                            .clicked()
                        {
                            st.snap(page);
                            let id = page.recordings[i].id;
                            page.recordings.retain(|r| r.id != id);
                            if st.audio.player.as_ref().is_some_and(|p| p.id == id) {
                                st.audio.player = None;
                                st.audio.player_page = None;
                            }
                            ui.close();
                        }
                    });
                    if page.recordings.len() <= i {
                        return;
                    }
                    if !phone {
                        let sp = ui.add(egui::Button::new(RichText::new(format!("{}× ▾", st.audio.speed)).size(12.0)).frame_when_inactive(false)).tip("Playback speed");
                        egui::Popup::menu(&sp).show(|ui| {
                            for x in [0.75f32, 1.0, 1.25, 1.5, 2.0] {
                                if ui.selectable_label(st.audio.speed == x, format!("{x}×")).clicked() {
                                    st.audio.speed = x;
                                    st.p.speed = x;
                                    st.dirty = true;
                                    if let Some(p) = &st.audio.player {
                                        p.set_speed(x);
                                    }
                                    ui.close();
                                }
                            }
                        });
                    }
                    let tip = "On: clicking a note or ink plays what was said while you wrote it";
                    let jump = if narrow { "👆".to_string() } else { tr("👆 Tap notes to jump").to_string() };
                    if ui.add(egui::Button::new(RichText::new(jump).size(12.0)).selected(st.audio.listen).corner_radius(10)).tip(tip).clicked() {
                        st.audio.listen ^= true;
                    }
                    // Inside right_to_left layout: add 10⟳ (ahead) BEFORE ⟲10 (back) so visually it reads [⟲10] [10⟳].
                    if !phone {
                        if ui.add(egui::Button::new(RichText::new("10⟳").size(12.0)).frame_when_inactive(false)).tip("Ahead 10 seconds (→)").clicked() && ensure_player(page, st, i) {
                            let p = st.audio.player.as_ref().unwrap();
                            p.seek(p.time() + 10.0);
                        }
                        if ui.add(egui::Button::new(RichText::new("⟲10").size(12.0)).frame_when_inactive(false)).tip("Back 10 seconds (←)").clicked() && ensure_player(page, st, i) {
                            let p = st.audio.player.as_ref().unwrap();
                            p.seek(p.time() - 10.0);
                        }
                    }
                    let rec = &page.recordings[i];
                    let t = st.audio.player.as_ref().filter(|p| p.id == rec.id).map_or(0.0, |p| p.time());
                    let time = if narrow { mmss(t) } else { format!("{} / {}", mmss(t), mmss(rec.secs)) };
                    ui.label(RichText::new(time).size(12.0).monospace().weak());
                    ui.spacing_mut().slider_width = (ui.available_width() - 8.0).clamp(40.0, 520.0);
                    let (mut t, secs) = (t, rec.secs);
                    if ui.add(egui::Slider::new(&mut t, 0.0..=secs.max(0.1)).show_value(false)).changed() && ensure_player(page, st, i) {
                        st.audio.player.as_ref().unwrap().seek(t);
                    }
                });
            });
        });
}
