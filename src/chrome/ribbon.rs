//! Ribbon toolbar with soft edge fades, clickable overflow chevrons, and all 6 tabs.

use super::{group, labelled_slider, pen_icon, rbtn, remember_color, sbtn, segmented, swatch, PEN_COLORS};
use crate::state::ActiveModal;
use crate::*;
use eframe::egui::{Align2, FontId, Rect, RichText, Sense};

impl NotesApp {
    pub(crate) fn ribbon(&mut self, ui: &mut egui::Ui) {
        if !self.st.p.ribbon_open || self.st.canvas.typing_kb { return; }
        let desk = ui.visuals().panel_fill;
        let touch = self.st.nav.touch_ui;
        egui::Panel::top("ribbon")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(desk).inner_margin(egui::Margin { left: 14, right: 14, top: 0, bottom: if touch { 6 } else { 12 } }))
            .show(ui, |ui| {
                card(ui)
                    .inner_margin(egui::Margin { left: 10, right: 10, top: if touch { 3 } else { 6 }, bottom: if touch { 1 } else { 4 } })
                    .show(ui, |ui| {
                        // Touch screens: slightly smaller rows and uniform button padding across every tab
                        // so toolbar height never jumps between tabs or tools.
                        if touch {
                            ui.spacing_mut().interact_size.y = 30.0;
                            ui.spacing_mut().button_padding.y = 5.0;
                        }
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                        ui.spacing_mut().scroll.bar_width = 4.0;
                        ui.horizontal_top(|ui| {
                            let (row, gap, bar) = (ui.spacing().interact_size.y, ui.spacing().item_spacing.y, ui.spacing().scroll);
                            let groups = (row * 2.0 + 6.0).max(52.0) + if row < 30.0 { 13.0 + gap } else { 0.0 };
                            ui.set_min_height(groups + bar.bar_width + bar.bar_inner_margin + bar.bar_outer_margin);
                            let fold = 16.0 + 2.0 * ui.spacing().button_padding.x + ui.spacing().item_spacing.x;
                            let tab_key = self.st.tab as u8;
                            let nudge_id = egui::Id::unique(("ribbon-nudge", tab_key));
                            let edge_id = egui::Id::unique(("ribbon-edges", tab_key));
                            let pending_offset: Option<f32> = ui.data_mut(|d| d.remove_temp(nudge_id));
                            let (had_left, had_right, last_off, last_max) = ui.data(|d| d.get_temp::<(bool, bool, f32, f32)>(edge_id)).unwrap_or((false, false, 0.0, 0.0));
                            let chevron_slot = 24.0;
                            let reserved = (if had_left { chevron_slot } else { 0.0 }) + (if had_right { chevron_slot } else { 0.0 });
                            let w = (ui.available_width() - fold - reserved).max(80.0);
                            let step = (w * 0.55).max(140.0);
                            let mut nudge_to: Option<f32> = None;
                            // Allocate dedicated scroll chevron buttons OUTSIDE the ScrollArea so they never obscure tools or swatches.
                            if had_left {
                                let (r, resp) = ui.allocate_exact_size(Vec2::new(22.0, groups), Sense::click());
                                let pill = Rect::from_center_size(r.center(), Vec2::new(22.0, 28.0));
                                let fill = if resp.hovered() { ui.visuals().widgets.hovered.weak_bg_fill } else { ui.visuals().faint_bg_color };
                                ui.painter().rect(pill, 8.0, fill, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
                                ui.painter().text(pill.center(), Align2::CENTER_CENTER, "‹", FontId::proportional(17.0), ui.visuals().strong_text_color());
                                if resp.clicked() {
                                    nudge_to = Some((last_off - step).max(0.0));
                                }
                            }
                            let mut sa = egui::ScrollArea::horizontal()
                                .id_salt(("ribbon-scroll", tab_key))
                                .max_width(w)
                                .auto_shrink([false, true]);
                            if let Some(off) = pending_offset {
                                sa = sa.horizontal_scroll_offset(off);
                            }
                            let out = sa.show(ui, |ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    match self.st.tab {
                                        Tab::Home => self.ribbon_home(ui),
                                        Tab::Insert => self.ribbon_insert(ui),
                                        Tab::Draw => self.ribbon_draw(ui),
                                        Tab::Ai => self.ribbon_ai(ui),
                                        Tab::Study => self.ribbon_study(ui),
                                        Tab::View => self.ribbon_view(ui),
                                    }
                                });
                            });
                            let max_x = (out.content_size.x - out.inner_rect.width()).max(0.0);
                            let off = out.state.clamped_offset().x;
                            let can_left = max_x > 4.0 && off > 2.0;
                            let can_right = max_x > 4.0 && off < max_x - 2.0;
                            if max_x > 4.0 {
                                let bg = ui.visuals().extreme_bg_color;
                                let painter = ui.painter();
                                if can_left { paint_edge_fade(painter, out.inner_rect, true, bg); }
                                if can_right { paint_edge_fade(painter, out.inner_rect, false, bg); }
                            }
                            if had_right {
                                let (r, resp) = ui.allocate_exact_size(Vec2::new(22.0, groups), Sense::click());
                                let pill = Rect::from_center_size(r.center(), Vec2::new(22.0, 28.0));
                                let fill = if resp.hovered() { ui.visuals().widgets.hovered.weak_bg_fill } else { ui.visuals().faint_bg_color };
                                ui.painter().rect(pill, 8.0, fill, ui.visuals().widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
                                ui.painter().text(pill.center(), Align2::CENTER_CENTER, "›", FontId::proportional(17.0), ui.visuals().strong_text_color());
                                if resp.clicked() {
                                    nudge_to = Some((off + step).min(max_x.max(last_max)));
                                }
                            }
                            if let Some(target) = nudge_to {
                                ui.data_mut(|d| d.insert_temp(nudge_id, target));
                                ui.ctx().request_repaint();
                            }
                            ui.data_mut(|d| d.insert_temp(edge_id, (can_left, can_right, off, max_x)));
                            if (can_left, can_right) != (had_left, had_right) {
                                ui.ctx().request_repaint();
                            }
                            if ui.add(egui::Button::new(RichText::new("⌃").size(16.0).color(ui.visuals().weak_text_color())).frame_when_inactive(false)).tip("Hide the toolbar (click a tab to show it again)").clicked() {
                                self.st.p.ribbon_open = false;
                                self.st.dirty = true;
                            }
                        });
                    });
            });
    }

    fn tool_buttons(&mut self, ui: &mut egui::Ui, tools: &[(Tool, &str, &str)]) {
        for (t, icon, label) in tools {
            if rbtn(ui, icon, label, self.st.canvas.tool == *t)
                .tip(match t {
                    Tool::Type => "Type and select (T)",
                    Tool::Pen => "Pen (P)",
                    Tool::Lasso => "Select ink (L)",
                    Tool::Eraser => "Eraser (E)",
                    _ => label,
                })
                .clicked()
            {
                self.st.canvas.tool = *t;
            }
        }
    }

    fn ribbon_home(&mut self, ui: &mut egui::Ui) {
        let lecture = |me: &mut Self, ui: &mut egui::Ui| {
            group(ui, "Lecture", true, |ui| {
                if rbtn(ui, "🎓", "New lecture", false).tip("A new page titled with today's date").clicked() { me.start_lecture(false); }
                if rbtn(ui, "●", "Record lecture", false).tip("New lecture page, recording from the start").clicked() { me.start_lecture(true); }
            });
        };
        if !self.st.nav.narrow { lecture(self, ui); }
        group(ui, "Tools", true, |ui| self.tool_buttons(ui, &[(Tool::Type, "Aa", "Type"), (Tool::Pen, "✏", "Pen"), (Tool::Lasso, "➰", "Select ink")]));
        if self.st.nav.narrow { lecture(self, ui); }
        let target = self.st.canvas.editing.or(self.st.canvas.last_text);
        let is_text = target.is_some_and(|id| self.page().blocks.iter().any(|b| b.id == id && matches!(b.kind, Kind::Text { .. })));
        let heading = target
            .and_then(|id| self.page().blocks.iter().find(|b| b.id == id))
            .and_then(|b| if let Kind::Text { heading, .. } = b.kind { Some(heading) } else { None })
            .unwrap_or(0);
        group(ui, "Text", true, |ui| {
            ui.add_enabled_ui(is_text, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(2.0, 3.0);
                    ui.horizontal(|ui| {
                        let hl = Color32::from_rgb(255, 236, 140);
                        for (t, tip, f) in [
                            (RichText::new("B").strong(), "Bold (Ctrl+B)", Fmt::Wrap("**")),
                            (RichText::new("I").italics(), "Italic (Ctrl+I)", Fmt::Wrap("*")),
                            (RichText::new("S").strikethrough(), "Strikethrough", Fmt::Wrap("~~")),
                            (RichText::new("H").background_color(hl).color(Color32::BLACK), "Highlight (Ctrl+Shift+H)", Fmt::Wrap("==")),
                            (RichText::new("</>").monospace(), "Code (Ctrl+`)", Fmt::Wrap("`")),
                            (RichText::new("$x$"), "Inline math (Ctrl+M)", Fmt::Wrap("$")),
                        ] {
                            if sbtn(ui, t, tip, false).clicked() { self.st.canvas.fmt = Some(f); }
                        }
                    });
                    ui.horizontal(|ui| {
                        for (t, tip, f) in [
                            ("•", "Bulleted list", Fmt::Prefix("- ")),
                            ("1.", "Numbered list", Fmt::Prefix("1. ")),
                            ("☐", "Checkbox list", Fmt::Prefix("[ ] ")),
                            ("❝", "Quote", Fmt::Prefix("> ")),
                        ] {
                            if sbtn(ui, RichText::new(t), tip, false).clicked() { self.st.canvas.fmt = Some(f); }
                        }
                        ui.add_space(4.0);
                        for (h, t, tip) in [(0u8, "¶", "Normal text"), (1, "H1", "Heading 1"), (2, "H2", "Heading 2")] {
                            if sbtn(ui, RichText::new(t), tip, is_text && heading == h).clicked() { self.st.canvas.fmt = Some(Fmt::Heading(h)); }
                        }
                    });
                });
            });
        });
        let tag = target.and_then(|id| self.page().blocks.iter().find(|b| b.id == id)).map(|b| b.tag);
        group(ui, "Tags", false, |ui| {
            ui.add_enabled_ui(tag.is_some(), |ui| {
                for (t, icon, label) in [(Tag::Todo, "☐", "To Do"), (Tag::Important, "★", "Important"), (Tag::Question, "?", "Question")] {
                    let on = tag == Some(t) || (t == Tag::Todo && tag == Some(Tag::Done));
                    if rbtn(ui, icon, label, on).clicked() {
                        let page = self.page();
                        let snap = page.clone();
                        if let Some(b) = page.blocks.iter_mut().find(|b| Some(b.id) == target) {
                            b.tag = if on { Tag::None } else { t };
                        }
                        self.st.snap(&snap);
                    }
                }
            });
            if rbtn(ui, "☑", "All tasks", self.st.nav.tasks).tip("Every open to-do from every notebook, in one list").clicked() {
                self.st.nav.tasks ^= true;
                self.st.p.show_nav = true;
            }
            if rbtn(ui, "🔎", "Find tags", self.st.nav.tag_filter.is_some()).tip("See every tagged note in all notebooks").clicked() {
                self.st.nav.tag_filter = if self.st.nav.tag_filter.is_some() { None } else { Some(Tag::None) };
                self.st.p.show_nav = true;
            }
        });
    }

    fn ribbon_insert(&mut self, ui: &mut egui::Ui) {
        group(ui, "Notes", true, |ui| {
            if rbtn(ui, "¶", "Text", false).clicked() { self.insert(Kind::Text { text: String::new(), heading: 0 }); }
            if rbtn(ui, "☑", "Checklist", false).clicked() { self.insert(Kind::Checklist { items: vec![(false, String::new())] }); }
            if rbtn(ui, "▦", "Table", false).clicked() { self.insert(Kind::Table { rows: vec![vec![String::new(); 3]; 3] }); }
            if rbtn(ui, "{ }", "Code", false).clicked() { self.insert(Kind::Code { lang: "text".into(), code: String::new() }); }
        });
        group(ui, "Math", true, |ui| {
            if rbtn(ui, "∑", "Equation", false).clicked() { self.insert(Kind::Math { src: String::new() }); }
            if rbtn(ui, "[⋮]", "Matrix", false).clicked() { self.insert(Kind::Math { src: r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}".into() }); }
            if rbtn(ui, "{≡", "Cases", false).clicked() { self.insert(Kind::Math { src: r"f(x) = \begin{cases} 1 & x \ge 0 \\ 0 & x < 0 \end{cases}".into() }); }
        });
        group(ui, "Media", true, |ui| {
            if rbtn(ui, "🖼", "Picture", false).clicked() { self.pick_file(Ins::Picture); }
            #[cfg(target_os = "android")]
            if rbtn(ui, "📷", "Photo", false).tip("Take a photo with the camera and put it on the page").clicked() {
                self.st.picking_since = now_f();
                if let Err(e) = crate::droid::take_photo() { self.st.say(tf!("⚠ {e}", e = e)); }
            }
            if rbtn(ui, "📄", "Printout", false).tip("Put a PDF's pages (or a picture, Markdown or text file) onto the page so you can write on it").clicked() { self.pick_file(Ins::Printout); }
            if rbtn(ui, "📎", "Attach file", false).tip("Attach any file as an icon you can open later").clicked() { self.pick_file(Ins::Attach); }
            if rbtn(ui, "🔗", "Link", false).clicked() { self.insert(Kind::Link { url: "https://".into(), title: String::new() }); }
        });
        group(ui, "Stamp", true, |ui| {
            if rbtn(ui, "🕓", "Date & time", false).clicked() { self.insert(Kind::Text { text: super::format_date(now_secs(), true), heading: 0 }); }
        });
        group(ui, "Page", false, |ui| {
            if rbtn(ui, "+", "Blank page", false).tip("New page (Ctrl+N)").clicked() { self.new_page(None); }
            let r = rbtn(ui, "▦", "Paper ▾", false).tip("A new page on the background you pick");
            egui::Popup::menu(&r).show(|ui| {
                for (p, icon, name) in PAPERS {
                    if ui.button(tf!("{icon}  {name}", icon = icon, name = tr(name))).clicked() { self.new_page(Some(p)); ui.close(); }
                }
                ui.separator();
                for (i, (name, ..)) in crate::TEMPLATES.iter().enumerate() {
                    if ui.button(format!("▦  {}", tr(name))).clicked() { self.new_page_template(i); ui.close(); }
                }
            });
        });
    }

    fn ribbon_draw(&mut self, ui: &mut egui::Ui) {
        group(ui, "Tools", true, |ui| {
            self.tool_buttons(ui, &[(Tool::Type, "Aa", "Type"), (Tool::Lasso, "➰", "Select ink")]);
            if rbtn(ui, "⌫", "Eraser", self.st.canvas.tool == Tool::Eraser).tip("Eraser (E)").clicked() {
                self.st.canvas.tool = Tool::Eraser;
            }
        });
        group(ui, "My pens", true, |ui| {
            let n = self.st.p.pens.len();
            let mut remove = None;
            for i in 0..n {
                let pen = self.st.p.pens[i];
                let sel = self.st.canvas.tool == Tool::Pen && self.st.p.pen == i;
                let r = pen_icon(ui, c4(pen.color), sel, Some(pen.kind))
                    .tip(tf!("{name} · size {width}\nChange it under Pen settings. Right-click to remove", name = tr(pen.kind.name()), width = format!("{:.1}", pen.width)));
                if r.clicked() {
                    self.st.canvas.tool = Tool::Pen;
                    self.st.p.pen = i;
                    self.st.dirty = true;
                }
                r.context_menu(|ui| {
                    if n > 1 && ui.button(tr("Remove this pen")).clicked() { remove = Some(i); ui.close(); }
                });
            }
            if let Some(i) = remove {
                self.st.p.pens.remove(i);
                if self.st.p.pen >= i && self.st.p.pen > 0 { self.st.p.pen -= 1; }
                self.st.dirty = true;
            }
            if ui.add(egui::Button::new(RichText::new("+").size(18.0)).min_size(Vec2::new(26.0, 50.0)).frame_when_inactive(false))
                .tip("Add a pen (starts as a copy of the chosen one)")
                .clicked()
            {
                let p = *self.st.pen();
                self.st.p.pens.push(p);
                self.st.p.pen = self.st.p.pens.len() - 1;
                self.st.canvas.tool = Tool::Pen;
                self.st.dirty = true;
            }
        });
        group(ui, "Highlighter", true, |ui| {
            for c in [[253, 224, 71, 120], [134, 239, 172, 120], [249, 168, 212, 120], [125, 211, 252, 120]] {
                let sel = self.st.canvas.tool == Tool::Highlighter && self.st.p.hl_color == c;
                if pen_icon(ui, c4([c[0], c[1], c[2], 255]), sel, None).tip("Highlighter (H): see-through colour over your notes").clicked() {
                    self.st.canvas.tool = Tool::Highlighter;
                    self.st.p.hl_color = c;
                    self.st.dirty = true;
                }
            }
        });
        let before = (self.st.p.erase_partial, self.st.p.eraser_size, self.st.p.hl_width, self.st.p.hl_color);
        if self.st.canvas.tool == Tool::Eraser {
            group(ui, "Eraser settings", true, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let start = ui.cursor().left();
                    let opts = [("Part of a stroke", "Erases only where you rub, like a real eraser"), ("Whole stroke", "Erases every stroke it touches, in one go")];
                    if let Some(i) = segmented(ui, &opts, usize::from(!self.st.p.erase_partial)) { self.st.p.erase_partial = i == 0; }
                    let w = ui.min_rect().right() - start;
                    labelled_slider(ui, "Size", w, &mut self.st.p.eraser_size, 3.0..=40.0, "How big the eraser is");
                });
                ui.add_space(4.0);
                let (r, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
                ui.painter().rect_filled(r, 8.0, ui.visuals().faint_bg_color);
                ui.painter().circle_stroke(r.center(), self.st.p.eraser_size.min(24.0), EStroke::new(1.2, ui.visuals().text_color()));
            });
        } else if self.st.canvas.tool == Tool::Highlighter {
            group(ui, "Highlighter settings", true, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr("Colour")).size(11.5).weak());
                        ui.spacing_mut().interact_size = Vec2::new(32.0, 24.0);
                        let mut c = self.st.p.hl_color;
                        if ui.color_edit_button_srgba_unmultiplied(&mut c).tip("Any colour for the highlighter").changed() {
                            self.st.p.hl_color = [c[0], c[1], c[2], c[3].min(160)];
                        }
                        for c in [[253, 224, 71, 120], [134, 239, 172, 120], [249, 168, 212, 120], [125, 211, 252, 120], [253, 186, 116, 120], [196, 181, 253, 120]] {
                            let cur = self.st.p.hl_color;
                            if swatch(ui, c, cur == c).clicked() { self.st.p.hl_color = c; }
                        }
                    });
                    labelled_slider(ui, "Size", 150.0, &mut self.st.p.hl_width, 6.0..=40.0, "How thick the highlighter is");
                });
                let (r, _) = ui.allocate_exact_size(Vec2::new(70.0, 52.0), Sense::hover());
                ui.painter().rect_filled(r, 8.0, ui.visuals().faint_bg_color);
                ui.painter().line_segment(
                    [r.left_center() + Vec2::new(10.0, 0.0), r.right_center() - Vec2::new(10.0, 0.0)],
                    EStroke::new(self.st.p.hl_width.min(40.0), c4(self.st.p.hl_color)),
                );
            });
        } else {
            group(ui, "Pen settings", true, |ui| {
                let before = *self.st.pen();
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.spacing_mut().button_padding.y = ui.spacing().button_padding.y.min(5.0);
                    ui.horizontal(|ui| {
                        let kind = self.st.pen().kind;
                        // Wider ComboBox so German "Kugelschreiber" and French "Stylo à bille" fit cleanly.
                        egui::ComboBox::from_id_salt("pen-kind").width(118.0).selected_text(tr(kind.name())).show_ui(ui, |ui| {
                            for k in PenKind::ALL {
                                if ui.selectable_label(kind == k, tr(k.name())).clicked() { self.st.pen().kind = k; }
                            }
                        }).response.tip("What the chosen pen writes like");
                        ui.spacing_mut().interact_size = Vec2::new(32.0, 24.0);
                        let pen = self.st.pen();
                        let mut c = pen.color;
                        if ui.color_edit_button_srgba_unmultiplied(&mut c).tip("Colour and see-through-ness").changed() {
                            pen.color = c;
                            remember_color(&mut self.st.p.recent_colors, c);
                        }
                        ui.spacing_mut().item_spacing.x = 3.0;
                        let recent = self.st.p.recent_colors.clone();
                        for c in PEN_COLORS.iter().copied().chain(recent) {
                            let cur = self.st.pen().color;
                            if swatch(ui, c, cur == c).clicked() { self.st.pen().color = c; }
                        }
                        if self.st.p.pens.len() > 1 && ui.add(egui::Button::new("🗑").frame_when_inactive(false)).tip("Remove this pen from My pens").clicked() {
                            let i = self.st.p.pen;
                            self.st.p.pens.remove(i);
                            self.st.p.pen = i.min(self.st.p.pens.len() - 1);
                            self.st.dirty = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        let tip = match self.st.canvas.force {
                            Some(f) => tf!("Pen thickness. Pen pressure now: {pct}%", pct = format!("{:.0}", f * 100.0)),
                            None => "Pen thickness. Pressure comes from your pen, or from speed with a mouse".into(),
                        };
                        let l = ui.label(RichText::new(tr("Size")).size(11.5).weak());
                        ui.spacing_mut().slider_width = 60.0;
                        ui.add(egui::Slider::new(&mut self.st.pen().width, 0.5..=16.0).show_value(false)).labelled_by(l.id).tip(tip);
                        ui.add_space(6.0);
                        let l = ui.label(RichText::new(tr("Smooth")).size(11.5).weak());
                        ui.spacing_mut().slider_width = 44.0;
                        ui.add(egui::Slider::new(&mut self.st.p.stabilizer, 0.0..=1.0).show_value(false)).labelled_by(l.id).tip("Smoothing: irons out shaky lines (see the sample). More smoothing makes the line trail the pen a little.");
                        if self.st.p.pressure && self.st.canvas.force.is_some() {
                            let l = ui.label(RichText::new(tr("Pressure")).size(11.5).weak());
                            ui.spacing_mut().slider_width = 60.0;
                            ui.add(egui::Slider::new(&mut self.st.p.pressure_curve, 0.4..=2.5).show_value(false)).labelled_by(l.id).tip("How hard you need to press: left makes light touches bolder, right needs a firmer hand");
                        }
                    });
                });
                let (stab, pen) = (self.st.p.stabilizer, *self.st.pen());
                let mut pts: Vec<[f32; 3]> = vec![];
                for i in 0..60 {
                    let t = i as f32 / 59.0;
                    let (x, y) = (t * 54.0, 18.0 - 9.0 * (t * 6.3).sin() + 1.8 * (i as f32 * 2.7).sin() + 1.2 * (i as f32 * 5.3).cos());
                    let pr = 0.35 + 0.65 * (t * 3.1).sin().abs();
                    let (x, y) = pts.last().map_or((x, y), |l| crate::canvas::trail(*l, x, y, stab));
                    pts.push([x, y, pr]);
                }
                crate::canvas::smooth_passes(&mut pts, stab);
                let dark = ui.visuals().dark_mode;
                let preview_col = if dark {
                    let [r, g, b, a] = pen.color;
                    if 0.2126 * (r as f32) + 0.7152 * (g as f32) + 0.0722 * (b as f32) < 60.0 {
                        [226, 230, 228, a]
                    } else {
                        pen.color
                    }
                } else {
                    pen.color
                };
                let sample = Stroke { id: 0, points: pts, color: preview_col, width: pen.width.min(6.0), highlighter: false, pen: pen.kind, t: 0.0 };
                let (r, _) = ui.allocate_exact_size(Vec2::new(64.0, 40.0), Sense::hover());
                ui.painter().rect(
                    r,
                    8.0,
                    if dark { ui.visuals().extreme_bg_color } else { ui.visuals().faint_bg_color },
                    ui.visuals().widgets.noninteractive.bg_stroke,
                    egui::StrokeKind::Inside,
                );
                crate::canvas::paint_stroke(&ui.painter_at(r), &sample, r.min + Vec2::new(6.0, 2.0));
                if *self.st.pen() != before { self.st.canvas.tool = Tool::Pen; self.st.dirty = true; }
            });
        }
        if (self.st.p.erase_partial, self.st.p.eraser_size, self.st.p.hl_width, self.st.p.hl_color) != before { self.st.dirty = true; }
        group(ui, "Shapes", true, |ui| {
            for (s, icon, label) in [(Shape::Line, "╱", "Line"), (Shape::Arrow, "↗", "Arrow"), (Shape::Rect, "▭", "Box"), (Shape::Ellipse, "◯", "Circle")] {
                if rbtn(ui, icon, label, self.st.canvas.tool == Tool::Shape(s)).clicked() { self.st.canvas.tool = Tool::Shape(s); }
            }
        });
        let any = !self.st.canvas.selected.is_empty();
        let show_paste = !self.st.canvas.ink_clip.is_empty() && !any && self.st.canvas.tool == Tool::Lasso;
        let has_sel_group = any || show_paste;
        // Keep Convert in a predictable place; Selection appears cleanly at the end when active.
        group(ui, "Convert", has_sel_group, |ui| {
            if rbtn(ui, "✎→A", "Ink to text", false).tip("Turn handwriting (selected, or all on the page) into typed text — works offline").clicked() { self.ink_convert(false); }
            if rbtn(ui, "✎→∑", "Ink to math", false).tip("Turn handwritten math into a LaTeX equation — works offline").clicked() { self.ink_convert(true); }
        });
        if any {
            group(ui, "Selection", false, |ui| {
                ui.add_enabled_ui(any, |ui| {
                    if rbtn(ui, "🎨", "Recolour", false).tip("Give the selected ink the current pen colour").clicked() {
                        let (sel, col) = (self.st.canvas.selected.clone(), self.st.pen().color);
                        let page = self.page();
                        let snap = page.clone();
                        for s in page.strokes.iter_mut().filter(|s| sel.contains(&s.id) && !s.highlighter) { s.color = col; }
                        self.st.snap(&snap);
                    }
                    if rbtn(ui, "⧉", "Duplicate", false).clicked() {
                        let sel = self.st.canvas.selected.clone();
                        let page = self.page();
                        let snap = page.clone();
                        let copies: Vec<Stroke> = page.strokes.iter().filter(|s| sel.contains(&s.id)).map(|s| Stroke { id: new_id(), points: s.points.iter().map(|p| [p[0] + 24.0, p[1] + 24.0, p[2]]).collect(), ..s.clone() }).collect();
                        let ids = copies.iter().map(|s| s.id).collect();
                        page.strokes.extend(copies);
                        self.st.snap(&snap);
                        self.st.canvas.selected = ids;
                    }
                    if rbtn(ui, "⎘", "Copy", false).tip("Copy the selected ink (Ctrl+C); paste with Ctrl+V, also on another page").clicked() { self.copy_ink(ui.ctx(), false); }
                    if rbtn(ui, "🗑", "Delete", false).clicked() { self.delete_selected_ink(); }
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            for c in PEN_COLORS {
                                if swatch(ui, c, false).clicked() {
                                    let sel = self.st.canvas.selected.clone();
                                    let page = self.page();
                                    let snap = page.clone();
                                    for s in page.strokes.iter_mut().filter(|s| sel.contains(&s.id)) {
                                        s.color = if s.highlighter { [c[0], c[1], c[2], s.color[3]] } else { c };
                                    }
                                    self.st.snap(&snap);
                                }
                            }
                        });
                    });
                });
            });
        } else if show_paste {
            group(ui, "Selection", false, |ui| {
                if rbtn(ui, "⎗", "Paste", false).tip("Paste the copied ink (Ctrl+V)").clicked() { self.paste_ink(); }
            });
        }
    }

    fn ribbon_ai(&mut self, ui: &mut egui::Ui) {
        group(ui, "This page", true, |ui| {
            if rbtn(ui, "✦", "Summarize", false).clicked() { self.chat(tr("Summarize this page in a few bullet points.").into()); }
            if rbtn(ui, "▤", "Flashcards", false).tip("Write flashcards from this page into it").clicked() { self.ai_cards(); }
            if rbtn(ui, "☑", "Action items", false).clicked() { self.chat(tr("List the action items from this page as a checklist.").into()); }
            if rbtn(ui, "?", "Quiz me", false).clicked() { self.chat(tr("Write 3 short quiz questions about this page. Put the answers at the end.").into()); }
            if rbtn(ui, "💡", "Explain", false).clicked() { self.chat(tr("Explain the main idea of this page simply, with one example.").into()); }
        });
        let has_text = self.st.canvas.last_text.is_some();
        group(ui, "Writing", true, |ui| {
            ui.add_enabled_ui(has_text, |ui| {
                if rbtn(ui, "✓", "Fix spelling", false).tip("Fix spelling and grammar in the note you last typed in").clicked() { self.rewrite(Rewrite::Replace, "Fix spelling and grammar. Change nothing else."); }
                if rbtn(ui, "✂", "Shorter", false).clicked() { self.rewrite(Rewrite::Replace, "Make this more concise without losing information."); }
                if rbtn(ui, "➜", "Continue", false).clicked() { self.rewrite(Rewrite::Append, "Continue this note with one or two short paragraphs in the same style. Reply with only the new text."); }
            });
        });
        group(ui, "Equation from words", true, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let r = ui.add(egui::TextEdit::singleline(&mut self.st.jobs.eq_prompt).hint_text(tr("e.g. area of a circle")).desired_width(170.0));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if ui.button(tr("∑ Insert equation")).clicked() || enter { self.equation_from_words(); }
            });
        });
        group(ui, "Handwriting", true, |ui| {
            if rbtn(ui, "✎→A", "Ink to text", false).clicked() { self.ink_convert(false); }
            if rbtn(ui, "✎→∑", "Ink to math", false).clicked() { self.ink_convert(true); }
        });
        group(ui, "Your AI", false, |ui| {
            if rbtn(ui, "⚙", "Settings", self.st.modals.has_settings()).clicked() { self.st.modals.toggle_settings(3); }
            ui.vertical(|ui| {
                ui.add_space(8.0);
                let ready = self.st.ai.ready();
                ui.label(RichText::new(tr(if ready { "● Connected with your key" } else { "○ No key yet" })).size(11.5).color(if ready { accent(ui) } else { ui.visuals().weak_text_color() }));
                ui.label(RichText::new(&self.st.ai.model).size(10.5).weak());
            });
        });
    }

    fn ribbon_study(&mut self, ui: &mut egui::Ui) {
        let page = &self.store.notebooks[self.nb].sections[self.sec].pages[self.pg];
        let (is_deck, n_cards) = (page.kind == PageKind::Deck, page.deck().len());
        let due_all = self.st.cache.due_now;
        group(ui, "Today", true, |ui| {
            if ui.add_enabled_ui(due_all > 0, |ui| rbtn(ui, "⏰", &tf!("Review due ({due_all})", due_all = due_all), due_all > 0)).inner.tip("Cards from all your decks that are due for review").clicked() {
                self.start_review();
            }
        });
        group(ui, "Decks", true, |ui| {
            if rbtn(ui, "+", "New deck", false).tip("A new page of flashcards in this section").clicked() { self.new_deck("New deck", vec![]); }
            ui.add_enabled_ui(is_deck && n_cards > 0, |ui| {
                if rbtn(ui, "▶", &tf!("Study deck ({n_cards})", n_cards = n_cards), false).tip("Go through this deck's cards").clicked() { self.start_deck(); }
            });
        });
        group(ui, "Make a deck from these notes", false, |ui| {
            ui.add_enabled_ui(!is_deck, |ui| {
                if rbtn(ui, "▤", "Suggest cards", false).tip("Builds a deck from questions, definitions and highlights on this page — you can edit everything after").clicked() { self.deck_from_notes(); }
                if rbtn(ui, "✦", "With AI", false).tip("Your AI writes a deck from this page (needs your AI key)").clicked() { self.ai_cards(); }
            });
        });
    }

    fn ribbon_view(&mut self, ui: &mut egui::Ui) {
        let paper = self.page().paper;
        let mut set = None;
        group(ui, "Paper", true, |ui| {
            for (p, icon, label) in PAPERS {
                if rbtn(ui, icon, label, paper == p).clicked() { set = Some(p); }
            }
        });
        if let Some(p) = set {
            let snap = self.page().clone();
            self.page().paper = p;
            self.st.snap(&snap);
        }
        group(ui, "Page zoom", true, |ui| {
            if rbtn(ui, "−", "Zoom out", false).tip("Or pinch / Ctrl+scroll on the page").clicked() { self.st.canvas.zoom_req = Some(ZoomReq::By(1.0 / 1.25)); }
            if rbtn(ui, &format!("{:.0}%", self.st.canvas.zoom_now * 100.0), "Actual size", false).clicked() { self.st.canvas.zoom_req = Some(ZoomReq::To(1.0)); }
            if rbtn(ui, "+", "Zoom in", false).clicked() { self.st.canvas.zoom_req = Some(ZoomReq::By(1.25)); }
            if rbtn(ui, "⇔", "Fit width", false).clicked() { self.st.canvas.zoom_req = Some(ZoomReq::FitWidth); }
        });
        let ctx = ui.ctx().clone();
        group(ui, "Interface", true, |ui| {
            let z = ctx.zoom_factor();
            let mut set = |z: f32| {
                let z = (z * 10.0).round() / 10.0;
                ctx.set_zoom_factor(z.clamp(0.6, 2.5));
                self.st.p.zoom = z.clamp(0.6, 2.5);
                self.st.dirty = true;
            };
            if rbtn(ui, "−", "Smaller", false).clicked() { set(z - 0.1); }
            if rbtn(ui, &format!("{:.0}%", z * 100.0), "Reset", false).clicked() { set(1.0); }
            if rbtn(ui, "+", "Larger", false).clicked() { set(z + 0.1); }
        });
        group(ui, "Look", true, |ui| {
            let dark = self.st.p.dark;
            if rbtn(ui, if dark { "☀" } else { "☾" }, if dark { "Light mode" } else { "Dark mode" }, false).clicked() {
                self.st.p.dark ^= true;
                self.st.p.follow_system = false;
                apply_theme(ui.ctx(), self.st.p.dark);
                self.st.dirty = true;
            }
            let focus = !self.st.p.show_nav && !self.st.p.show_ai;
            if rbtn(ui, "⛶", "Focus", focus).tip("Hide the side panels to concentrate on the page").clicked() {
                self.st.p.show_nav = focus;
                self.st.p.show_ai = focus;
                self.st.dirty = true;
            }
            let full_w = ui.ctx().content_rect().width();
            if rbtn(ui, "☰", "Notebooks", self.st.p.show_nav).clicked() {
                self.st.p.show_nav ^= true;
                if self.st.p.show_nav && full_w < 760.0 { self.st.p.show_ai = false; }
                self.st.dirty = true;
            }
            if rbtn(ui, "✦", "Assistant", self.st.p.show_ai).clicked() {
                self.st.p.show_ai ^= true;
                if self.st.p.show_ai && full_w < 760.0 { self.st.p.show_nav = false; }
                self.st.dirty = true;
            }
        });
        group(ui, "Help", false, |ui| {
            if rbtn(ui, "⌨", "Shortcuts", self.st.modals.has_shortcuts()).clicked() { self.st.modals.toggle(ActiveModal::Shortcuts); }
            if rbtn(ui, "🗑", &tf!("Deleted ({trash_n})", trash_n = self.store.trash.len()), self.st.modals.has_trash()).clicked() {
                self.st.modals.toggle(ActiveModal::Trash);
            }
        });
    }
}

/// Soft translucent edge fade indicating horizontally scrolled ribbon content.
fn paint_edge_fade(painter: &egui::Painter, rect: Rect, left: bool, bg: Color32) {
    let steps = 5;
    let total_w = 16.0;
    let sw = total_w / steps as f32;
    for i in 0..steps {
        let t = 1.0 - (i as f32 / steps as f32);
        let alpha = (t * t * 185.0) as u8;
        let col = Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), alpha);
        let r = if left {
            Rect::from_min_max(
                Pos2::new(rect.left() + i as f32 * sw, rect.top()),
                Pos2::new(rect.left() + (i + 1) as f32 * sw, rect.bottom()),
            )
        } else {
            Rect::from_min_max(
                Pos2::new(rect.right() - (i + 1) as f32 * sw, rect.top()),
                Pos2::new(rect.right() - i as f32 * sw, rect.bottom()),
            )
        };
        painter.rect_filled(r, 0.0, col);
    }
}
