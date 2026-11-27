//! Top title bar and ribbon tab switcher pill.

use super::{focus_ring, want_focus};
use crate::state::ActiveModal;
use crate::*;
use eframe::egui::{Align, Align2, FontId, Layout, Rect, RichText, Sense};

impl NotesApp {
    pub(crate) fn title_bar(&mut self, ui: &mut egui::Ui) {
        let desk = ui.visuals().panel_fill;
        let my = if self.st.nav.touch_ui { 5 } else { 10 };
        egui::Panel::top("title")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(desk).inner_margin(egui::Margin { left: 14, right: 14, top: my, bottom: my }))
            .show(ui, |ui| {
                let full = ui.available_width();
                let tw = |t: &str, size: f32| ui.painter().layout_no_wrap(t.to_string(), FontId::proportional(size), Color32::WHITE).size().x;
                let tight = self.st.nav.short;
                let tabs_w = self.tabs_width(ui, if tight { 16.0 } else { 30.0 });
                let pad = ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x;
                let recording = self.st.audio.recorder.is_some();
                let flag_w = if recording { 34.0 + ui.spacing().item_spacing.x } else { 0.0 };
                let need = |compact: bool, search_icon: bool| {
                    let brand = if tight { 0.0 } else { 34.0 } + 30.0 + if compact { 0.0 } else { tw(tr("Leaf Notes"), 19.0) } + 3.0 * ui.spacing().item_spacing.x;
                    let rec = tw("●  00:00  ■", 14.0).max(if compact { 0.0 } else { tw(tr("●  Record"), 14.0) }) + pad + flag_w;
                    let ai = if compact { tw("✦", 14.0) } else { tw(tr("✦  Assistant"), 14.0) } + pad;
                    brand + 18.0 + tabs_w + rec + ai + 4.0 * 34.0 + if search_icon { 34.0 } else { 150.0 } + 6.0 * 6.0 + 12.0
                };
                let search_icon = full < 480.0 || (need(true, false) > full && need(true, true) <= full);
                self.st.nav.search_icon_mode = search_icon;
                let two_rows = need(true, search_icon) > full;
                let compact = if two_rows { full < 845.0 } else { need(false, search_icon) > full };
                let rec_w = tw(if recording { "●  00:00  ■" } else if compact { tr("● Rec") } else { tr("●  Record") }, 14.0) + pad + flag_w;
                // Only show the serif brand title when it comfortably fits without crowding search or jumping at 560 px.
                let show_title = !tight && (if two_rows { full >= 640.0 } else { !compact });
                ui.horizontal(|ui| {
                    ui.set_min_height(34.0);
                    if full < 480.0 {
                        ui.spacing_mut().button_padding.x = 6.0;
                        ui.spacing_mut().item_spacing.x = 4.0;
                    }
                    let v = ui.visuals().clone();
                    let acc = accent(ui);
                    if ui.add(egui::Button::new(RichText::new("☰").size(17.0).color(v.weak_text_color())).frame_when_inactive(false)).tip("Show or hide notebooks").clicked() {
                        self.st.p.show_nav ^= true;
                        if self.st.p.show_nav && full < 760.0 { self.st.p.show_ai = false; }
                        self.st.dirty = true;
                    }
                    if !tight {
                        let (tile, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
                        let id = egui::Id::unique("logo");
                        let logo = ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
                            let img = image::load_from_memory(include_bytes!("../../packaging/leafnotes.png")).map(|i| i.to_rgba8()).unwrap_or_default();
                            let t = ui.ctx().load_texture("logo", egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), egui::TextureOptions::LINEAR);
                            ui.ctx().data_mut(|d| d.insert_temp(id, t.clone()));
                            t
                        });
                        ui.painter().image(logo.id(), tile, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                    }
                    if show_title {
                        ui.label(RichText::new(tr("Leaf Notes")).font(rich::serif(19.0)).color(v.strong_text_color()));
                    }
                    if !two_rows {
                        ui.add_space(18.0);
                        self.tabs(ui);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = if full < 480.0 { 2.0 } else { 6.0 };
                        if ui.add(egui::Button::new(RichText::new("⚙").size(16.0).color(v.weak_text_color())).frame_when_inactive(false)).tip("Settings").clicked() {
                            self.st.modals.toggle_settings(0);
                        }
                        if ui.add(egui::Button::new(RichText::new("?").size(16.0).color(v.weak_text_color())).frame_when_inactive(false)).tip("Help").clicked() {
                            self.st.modals.toggle(ActiveModal::Help);
                        }
                        let ai_on = self.st.p.show_ai;
                        let b = egui::Button::new(RichText::new(tr(if compact { "✦" } else { "✦  Assistant" })).color(if ai_on { Color32::WHITE } else { acc }))
                            .fill(if ai_on { accent_base() } else { v.extreme_bg_color })
                            .stroke(EStroke::new(1.0, if ai_on { accent_base() } else { v.widgets.noninteractive.bg_stroke.color }))
                            .corner_radius(17)
                            .min_size(Vec2::new(0.0, 34.0));
                        if ui.add(b).tip("Show or hide the AI assistant").clicked() {
                            self.st.p.show_ai ^= true;
                            if self.st.p.show_ai && full < 760.0 { self.st.p.show_nav = false; }
                            self.st.dirty = true;
                        }
                        if search_icon {
                            let open_full_search = |me: &mut Self| {
                                me.st.p.show_nav = true;
                                if full < 760.0 { me.st.p.show_ai = false; }
                                me.st.nav.tasks = false;
                                me.st.nav.search_open = true;
                                me.st.nav.focus_id = Some(egui::Id::unique("search-panel-input"));
                                me.st.dirty = true;
                            };
                            if self.st.nav.focus_id == Some(egui::Id::unique("search")) {
                                self.st.nav.focus_id = None;
                                open_full_search(self);
                            }
                            if ui.add(egui::Button::new(RichText::new("🔍").size(15.0).color(if self.st.nav.search_open { acc } else { v.weak_text_color() })).frame_when_inactive(false)).tip("Search all notes (Ctrl+F)").clicked() {
                                open_full_search(self);
                            }
                        } else {
                            let w = (ui.available_width() - rec_w - 2.0 * 34.0 - 30.0).clamp(90.0, 300.0);
                            let (pill, pill_resp) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::click());
                            let id = egui::Id::unique("search");
                            let focused = ui.memory(|m| m.has_focus(id));
                            ui.painter().rect_filled(pill, 17.0, v.extreme_bg_color);
                            ui.painter().rect_stroke(pill, 17.0, if focused { EStroke::new(1.5, acc) } else { v.widgets.noninteractive.bg_stroke }, egui::StrokeKind::Inside);
                            ui.painter().text(Pos2::new(pill.left() + 16.0, pill.center().y), Align2::CENTER_CENTER, "🔍", FontId::proportional(13.0), v.weak_text_color());
                            let key_w = if self.st.nav.search.is_empty() && !focused && w > 170.0 && !self.st.nav.touch_ui {
                                let g = ui.painter().layout_no_wrap(tr("Ctrl F").into(), FontId::proportional(11.0), v.weak_text_color());
                                let key = Rect::from_center_size(Pos2::new(pill.right() - 14.0 - g.size().x / 2.0 - 5.0, pill.center().y), g.size() + Vec2::new(10.0, 4.0));
                                ui.painter().rect_filled(key, 5.0, v.widgets.inactive.weak_bg_fill);
                                ui.painter().galley(key.center() - g.size() / 2.0, g, v.weak_text_color());
                                key.width() + 10.0
                            } else {
                                0.0
                            };
                            // Symmetrically center TextEdit inside pill with matched interact_size so placeholder, typed text, and caret align cleanly.
                            let field = Rect::from_min_max(Pos2::new(pill.left() + 30.0, pill.top()), Pos2::new(pill.right() - 12.0 - key_w, pill.bottom()));
                            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field).layout(Layout::left_to_right(Align::Center)));
                            child.spacing_mut().interact_size.y = 20.0;
                            let r = child.add(
                                egui::TextEdit::singleline(&mut self.st.nav.search)
                                    .id(id)
                                    .hint_text(tr("Search everything"))
                                    .frame(egui::Frame::NONE)
                                    .align(Align2::LEFT_CENTER)
                                    .margin(egui::Margin::symmetric(6, 2))
                                    .desired_width(field.width())
                                    .font(FontId::proportional(14.0)),
                            );
                            if pill_resp.clicked() { r.request_focus(); }
                            want_focus(&mut self.st, &r);
                        }
                        if ui.add_enabled(!self.st.redo.is_empty(), egui::Button::new(RichText::new("↷").size(if self.st.nav.touch_ui { 20.0 } else { 16.0 })).frame_when_inactive(false)).tip("Redo (Ctrl+Y)").clicked() {
                            self.undo(true);
                        }
                        if ui.add_enabled(!self.st.undo.is_empty(), egui::Button::new(RichText::new("↶").size(if self.st.nav.touch_ui { 20.0 } else { 16.0 })).frame_when_inactive(false)).tip("Undo (Ctrl+Z)").clicked() {
                            self.undo(false);
                        }
                        ui.add_space(6.0);
                        let red = Color32::from_rgb(220, 50, 47);
                        let rec_btn = if let Some((_, _, _, start)) = &self.st.audio.recorder {
                            let secs = (now_f() - start) as u64;
                            let pulse = (ui.input(|i| i.time) * 2.0).sin() > 0.0;
                            egui::Button::new(RichText::new(format!("{}  {:02}:{:02}  ■", if pulse { "●" } else { "○" }, secs / 60, secs % 60)).color(Color32::WHITE))
                                .fill(red)
                                .corner_radius(17)
                                .min_size(Vec2::new(0.0, 34.0))
                        } else {
                            let label = tr(if compact { "● Rec" } else { "●  Record" });
                            egui::Button::new(RichText::new(label).size(if compact { 12.5 } else { 14.0 }).color(red))
                                .fill(if compact { Color32::from_rgba_unmultiplied(220, 50, 47, if v.dark_mode { 36 } else { 20 }) } else { Color32::TRANSPARENT })
                                .stroke(if compact { EStroke::new(1.0, Color32::from_rgba_unmultiplied(220, 50, 47, 85)) } else { EStroke::NONE })
                                .frame_when_inactive(compact)
                                .corner_radius(17)
                                .min_size(Vec2::new(0.0, 32.0))
                        };
                        if ui.add(rec_btn).tip("Record audio while you take notes; later, tap any note to hear what was said then").clicked() {
                            self.st.audio.rec_toggle = true;
                        }
                        if self.st.audio.recorder.is_some()
                            && ui.add(egui::Button::new(RichText::new("⚑").size(16.0)).frame_when_inactive(false)).tip("Mark this moment (a note linked to it)").clicked()
                        {
                            let at = self.st.audio.recorder.as_ref().map_or(0.0, |r| now_f() - r.3);
                            let y = self.next_free_y();
                            let mut b = block(24.0, y, 260.0, Kind::Text { text: tf!("⚑ Marked at {t}", t = format!("{:02}:{:02}", at as u64 / 60, at as u64 % 60)), heading: 0 });
                            b.tag = Tag::Important;
                            let snap = self.page().clone();
                            self.st.snap(&snap);
                            self.page().blocks.push(b);
                        }
                    });
                });
                if two_rows {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| self.tabs(ui));
                }
            });
    }

    fn tab_names(&self) -> [String; 6] {
        let due = self.st.cache.due_now;
        let study = if due > 0 { tf!("Study · {due}", due = due) } else { tr("Study").to_string() };
        [tr("Home").into(), tr("Insert").into(), tr("Draw").into(), tr("AI").into(), study, tr("View").into()]
    }

    /// The width the tab pill takes with `pad` around each name.
    fn tabs_width(&self, ui: &egui::Ui, pad: f32) -> f32 {
        self.tab_names().iter().map(|n| ui.painter().layout_no_wrap(n.clone(), FontId::proportional(14.0), Color32::WHITE).size().x + pad).sum::<f32>() + 8.0
    }

    /// The ribbon tabs as a segmented pill; shrinks padding first and adds visible overflow cue on narrow phones.
    fn tabs(&mut self, ui: &mut egui::Ui) {
        let show_expand = !self.st.p.ribbon_open;
        let expand_w = if show_expand { 30.0 } else { 0.0 };
        let avail = (ui.available_width() - expand_w).max(80.0);
        let mut pad = if avail < 440.0 || self.st.nav.short { 16.0 } else { 30.0 };
        if self.tabs_width(ui, pad) > avail + 1.0 {
            pad = 10.0;
        }
        let need_w = self.tabs_width(ui, pad);
        if need_w > avail + 1.0 {
            let nudge_id = egui::Id::unique("tabs-nudge");
            let pending: Option<f32> = ui.data_mut(|d| d.remove_temp(nudge_id));
            let mut sa = egui::ScrollArea::horizontal()
                .id_salt("tabs-scroll")
                .max_width(avail)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .auto_shrink([false, true]);
            if let Some(off) = pending { sa = sa.horizontal_scroll_offset(off); }
            let out = sa.show(ui, |ui| self.tabs_inner(ui, pad));
            let max_x = (out.content_size.x - out.inner_rect.width()).max(0.0);
            if max_x > 2.0 {
                let off = out.state.clamped_offset().x;
                let bg = ui.visuals().panel_fill;
                let p = ui.painter();
                let step = (out.inner_rect.width() * 0.5).max(80.0);
                if off > 2.0 {
                    let r = Rect::from_center_size(Pos2::new(out.inner_rect.left() + 10.0, out.inner_rect.center().y), Vec2::new(20.0, 26.0));
                    let resp = ui.interact(r, ui.scope_id().with("tabs-l"), Sense::click());
                    p.rect(r, 8.0, bg, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
                    p.text(r.center(), Align2::CENTER_CENTER, "‹", FontId::proportional(15.0), ui.visuals().strong_text_color());
                    if resp.clicked() { ui.data_mut(|d| d.insert_temp(nudge_id, (off - step).max(0.0))); ui.ctx().request_repaint(); }
                }
                if off < max_x - 2.0 {
                    let r = Rect::from_center_size(Pos2::new(out.inner_rect.right() - 10.0, out.inner_rect.center().y), Vec2::new(20.0, 26.0));
                    let resp = ui.interact(r, ui.scope_id().with("tabs-r"), Sense::click());
                    p.rect(r, 8.0, bg, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
                    p.text(r.center(), Align2::CENTER_CENTER, "›", FontId::proportional(15.0), ui.visuals().strong_text_color());
                    if resp.clicked() { ui.data_mut(|d| d.insert_temp(nudge_id, (off + step).min(max_x))); ui.ctx().request_repaint(); }
                }
            }
        } else {
            self.tabs_inner(ui, pad);
        }
        if show_expand
            && ui.add(egui::Button::new(RichText::new("⌄").size(16.0).color(ui.visuals().weak_text_color())).frame_when_inactive(false)).tip("Show the toolbar").clicked()
        {
            self.st.p.ribbon_open = true;
            self.st.dirty = true;
        }
    }

    fn tabs_inner(&mut self, ui: &mut egui::Ui, pad: f32) {
        let v = ui.visuals().clone();
        let acc = accent(ui);
        let tab_names = self.tab_names();
        let names = [Tab::Home, Tab::Insert, Tab::Draw, Tab::Ai, Tab::Study, Tab::View].into_iter().zip(tab_names.iter()).collect::<Vec<_>>();
        let font = FontId::proportional(14.0);
        let widths: Vec<f32> = tab_names.iter().map(|n| ui.painter().layout_no_wrap(n.to_string(), font.clone(), Color32::WHITE).size().x + pad).collect();
        let (track, _) = ui.allocate_exact_size(Vec2::new(widths.iter().sum::<f32>() + 8.0, 34.0), Sense::hover());
        ui.painter().rect_filled(track, 17.0, if v.dark_mode { Color32::from_rgb(30, 36, 37) } else { Color32::from_rgb(222, 229, 226) });
        let mut x = track.left() + 4.0;
        for ((t, name), w) in names.iter().zip(widths) {
            let r = Rect::from_min_size(Pos2::new(x, track.top() + 4.0), Vec2::new(w, 26.0));
            x += w;
            let resp = ui.interact(r, ui.scope_id().with(("tab", name.as_str())), Sense::click());
            focus_ring(ui, &resp, r, 13.0);
            let (label, chosen) = (name.to_string(), self.st.tab == *t);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, chosen, &label));
            let sel = self.st.tab == *t;
            let open = self.st.p.ribbon_open;
            if sel && open {
                ui.painter().add(egui::Shape::rect_filled(r.translate(Vec2::new(0.0, 1.0)), 13.0, Color32::from_black_alpha(18)));
                ui.painter().rect_filled(r, 13.0, v.extreme_bg_color);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 13.0, v.widgets.hovered.weak_bg_fill);
            }
            let col = if sel && open { acc } else { v.text_color() };
            ui.painter().text(r.center(), Align2::CENTER_CENTER, name, font.clone(), col);
            let resp = if sel && open { resp.tip("Click again to fold the toolbar away") } else { resp };
            if resp.clicked() {
                if sel {
                    self.st.p.ribbon_open ^= true;
                } else {
                    self.st.tab = *t;
                    self.st.p.ribbon_open = true;
                    // Switching to Draw from Type automatically arms the pen so the user can write immediately.
                    if *t == Tab::Draw && self.st.canvas.tool == Tool::Type {
                        self.st.canvas.tool = Tool::Pen;
                    }
                }
                self.st.dirty = true;
            }
        }
    }
}
