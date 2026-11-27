//! The "/" insert menu and "[[" page-link autocomplete popup inside text blocks.

use crate::*;
use eframe::egui::{self, RichText};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum PopKind {
    Slash,
    Link,
}

/// An open "/" menu or "[[" page list: which note, where it started (char index), the highlighted row.
#[derive(Clone, Debug)]
pub(crate) struct Popup {
    pub block: u64,
    pub start: usize,
    pub kind: PopKind,
    pub sel: usize,
}

pub(crate) enum Act {
    Prefix(&'static str),
    Insert(&'static str),
    Block(u8),
    Date,
    Link(String),
}

pub(crate) fn popup_query(ctx: &egui::Context, id: egui::Id, text: &str, pp: &Popup) -> String {
    let cur = egui::text_edit::TextEditState::load(ctx, id)
        .and_then(|s| s.cursor.char_range())
        .map_or(text.chars().count(), |r| r.primary.index.0);
    let from = pp.start + if pp.kind == PopKind::Slash { 1 } else { 2 };
    text.chars().skip(from).take(cur.saturating_sub(from)).collect()
}

pub(crate) fn popup_items(pp: &Popup, q: &str, titles: &[String]) -> Vec<(String, &'static str, Act)> {
    let ql = q.to_lowercase();
    match pp.kind {
        PopKind::Slash => {
            let all: Vec<(&str, &'static str, Act)> = vec![
                ("Heading 1", "H1", Act::Prefix("# ")),
                ("Heading 2", "H2", Act::Prefix("## ")),
                ("Heading 3", "H3", Act::Prefix("### ")),
                ("Bulleted list", "•", Act::Prefix("- ")),
                ("Numbered list", "1.", Act::Prefix("1. ")),
                ("To-do", "☐", Act::Prefix("[ ] ")),
                ("Quote", "❝", Act::Prefix("> ")),
                ("Inline math", "$x$", Act::Insert("$|$")),
                ("Link to a page", "↗", Act::Insert("[[|")),
                ("Date and time", "🕓", Act::Date),
                ("Equation", "∑", Act::Block(0)),
                ("Table", "▦", Act::Block(1)),
                ("Checklist", "☑", Act::Block(2)),
                ("Code", "{ }", Act::Block(3)),
            ];
            all.into_iter()
                .filter(|(l, ..)| tr(l).to_lowercase().contains(&ql) || l.to_lowercase().contains(&ql))
                .map(|(l, i, a)| (l.to_string(), i, a))
                .collect()
        }
        PopKind::Link => {
            let mut found: Vec<&String> = titles.iter().filter(|t| t.to_lowercase().contains(&ql)).collect();
            found.sort_by_key(|t| !t.to_lowercase().starts_with(&ql));
            let mut v: Vec<(String, &'static str, Act)> = found.into_iter().take(7).map(|t| (t.clone(), "↗", Act::Link(t.clone()))).collect();
            let q = q.trim();
            if !q.is_empty() && !titles.iter().any(|t| t.eq_ignore_ascii_case(q)) {
                v.push((tf!("New page “{q}”", q = q), "+", Act::Link(q.to_string())));
            }
            v
        }
    }
}

pub(crate) fn set_cursor(ctx: &egui::Context, id: egui::Id, c: usize) {
    use egui::text::{CCursor, CCursorRange};
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
    state.store(ctx, id);
}

/// Carry out row `i` of the open menu. Returns true if the text changed.
pub(crate) fn apply_popup(ctx: &egui::Context, id: egui::Id, block_id: u64, text: &mut String, i: usize, st: &mut St) -> bool {
    let Some(pp) = st.canvas.popup.take() else { return false };
    let q = popup_query(ctx, id, text, &pp);
    let items = popup_items(&pp, &q, &st.cache.page_titles);
    let Some((_, _, act)) = items.into_iter().nth(i) else { return false };
    let byte = |t: &str, c: usize| t.char_indices().nth(c).map_or(t.len(), |(b, _)| b);
    let from = if pp.kind == PopKind::Link { pp.start + 2 } else { pp.start };
    let to = pp.start + if pp.kind == PopKind::Slash { 1 } else { 2 } + q.chars().count();
    let (fb, tb) = (byte(text, from), byte(text, to));
    text.replace_range(fb..tb, "");
    let mut cur = from;
    match act {
        Act::Prefix(p) => {
            cur = rich::toggle_prefix(text, from, from, p).0;
        }
        Act::Insert(s) => {
            let clean = s.replace('|', "");
            text.insert_str(byte(text, from), &clean);
            cur = from + s.find('|').map_or(clean.chars().count(), |b| s[..b].chars().count());
            if s.starts_with("[[") {
                st.canvas.popup = Some(Popup { block: block_id, start: from, kind: PopKind::Link, sel: 0 });
            }
        }
        Act::Date => {
            let d = crate::chrome::format_date(now_secs(), true);
            text.insert_str(byte(text, from), &d);
            cur = from + d.chars().count();
        }
        Act::Block(k) => {
            let kind = match k {
                0 => Kind::Math { src: String::new() },
                1 => Kind::Table { rows: vec![vec![String::new(); 3]; 3] },
                2 => Kind::Checklist { items: vec![(false, String::new())] },
                _ => Kind::Code { lang: "text".into(), code: String::new() },
            };
            st.canvas.insert_below = Some((block_id, kind));
        }
        Act::Link(title) => {
            let ins = format!("{title}]]");
            text.insert_str(byte(text, from), &ins);
            cur = from + ins.chars().count();
        }
    }
    set_cursor(ctx, id, cur);
    if st.canvas.insert_below.is_none() {
        st.nav.focus_id = Some(id);
        st.canvas.editing = Some(block_id);
    }
    true
}

/// Open, update, close and draw the "/" menu or "[[" page list.
/// Returns `(clicked_row, pointer_interacting_with_popup)` so clicking a row isn't cancelled by TextEdit blur.
pub(crate) fn popup_ui(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    id: egui::Id,
    block_id: u64,
    text: &str,
    out: &egui::text_edit::TextEditOutput,
    st: &mut St,
) -> (Option<usize>, bool) {
    let cur = out
        .cursor_range
        .map(|r| r.primary.index.0)
        .or_else(|| egui::text_edit::TextEditState::load(ctx, id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0));
    let Some(cur) = cur else { return (None, false) };
    let chars: Vec<char> = text.chars().collect();
    if out.response.changed() && st.canvas.popup.as_ref().is_none_or(|pp| pp.block != block_id) {
        if cur >= 1 && chars.get(cur - 1) == Some(&'/') && (cur == 1 || chars.get(cur - 2).is_some_and(|c| c.is_whitespace())) {
            st.canvas.popup = Some(Popup { block: block_id, start: cur - 1, kind: PopKind::Slash, sel: 0 });
        } else if cur >= 2 && chars.get(cur - 2) == Some(&'[') && chars.get(cur - 1) == Some(&'[') {
            st.canvas.popup = Some(Popup { block: block_id, start: cur - 2, kind: PopKind::Link, sel: 0 });
        }
    }
    let Some(pp) = st.canvas.popup.clone().filter(|pp| pp.block == block_id) else {
        return (None, false);
    };
    let q = popup_query(ctx, id, text, &pp);
    let off = if pp.kind == PopKind::Slash { 1 } else { 2 };
    let items = popup_items(&pp, &q, &st.cache.page_titles);
    let trigger_gone = chars.get(pp.start) != Some(&(if pp.kind == PopKind::Slash { '/' } else { '[' }));
    if cur < pp.start + off || trigger_gone || q.contains('\n') || (items.is_empty() && q.ends_with(' ')) {
        st.canvas.popup = None;
        return (None, false);
    }

    let sel = pp.sel.min(items.len().saturating_sub(1));
    let caret_rect = out.galley.pos_from_cursor(egui::text::CCursor::new(cur));
    let to_global = ctx.layer_transform_to_global(ui.layer_id()).unwrap_or_default();
    let caret_below = to_global * (out.galley_pos + caret_rect.left_bottom().to_vec2() + Vec2::new(0.0, 6.0));
    let caret_above = to_global * (out.galley_pos + caret_rect.left_top().to_vec2() - Vec2::new(0.0, 6.0));

    // Clamp popup inside visible screen area (accounting for Android system bars/keyboard),
    // flipping above the caret when room below is tighter than room above.
    let screen = ctx.content_rect();
    let top_bound = screen.top() + st.bars[0] + 8.0;
    let bot_bound = screen.bottom() - st.bars[1] - 8.0;
    let pop_w = 256.0f32.min((screen.width() - 16.0).max(180.0));
    let est_h = (items.len().max(1) as f32 * 30.0 + 56.0).min(300.0);
    let room_below = (bot_bound - caret_below.y).max(0.0);
    let room_above = (caret_above.y - top_bound).max(0.0);
    let (y, max_scroll_h) = if room_below >= est_h || room_below >= room_above {
        (caret_below.y.clamp(top_bound, (bot_bound - 120.0).max(top_bound)), (bot_bound - caret_below.y - 54.0).clamp(90.0, 260.0))
    } else {
        let h = est_h.min(room_above.max(120.0));
        ((caret_above.y - h).max(top_bound), (h - 54.0).clamp(90.0, 260.0))
    };
    let x = caret_below.x.clamp(screen.left() + 8.0, (screen.right() - pop_w - 8.0).max(screen.left() + 8.0));

    let mut picked = None;
    let area_resp = egui::Area::new(egui::Id::unique(("popup", block_id)))
        .order(egui::Order::Foreground)
        .fixed_pos(Pos2::new(x, y))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(6).show(ui, |ui| {
                ui.set_width(pop_w - 12.0);
                ui.label(RichText::new(tr(if pp.kind == PopKind::Slash { "INSERT" } else { "LINK TO A PAGE" })).size(10.5).weak());
                if items.is_empty() {
                    ui.label(RichText::new(tr("No matches")).weak());
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt(("popup-scroll", block_id))
                        .max_height(max_scroll_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (i, (label, icon, _)) in items.iter().enumerate() {
                                let r = ui.add(
                                    egui::Button::new(RichText::new(tf!("{icon}   {label}", icon = icon, label = tr(label))).size(13.5))
                                        .selected(i == sel)
                                        .frame_when_inactive(false)
                                        .min_size(Vec2::new(ui.available_width(), 28.0)),
                                );
                                if r.clicked() {
                                    picked = Some(i);
                                }
                            }
                        });
                }
                if !st.nav.touch_ui {
                    ui.label(RichText::new(tr("↑↓ to choose · Enter to insert · Esc to close")).size(10.5).weak());
                }
            });
        })
        .response;

    let pointer_in_popup = ctx.input(|i| {
        i.pointer
            .interact_pos()
            .or_else(|| i.pointer.press_origin())
            .is_some_and(|p| area_resp.rect.expand(4.0).contains(p))
    });
    let interacting = picked.is_some() || area_resp.hovered() || pointer_in_popup;
    if !out.response.has_focus() && !out.response.lost_focus() && !interacting {
        st.canvas.popup = None;
    }
    (picked, interacting)
}
