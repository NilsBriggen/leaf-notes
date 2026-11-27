//! Right-hand AI assistant side pane.

use crate::state::AiJobKind;
use crate::*;
use eframe::egui::{Align, Layout, RichText};

impl NotesApp {
    pub(crate) fn ai_pane(&mut self, ui: &mut egui::Ui) {
        let full = ui.ctx().content_rect().width();
        let panel = egui::Panel::right("ai").show_separator_line(false);
        let panel = if full < crate::DRAWER_COVER_W {
            panel.exact_size((full - 28.0).max(220.0)).resizable(false)
        } else {
            panel.default_size(284.0).size_range(240.0..=520.0)
        };
        panel
            .frame(crate::card(ui).inner_margin(14).outer_margin(egui::Margin { left: 6, right: 14, top: 0, bottom: 12 }))
            .show(ui, |ui| {
                let acc = accent(ui);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("✦").size(17.0).color(acc));
                    ui.label(RichText::new(tr("Assistant")).size(16.0).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new("✕").frame_when_inactive(false)).tip("Close").clicked() {
                            self.st.p.show_ai = false;
                            self.st.dirty = true;
                        }
                        if ui.add(egui::Button::new("⚙").frame_when_inactive(false)).tip("AI settings").clicked() {
                            self.st.modals.open_settings(3);
                        }
                        if !self.st.jobs.ai_log.is_empty() && ui.add(egui::Button::new("🗑").frame_when_inactive(false)).tip("Clear the conversation").clicked() {
                            self.st.jobs.ai_log.clear();
                        }
                    });
                });
                let short_pane = ui.available_height() < 340.0;
                let mut open_ai_settings = false;
                let mut chip_prompt: Option<String> = None;
                let render_top_extras = |ui: &mut egui::Ui, ai: &crate::ai::AiSettings, prompts: &str, open_settings: &mut bool, picked: &mut Option<String>| {
                    if ai.ready() {
                        ui.label(RichText::new(tf!("{model} · your own key", model = ai.model)).size(11.0).weak());
                    } else {
                        egui::Frame::new().fill(ui.visuals().selection.bg_fill).corner_radius(8).inner_margin(10).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(tr("Bring your own AI key")).strong());
                            ui.label(RichText::new(tr("Works with Anthropic, OpenAI, OpenRouter or a local model. Your key stays on this device.")).size(12.0));
                            if ui.button(tr("Add a key…")).clicked() { *open_settings = true; }
                        });
                    }
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                        for (label, prompt) in [
                            ("Summarize", "Summarize this page in a few bullet points."),
                            ("Action items", "List the action items from this page as a checklist."),
                            ("Quiz me", "Write 3 short quiz questions about this page. Put the answers at the end."),
                            ("Explain", "Explain the main idea of this page simply, with one example."),
                        ] {
                            if ui.add(egui::Button::new(RichText::new(tr(label)).size(12.0)).corner_radius(10)).clicked() {
                                *picked = Some(tr(prompt).into());
                            }
                        }
                        for p in prompts.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from) {
                            let label: String = p.chars().take(24).collect();
                            if ui.add(egui::Button::new(RichText::new(label).size(12.0)).corner_radius(10)).tip(&p).clicked() {
                                *picked = Some(p);
                            }
                        }
                    });
                    ui.add_space(4.0);
                };
                if !short_pane {
                    let prompts = self.st.p.prompts.clone();
                    render_top_extras(ui, &self.st.ai, &prompts, &mut open_ai_settings, &mut chip_prompt);
                }
                let mut insert = None;
                let colors = rich::Colors::from(ui, acc);
                // Keep the prompt input and Send ➤ footer permanently visible even on short landscape viewports (< 420 px).
                let prompt_rows = if short_pane { 1 } else { 2 };
                let footer_h = if self.st.nav.touch_ui {
                    if short_pane { 82.0 } else { 114.0 }
                } else if short_pane {
                    76.0
                } else {
                    104.0
                };
                let h = (ui.available_height() - footer_h).max(40.0);
                let streaming_idx = self.st.jobs.ai_jobs.iter().find_map(|j| matches!(j.kind, AiJobKind::Chat).then_some(j.chat_idx).flatten());
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                ui.spacing_mut().scroll.bar_width = 4.0;
                egui::ScrollArea::vertical().id_salt("ai-scroll").max_height(h).stick_to_bottom(true).auto_shrink([false; 2]).show(ui, |ui| {
                    if short_pane {
                        let prompts = self.st.p.prompts.clone();
                        render_top_extras(ui, &self.st.ai, &prompts, &mut open_ai_settings, &mut chip_prompt);
                    }
                    if self.st.jobs.ai_log.is_empty() {
                        if !short_pane { ui.add_space(24.0); }
                        ui.vertical_centered(|ui| ui.label(RichText::new(tr("Ask anything about this page.\nAnswers can be inserted as notes,\nincluding equations and checklists.")).weak()));
                    }
                    let w = ui.available_width();
                    for (i, (from_ai, text)) in self.st.jobs.ai_log.iter().enumerate() {
                        if text.starts_with("———") {
                            ui.vertical_centered(|ui| ui.label(RichText::new(text).size(11.0).weak()));
                        } else if *from_ai {
                            egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(10).inner_margin(10).show(ui, |ui| {
                                ui.set_width((w - 20.0).max(80.0));
                                let _ = rich::view(ui, text, 13.5, 0, &colors);
                                if !text.starts_with('⚠') && streaming_idx != Some(i) && !text.is_empty() {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.menu_button(RichText::new(tr("↧ Insert")).small(), |ui| {
                                            if ui.button(tr("At the end of the page")).clicked() { insert = Some(i); ui.close(); }
                                            if self.st.canvas.last_text.is_some() && ui.button(tr("Below the note I was writing in")).clicked() {
                                                insert = Some(i);
                                                self.st.jobs.insert_near = true;
                                                ui.close();
                                            }
                                        });
                                        if ui.small_button(tr("⧉ Copy")).clicked() { ui.ctx().copy_text(text.clone()); }
                                    });
                                }
                            });
                        } else {
                            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                egui::Frame::new().fill(ui.visuals().selection.bg_fill).corner_radius(10).inner_margin(egui::Margin::symmetric(10, 7)).show(ui, |ui| {
                                    ui.set_max_width((w * 0.85).max(80.0));
                                    ui.label(RichText::new(text).size(13.5));
                                });
                            });
                        }
                        ui.add_space(6.0);
                    }
                    if self.st.jobs.ai_busy() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(tr("Thinking…")).weak());
                            if ui.small_button(tr("Stop")).clicked() {
                                self.st.jobs.cancel_ai();
                                self.st.say("Stopped.");
                            }
                        });
                    }
                });
                if open_ai_settings { self.st.modals.open_settings(3); }
                if let Some(p) = chip_prompt { self.chat(p); }
                if let Some(i) = insert {
                    let t = self.st.jobs.ai_log[i].1.clone();
                    self.insert_markdown(&t);
                }
                ui.add_space(4.0);
                // Consume plain Enter before TextEdit so pressing Enter on an empty box doesn't insert a stray newline.
                let input_id = egui::Id::unique("ai-input");
                let focused = ui.memory(|m| m.has_focus(input_id));
                let enter = focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                ui.add(egui::TextEdit::multiline(&mut self.st.jobs.ai_prompt).id(input_id).hint_text(tr("Ask about this page…")).desired_rows(prompt_rows).desired_width(f32::INFINITY));
                ui.horizontal_wrapped(|ui| {
                    let send = ui.add(egui::Button::new(RichText::new(tr("Send ➤")).color(Color32::WHITE)).fill(accent_base())).clicked();
                    if !self.st.nav.touch_ui {
                        ui.label(RichText::new(tr("Shift+Enter for a new line")).size(10.5).weak());
                    }
                    if (send || enter) && !self.st.jobs.ai_prompt.trim().is_empty() {
                        let p = std::mem::take(&mut self.st.jobs.ai_prompt);
                        self.chat(p.trim().to_string());
                    }
                });
            });
    }
}
