//! The card editor of a flashcard deck page.

use crate::*;
use eframe::egui::RichText;

/// The card editor of a flashcard deck. Returns true when "Study" is pressed.
pub(crate) fn deck_view(
    ui: &mut egui::Ui,
    page: &mut Page,
    st: &mut St,
    study: &std::collections::HashMap<String, CardState>,
) -> bool {
    let acc = accent(ui);
    let now = now_secs();
    let weak = ui.visuals().weak_text_color();
    let status = |c: &DeckCard| match study.get(&format!("card{}", c.id)) {
        None => ("new".to_string(), acc),
        Some(s) if s.due <= now => ("due".to_string(), Color32::from_rgb(214, 120, 40)),
        Some(s) => {
            let d = (s.due - now).div_ceil(86_400);
            (if d <= 1 { "next tomorrow".into() } else { tf!("next in {d} days", d = d) }, weak)
        }
    };
    let ready = page.deck().len();
    let (due, new) = page.cards.iter().fold((0, 0), |(d, n), c| match status(c).0.as_str() {
        "due" => (d + 1, n),
        "new" if !c.front.trim().is_empty() && !c.back.trim().is_empty() => (d, n + 1),
        _ => (d, n),
    });
    let mut study_now = false;
    let stack = ui.available_width() < 520.0;
    let chip = |ui: &mut egui::Ui, t: String, c: Color32| {
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(10, 4))
            .show(ui, |ui| ui.label(RichText::new(t).size(12.5).color(c)));
    };
    let study_btn = |ui: &mut egui::Ui, full_w: bool| -> bool {
        let min_w = if full_w { ui.available_width() } else { 150.0 };
        let b = ui.add_enabled(
            ready > 0,
            egui::Button::new(
                RichText::new(tf!("▶  Study{x}", x = if due + new > 0 { format!(" ({})", due + new) } else { String::new() }))
                    .size(15.0)
                    .color(Color32::WHITE),
            )
            .fill(accent_base())
            .corner_radius(18)
            .min_size(Vec2::new(min_w, 36.0)),
        );
        b.tip("Go through the cards; due and new ones come first").clicked()
    };
    if stack {
        ui.horizontal_wrapped(|ui| {
            chip(ui, if page.cards.len() == 1 { tr("▤  1 card").to_string() } else { tf!("▤  {cards_n} cards", cards_n = page.cards.len()) }, ui.visuals().text_color());
            if due > 0 { chip(ui, tf!("{due} due", due = due), Color32::from_rgb(214, 120, 40)); }
            if new > 0 { chip(ui, tf!("{new} new", new = new), acc); }
        });
        ui.add_space(4.0);
        if study_btn(ui, true) {
            study_now = true;
        }
    } else {
        ui.horizontal(|ui| {
            chip(ui, if page.cards.len() == 1 { tr("▤  1 card").to_string() } else { tf!("▤  {cards_n} cards", cards_n = page.cards.len()) }, ui.visuals().text_color());
            if due > 0 { chip(ui, tf!("{due} due", due = due), Color32::from_rgb(214, 120, 40)); }
            if new > 0 { chip(ui, tf!("{new} new", new = new), acc); }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(12.0);
                if study_btn(ui, false) {
                    study_now = true;
                }
            });
        });
    }
    ui.add_space(10.0);
    let mut add_after: Option<usize> = None;
    let mut remove = None;
    let mut swap = None;
    let mut changed = false;
    let mut focus_snap = false;
    let colors = rich::Colors::from(ui, acc);
    egui::ScrollArea::vertical().id_salt(("deck", page.id)).auto_shrink([false; 2]).show(ui, |ui| {
        ui.set_max_width((ui.available_width().min(900.0) - 12.0).max(160.0));
        let n = page.cards.len();
        for (i, c) in page.cards.iter_mut().enumerate() {
            let (label, col) = status(c);
            egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{}", i + 1)).size(13.0).weak());
                        ui.label(RichText::new(tr(&label)).size(11.0).color(col));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new("🗑").frame_when_inactive(false)).tip("Delete this card").clicked() { remove = Some(i); }
                            if i + 1 < n && ui.add(egui::Button::new("↓").frame_when_inactive(false)).tip("Move down").clicked() { swap = Some((i, i + 1)); }
                            if i > 0 && ui.add(egui::Button::new("↑").frame_when_inactive(false)).tip("Move up").clicked() { swap = Some((i, i - 1)); }
                        });
                    });
                    let mut draw_side = |ui: &mut egui::Ui, k: usize, side: &str, text: &mut String, hint: &str| {
                        ui.label(RichText::new(side).size(10.5).weak());
                        let id = egui::Id::unique((if k == 0 { "cardf" } else { "cardb" }, c.id));
                        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                            let mut job = rich::edit_job(buf.as_str(), 15.0, 0, &colors);
                            job.wrap.max_width = wrap;
                            ui.fonts_mut(|f| f.layout_job(job))
                        };
                        let r = ui.add(
                            egui::TextEdit::multiline(text)
                                .id(id)
                                .hint_text(hint)
                                .desired_rows(2)
                                .desired_width(f32::INFINITY)
                                .layouter(&mut layouter)
                                .background_color(ui.visuals().faint_bg_color)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        if st.nav.focus_id == Some(id) {
                            r.request_focus();
                            st.nav.focus_id = None;
                        }
                        focus_snap |= r.gained_focus();
                        changed |= r.changed();
                        if k == 1 && r.has_focus() && ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter)) {
                            add_after = Some(i);
                        }
                        if text.contains('$') {
                            egui::Frame::new().inner_margin(egui::Margin { left: 6, right: 0, top: 4, bottom: 0 }).show(ui, |ui| {
                                let _ = rich::view(ui, text, 14.0, 0, &colors);
                            });
                        }
                    };
                    if stack {
                        draw_side(ui, 0, tr("FRONT"), &mut c.front, tr("Question or term"));
                        ui.add_space(6.0);
                        draw_side(ui, 1, tr("BACK"), &mut c.back, tr("Answer"));
                    } else {
                        ui.columns(2, |cols| {
                            draw_side(&mut cols[0], 0, tr("FRONT"), &mut c.front, tr("Question or term"));
                            draw_side(&mut cols[1], 1, tr("BACK"), &mut c.back, tr("Answer"));
                        });
                    }
                });
            ui.add_space(8.0);
        }
        if let Some(i) = add_after {
            let c = DeckCard::new("", "");
            st.nav.focus_id = Some(egui::Id::unique(("cardf", c.id)));
            st.snap(page);
            page.cards.insert(i + 1, c);
        }
        if page.cards.is_empty() {
            ui.label(RichText::new(tr("No cards yet.")).weak());
        }
        let add = ui.add(
            egui::Button::new(RichText::new(tr("+  Add card")).size(14.0).color(acc))
                .min_size(Vec2::new(ui.available_width(), 40.0))
                .corner_radius(12)
                .fill(ui.visuals().faint_bg_color)
                .stroke(EStroke::new(1.0, acc.gamma_multiply(0.5))),
        );
        if add.clicked() {
            let c = DeckCard::new("", "");
            st.nav.focus_id = Some(egui::Id::unique(("cardf", c.id)));
            st.snap(page);
            page.cards.push(c);
        }
        ui.add_space(6.0);
        let bulk_id = egui::Id::unique(("bulk", page.id));
        let mut bulk: Option<String> = ui.data(|d| d.get_temp(bulk_id));
        match &mut bulk {
            None => {
                if ui
                    .add(egui::Button::new(tr("Add many from text…")).frame_when_inactive(false))
                    .tip("One card per line: “front :: back”, front and back separated by a tab, or Q: and A: lines")
                    .clicked()
                {
                    bulk = Some(String::new());
                }
            }
            Some(t) => {
                ui.add(
                    egui::TextEdit::multiline(t)
                        .desired_rows(5)
                        .desired_width(f32::INFINITY)
                        .hint_text(tr("term :: meaning\nterm<Tab>meaning\nQ: question\nA: answer")),
                );
                let (mut add, mut cancel) = (false, false);
                ui.horizontal(|ui| {
                    add = ui.button(tr("Add these cards")).clicked();
                    cancel = ui.button(tr("Cancel")).clicked();
                });
                if add {
                    let new = cards_from_text(t);
                    let n = new.iter().filter(|(f, _)| !page.cards.iter().any(|c| c.front.trim() == f.trim())).count();
                    st.snap(page);
                    for (f, b) in new {
                        if !page.cards.iter().any(|c| c.front.trim() == f.trim()) {
                            page.cards.push(DeckCard::new(&f, &b));
                        }
                    }
                    changed = true;
                    st.say(tf!("Added {n} card(s).", n = n));
                }
                if add || cancel {
                    bulk = None;
                }
            }
        }
        match bulk {
            Some(b) => { ui.data_mut(|d| d.insert_temp(bulk_id, b)); }
            None => ui.data_mut(|d| d.remove::<String>(bulk_id)),
        }
        ui.label(RichText::new(tr("Tip: Tab jumps from front to back, Ctrl+Enter adds the next card. Use $x^2$ for maths and **bold** for emphasis.")).size(11.5).weak());
        ui.add_space(30.0);
    });
    if focus_snap {
        st.snap(page);
    }
    if let Some(i) = remove {
        st.snap(page);
        page.cards.remove(i);
    }
    if let Some((a, b)) = swap {
        st.snap(page);
        page.cards.swap(a, b);
    }
    if changed {
        st.dirty = true;
        st.cache.derived_dirty = true;
    }
    study_now
}
