//! Everything around the page: title bar, ribbon, navigation, search, AI pane, status bar and dialogs.

use crate::*;
use eframe::egui::{Align, Align2, FontId, Layout, Rect, RichText, Sense};


fn want_focus(st: &mut St, r: &egui::Response) {
    if st.focus_id == Some(r.id) { r.request_focus(); st.focus_id = None; }
}

/// Ribbon button: icon over a small label.
pub(crate) fn rbtn(ui: &mut egui::Ui, icon: &str, label: &str, selected: bool) -> egui::Response {
    let label = tr(label);
    let lw = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(11.0), Color32::WHITE).size().x;
    let (rect, r) = ui.allocate_exact_size(Vec2::new((lw + 14.0).max(50.0), 50.0), Sense::click());
    let v = ui.visuals();
    let acc = accent(ui);
    if selected { ui.painter().rect_filled(rect, 6.0, v.selection.bg_fill); } else if r.hovered() { ui.painter().rect_filled(rect, 6.0, v.widgets.hovered.weak_bg_fill); }
    let col = if !ui.is_enabled() { v.weak_text_color() } else if selected { acc } else { v.text_color() };
    ui.painter().text(rect.center_top() + Vec2::new(0.0, 17.0), Align2::CENTER_CENTER, icon, FontId::proportional(18.0), col);
    ui.painter().text(rect.center_bottom() - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, label, FontId::proportional(11.0), if ui.is_enabled() { v.text_color() } else { v.weak_text_color() });
    focus_ring(ui, &r, rect, 6.0);
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
    r
}

/// A visible ring around the widget that keyboard focus is on (Tab moves it).
pub(crate) fn focus_ring(ui: &egui::Ui, r: &egui::Response, rect: Rect, radius: f32) {
    if r.has_focus() { ui.painter().rect_stroke(rect.expand(2.0), radius + 2.0, EStroke::new(2.0, accent(ui)), egui::StrokeKind::Outside); }
}

/// A two-or-more-way switch drawn as one pill (like the tabs): returns the option clicked.
pub(crate) fn segmented(ui: &mut egui::Ui, options: &[(&str, &str)], selected: usize) -> Option<usize> {
    let font = FontId::proportional(13.0);
    let w = options.iter().map(|(l, _)| ui.painter().layout_no_wrap(tr(l).to_string(), font.clone(), Color32::WHITE).size().x).fold(0.0, f32::max) + 22.0;
    let h = ui.spacing().interact_size.y.max(26.0);
    let (track, _) = ui.allocate_exact_size(Vec2::new(w * options.len() as f32 + 6.0, h), Sense::hover());
    let v = ui.visuals().clone();
    ui.painter().rect_filled(track, h / 2.0, if v.dark_mode { Color32::from_rgb(30, 36, 37) } else { Color32::from_rgb(222, 229, 226) });
    let mut picked = None;
    for (i, (label, tip)) in options.iter().enumerate() {
        let r = Rect::from_min_size(track.min + Vec2::new(3.0 + i as f32 * w, 3.0), Vec2::new(w, h - 6.0));
        let resp = ui.interact(r, ui.scope_id().with(("seg", label)), Sense::click());
        focus_ring(ui, &resp, r, (h - 6.0) / 2.0);
        let label = tr(label).to_string();
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, i == selected, &label));
        if i == selected {
            ui.painter().add(egui::Shape::rect_filled(r.translate(Vec2::new(0.0, 1.0)), (h - 6.0) / 2.0, Color32::from_black_alpha(18)));
            ui.painter().rect_filled(r, (h - 6.0) / 2.0, v.extreme_bg_color);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, (h - 6.0) / 2.0, v.widgets.hovered.weak_bg_fill);
        }
        ui.painter().text(r.center(), Align2::CENTER_CENTER, &label, font.clone(), if i == selected { accent(ui) } else { v.text_color() });
        if resp.tip(*tip).clicked() { picked = Some(i); }
    }
    picked
}

/// A small caption, then a slider that ends exactly `width` from where the row starts.
/// Pen colours a tap away (black, blue, red, green, purple, orange, brown, grey).
const PEN_COLORS: [[u8; 4]; 8] = [[30, 30, 30, 255], [37, 99, 235, 255], [220, 38, 38, 255], [22, 163, 74, 255], [124, 58, 237, 255], [234, 88, 12, 255], [120, 72, 40, 255], [107, 114, 128, 255]];

/// A small round colour button.
fn swatch(ui: &mut egui::Ui, c: [u8; 4], selected: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
    ui.painter().circle_filled(r.center(), 7.0, c4([c[0], c[1], c[2], 255]));
    if selected || resp.hovered() { ui.painter().circle_stroke(r.center(), 8.5, EStroke::new(1.5, ui.visuals().text_color())); }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])));
    resp
}

/// The last colours picked by hand, newest first (four kept).
fn remember_color(v: &mut Vec<[u8; 4]>, c: [u8; 4]) {
    if PEN_COLORS.contains(&c) { return; }
    v.retain(|x| *x != c);
    v.insert(0, c);
    v.truncate(4);
}

fn labelled_slider(ui: &mut egui::Ui, caption: &str, width: f32, value: &mut f32, range: std::ops::RangeInclusive<f32>, tip: &str) {
    ui.horizontal(|ui| {
        let start = ui.cursor().left();
        let label = ui.label(RichText::new(tr(caption)).size(11.5).weak());
        ui.spacing_mut().slider_width = (width - (ui.cursor().left() - start)).max(40.0);
        ui.add(egui::Slider::new(value, range).show_value(false)).labelled_by(label.id).tip(tip);
    });
}

/// Small square ribbon button used for text formatting.
fn sbtn(ui: &mut egui::Ui, text: RichText, tip: &str, selected: bool) -> egui::Response {
    let b = egui::Button::new(text).min_size(Vec2::new(28.0, 22.0)).selected(selected).frame_when_inactive(false);
    ui.add(b).tip(tip)
}

/// A captioned ribbon group followed by a divider.
fn group(ui: &mut egui::Ui, caption: &str, add: impl FnOnce(&mut egui::Ui)) {
    let caption = tr(caption);
    // room for two rows of controls in every group: captions line up and the ribbon keeps its height across tabs
    let h = (ui.spacing().interact_size.y * 2.0 + 6.0).max(52.0);
    let captions = ui.spacing().interact_size.y < 30.0; // the touch ribbon (see `ribbon`) leaves them out
    ui.vertical(|ui| {
        let r = ui.horizontal(|ui| { ui.set_min_height(h); add(ui); }).response.rect;
        if captions {
            // a caption longer than its buttons widens the group rather than running into the next one
            let g = ui.painter().layout_no_wrap(caption.to_string(), FontId::proportional(10.5), ui.visuals().weak_text_color());
            let (cr, _) = ui.allocate_exact_size(Vec2::new(r.width().max(g.size().x + 8.0), 13.0), Sense::hover());
            ui.painter().galley(cr.center() - g.size() / 2.0, g, ui.visuals().weak_text_color());
        }
    });
    let (r, _) = ui.allocate_exact_size(Vec2::new(9.0, if captions { h + 12.0 } else { h }), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range().shrink(6.0), EStroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
}


/// A moment as the clock on this device's wall shows it (local time, not UTC).
/// "2026-11-09" (local date), for file names.
pub(crate) fn day_name(secs: u64) -> String {
    let d = (secs as i64 + local_offset(secs)).max(0) / 86_400;
    let z = d + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (day, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{m:02}-{day:02}", yoe + era * 400 + (m <= 2) as i64)
}

pub(crate) fn format_date(secs: u64, time: bool) -> String {
    format_utc((secs as i64 + local_offset(secs)).max(0) as u64, time)
}

/// The date without its year, in the reading language ("Wednesday, November 4", "Mittwoch, 4. November"), for page titles.
pub(crate) fn short_date(secs: u64) -> String { without_year(&format_date(secs, false)) }
fn without_year(d: &str) -> String { d.rsplit_once(' ').map_or(d.to_string(), |(a, _)| a.trim_end_matches(',').to_string()) }

/// This device's offset from UTC in seconds at that moment (summer time included).
fn local_offset(secs: u64) -> i64 {
    #[cfg(unix)]
    // SAFETY: localtime_r only writes the tm it is given
    unsafe {
        let t = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&t, &mut tm).is_null() { return tm.tm_gmtoff as i64; }
    }
    #[cfg(windows)]
    // SAFETY: GetTimeZoneInformation only fills the struct it is given
    unsafe {
        use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        let mut tz: TIME_ZONE_INFORMATION = std::mem::zeroed();
        let r = GetTimeZoneInformation(&mut tz);
        // shortcut: today's summer/winter time, also for other dates (Windows has no per-date call this simple)
        if r != u32::MAX { return -((tz.Bias + if r == 2 { tz.DaylightBias } else { tz.StandardBias }) as i64) * 60; }
    }
    #[allow(unreachable_code)]
    { let _ = secs; 0 }
}

fn format_utc(secs: u64, time: bool) -> String {
    let days = (secs / 86_400) as i64;
    let (y, m, d) = {
        // civil-from-days (Howard Hinnant)
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        (yoe + era * 400 + (m <= 2) as i64, m, d)
    };
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    const DAYS: [&str; 7] = ["Thursday", "Friday", "Saturday", "Sunday", "Monday", "Tuesday", "Wednesday"];
    let (wd, mo) = (DAYS[days.rem_euclid(7) as usize], MONTHS[(m - 1) as usize]);
    let mut s = match crate::i18n::code() {
        "de" => format!("{}, {d}. {} {y}", tr(wd), tr(mo)),
        "fr" => format!("{} {d} {} {y}", tr(wd).to_lowercase(), tr(mo).to_lowercase()),
        _ => format!("{wd}, {mo} {d}, {y}"),
    };
    if time { s += &format!("  ·  {:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60); }
    s
}

/// The day a task is due, from "@2026-11-20" in its text (days since 1970).
pub(crate) fn due_day(text: &str) -> Option<i64> {
    let at = text.find('@')?;
    let d = text.get(at + 1..at + 11)?;
    let (y, m, day) = (d.get(0..4)?.parse::<i64>().ok()?, d.get(5..7)?.parse::<i64>().ok()?, d.get(8..10)?.parse::<i64>().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || d.as_bytes()[4] != b'-' { return None; }
    // days-from-civil (Howard Hinnant)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    Some(era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468)
}

/// A page being dragged in the pages column: (its section's id, its place there).
struct DragPage(u64, usize);

/// The Ctrl+K list's commands (in `run_command`'s order).
const COMMANDS: [&str; 11] = ["New page", "New deck", "Review due cards", "All tasks", "Settings", "Recently deleted", "Page history", "Keyboard shortcuts", "Dark mode on/off", "Export page as PDF", "New lecture"];

/// A Ctrl+K row: (score, title, notebook › section, where it is — or a command or "create").
pub(crate) type SwitchHit = (f32, String, String, (usize, usize, usize));

/// A search result: (only a near miss, where the page is, title, notebook › section, the page's text, match start, match end).
pub(crate) type SearchHit = (bool, (usize, usize, usize), String, String, String, usize, usize);

/// A study session: the deck, what's left to go, and how it went.
pub(crate) struct Study { pub title: String, pub cards: Vec<Card>, pub queue: Vec<usize>, pub total: usize, pub shown: bool, pub tally: [u32; 3], pub missed: Vec<usize>, pub undo: Option<StudyUndo> }

/// The last rating, to take back (a wrong key): the card's key and progress before, the rating, the queue and misses before.
pub(crate) struct StudyUndo { key: String, before: Option<CardState>, rating: usize, queue: Vec<usize>, missed: Vec<usize> }

/// A different order each time, so a card's place in the deck isn't a hint.
fn shuffle(v: &mut [usize]) {
    let mut x = now_f().to_bits() | 1;
    for i in (1..v.len()).rev() { x ^= x << 13; x ^= x >> 7; x ^= x << 17; v.swap(i, (x % (i as u64 + 1)) as usize); }
}

/// Section title inside the Settings window.
/// "just now", "5 min ago", "3 h ago"…
pub(crate) fn ago(t: u64) -> String {
    let d = now_secs().saturating_sub(t);
    if d < 60 { tr("just now").into() } else if d < 3600 { tf!("{n} min ago", n = d / 60) } else if d < 86_400 { tf!("{n} h ago", n = d / 3600) } else { tf!("{n} days ago", n = d / 86_400) }
}

pub(crate) fn heading(ui: &mut egui::Ui, text: &str) {
    let text = tr(text);
    ui.label(RichText::new(text).font(rich::serif(22.0)).color(ui.visuals().strong_text_color()));
    ui.add_space(2.0);
}

/// One setting: name and explanation on the left, its control on the right.
pub(crate) fn row(ui: &mut egui::Ui, title: &str, desc: &str, control: impl FnOnce(&mut egui::Ui)) {
    let (title, desc) = (tr(title), tr(desc));
    egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if ui.available_width() < 420.0 {
            ui.label(RichText::new(title).size(14.0).color(ui.visuals().strong_text_color()));
            ui.label(RichText::new(desc).size(12.0).weak());
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| { ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend); control(ui) });
            return;
        }
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - 230.0).max(160.0));
                ui.label(RichText::new(title).size(14.0).color(ui.visuals().strong_text_color()));
                ui.label(RichText::new(desc).size(12.0).weak());
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| { ui.horizontal_wrapped(|ui| { ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend); control(ui) }); });
        });
    });
}

/// Settings row whose choices sit on their own line under the description, in reading order.
pub(crate) fn row_below(ui: &mut egui::Ui, title: &str, desc: &str, control: impl FnOnce(&mut egui::Ui)) {
    let (title, desc) = (tr(title), tr(desc));
    egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).size(14.0).color(ui.visuals().strong_text_color()));
        ui.label(RichText::new(desc).size(12.0).weak());
        ui.add_space(2.0);
        ui.horizontal_wrapped(control);
    });
}

/// An on/off switch in the accent colour.
pub(crate) fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut r) = ui.allocate_exact_size(Vec2::new(42.0, 24.0), Sense::click());
    if r.clicked() { *on = !*on; r.mark_changed(); }
    let t = ui.ctx().animate_bool_responsive(r.id, *on);
    let v = ui.visuals();
    let track = if *on { accent_base() } else { v.widgets.inactive.weak_bg_fill.gamma_multiply(if v.dark_mode { 1.6 } else { 0.92 }) };
    ui.painter().rect_filled(rect, 12.0, track);
    if !*on { ui.painter().rect_stroke(rect, 12.0, v.widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside); }
    let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12.0, t);
    ui.painter().circle_filled(Pos2::new(x, rect.center().y + 0.5), 9.5, Color32::from_black_alpha(30));
    ui.painter().circle_filled(Pos2::new(x, rect.center().y), 9.0, Color32::WHITE);
    focus_ring(ui, &r, rect, 12.0);
    let on = *on;
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, ""));
    r
}

/// One line of text that ends in "…" where it doesn't fit, instead of being cut mid-letter.
pub(crate) fn one_line(painter: &egui::Painter, text: &str, font: FontId, color: Color32, max_width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping { max_width: max_width.max(10.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    painter.layout_job(job)
}

#[allow(clippy::too_many_arguments)]
fn nav_item(ui: &mut egui::Ui, st: &mut St, id: u64, name: &str, color: Option<Color32>, selected: bool, indent: u8, sub: Option<String>) -> egui::Response {
    if let Some((rid, buf)) = &mut st.rename {
        if *rid == id {
            let r = ui.add(egui::TextEdit::singleline(buf).desired_width(f32::INFINITY).id(egui::Id::unique(("rename", id))));
            if !r.has_focus() && !r.lost_focus() { r.request_focus(); }
            if r.lost_focus() { st.commit_rename = true; }
            return r;
        }
    }
    let h = if sub.is_some() { 52.0 } else { 38.0 };
    let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    let v = ui.visuals();
    // Coloured items (notebooks, sections) sit on the desk and rise as a white tab when chosen.
    let raised = color.is_some();
    if selected && raised {
        ui.painter().rect_filled(rect.translate(Vec2::new(0.0, 1.5)), 10.0, Color32::from_black_alpha(if v.dark_mode { 60 } else { 14 }));
        ui.painter().rect_filled(rect, 10.0, v.extreme_bg_color);
    } else if selected {
        ui.painter().rect_filled(rect, 10.0, v.selection.bg_fill);
    } else if r.hovered() {
        ui.painter().rect_filled(rect, 10.0, v.widgets.hovered.weak_bg_fill);
    }
    let mut x = rect.left() + 12.0 + indent as f32 * 16.0;
    if indent > 0 { ui.painter().vline(x - 9.0, rect.y_range().shrink(9.0), EStroke::new(1.5, v.widgets.noninteractive.bg_stroke.color)); }
    if let Some(c) = color {
        ui.painter().circle_filled(Pos2::new(x + 4.0, rect.center().y), 5.0, c);
        x += 18.0;
    }
    let top = if sub.is_some() { rect.top() + 17.0 } else { rect.center().y };
    let painter = ui.painter_at(rect.shrink2(Vec2::new(10.0, 2.0)));
    let font = FontId::proportional(if indent > 0 { 13.5 } else { 14.5 });
    let room = rect.right() - 10.0 - x;
    let line = |text: &str, font: FontId, color: Color32, y: f32| {
        let g = one_line(&painter, text, font, color, room);
        painter.galley(Pos2::new(x, y - g.size().y / 2.0), g, color);
    };
    line(name, font, if selected { v.strong_text_color() } else { v.text_color() }, top);
    if let Some(s) = sub { line(&s, FontId::proportional(11.5), v.weak_text_color(), rect.top() + 36.0); }
    if r.double_clicked() { st.rename = Some((id, name.to_string())); }
    focus_ring(ui, &r, rect, 10.0);
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, name));
    r
}

impl NotesApp {
    pub(crate) fn title_bar(&mut self, ui: &mut egui::Ui) {
        let desk = ui.visuals().panel_fill;
        // touch screens: every point of height goes to the page
        let my = if self.st.touch_ui { 5 } else { 10 };
        egui::Panel::top("title").show_separator_line(false).frame(egui::Frame::new().fill(desk).inner_margin(egui::Margin { left: 14, right: 14, top: my, bottom: my })).show(ui, |ui| {
            // Narrow windows (and phones): the tabs get their own row, then buttons shrink to icons.
            let full = ui.available_width();
            // Measured, not guessed: one row with labels if everything fits (the Record button
            // counts at its widest, with the timer), else one row with icons, else two rows.
            let tw = |t: &str, size: f32| ui.painter().layout_no_wrap(t.to_string(), FontId::proportional(size), Color32::WHITE).size().x;
            // short screens (phones on their side): no logo and tighter tabs, so everything fits one row
            let tight = self.st.short;
            let tabs_w = self.tabs_width(ui, if tight { 16.0 } else { 30.0 });
            let pad = ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x;
            let need = |compact: bool, search_icon: bool| {
                let brand = if tight { 0.0 } else { 34.0 } + 30.0 + if compact { 0.0 } else { tw(tr("Leaf Notes"), 19.0) } + 3.0 * ui.spacing().item_spacing.x;
                let rec = tw("●  00:00  ■", 14.0).max(if compact { 0.0 } else { tw(tr("●  Record"), 14.0) }) + pad;
                let ai = if compact { tw("✦", 14.0) } else { tw(tr("✦  Assistant"), 14.0) } + pad;
                // settings, help, undo, redo, the smallest search box, gaps
                brand + 18.0 + tabs_w + rec + ai + 4.0 * 34.0 + if search_icon { 34.0 } else { 150.0 } + 6.0 * 6.0 + 12.0
            };
            // the search box gives way to a 🔍 button before the tabs move to a second row (and on phones)
            let search_icon = full < 480.0 || (need(true, false) > full && need(true, true) <= full);
            let two_rows = need(true, search_icon) > full;
            let compact = if two_rows { full < 845.0 } else { need(false, search_icon) > full };
            let rec_w = tw(if self.st.recorder.is_some() { "●  00:00  ■" } else if compact { "●" } else { tr("●  Record") }, 14.0) + pad;
            ui.horizontal(|ui| {
                ui.set_min_height(34.0);
                // phones: the buttons' touch padding alone would push Record under the logo
                if full < 480.0 { ui.spacing_mut().button_padding.x = 6.0; ui.spacing_mut().item_spacing.x = 4.0; }
                let v = ui.visuals().clone();
                let acc = accent(ui);
                if ui.add(egui::Button::new(RichText::new("☰").size(17.0).color(v.weak_text_color())).frame_when_inactive(false)).tip("Show or hide notebooks").clicked() { self.st.p.show_nav ^= true; self.st.dirty = true; }
                if !tight {
                    // brand: the app icon itself, and the name in the serif
                    let (tile, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
                    // (loaded once, then kept; loading inside data_mut would lock egui twice)
                    let id = egui::Id::unique("logo");
                    let logo = ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
                        let img = image::load_from_memory(include_bytes!("../packaging/leafnotes.png")).map(|i| i.to_rgba8()).unwrap_or_default();
                        let t = ui.ctx().load_texture("logo", egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), egui::TextureOptions::LINEAR);
                        ui.ctx().data_mut(|d| d.insert_temp(id, t.clone()));
                        t
                    });
                    ui.painter().image(logo.id(), tile, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                if full >= 560.0 && !(compact && !two_rows) { ui.label(RichText::new(tr("Leaf Notes")).font(rich::serif(19.0)).color(v.strong_text_color())); }
                if !two_rows {
                    ui.add_space(18.0);
                    self.tabs(ui);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = if full < 480.0 { 2.0 } else { 6.0 };
                    if ui.add(egui::Button::new(RichText::new("⚙").size(16.0).color(v.weak_text_color())).frame_when_inactive(false)).tip("Settings").clicked() { self.st.show_settings ^= true; self.st.settings_tab = 0; }
                    if ui.add(egui::Button::new(RichText::new("?").size(16.0).color(v.weak_text_color())).frame_when_inactive(false)).tip("Help").clicked() { self.st.show_help ^= true; }
                    let ai_on = self.st.p.show_ai;
                    let b = egui::Button::new(RichText::new(tr(if compact { "✦" } else { "✦  Assistant" })).color(if ai_on { Color32::WHITE } else { acc }))
                        .fill(if ai_on { accent_base() } else { v.extreme_bg_color }).stroke(EStroke::new(1.0, if ai_on { accent_base() } else { v.widgets.noninteractive.bg_stroke.color }))
                        .corner_radius(17).min_size(Vec2::new(0.0, 34.0));
                    if ui.add(b).tip("Show or hide the AI assistant").clicked() { self.st.p.show_ai ^= true; self.st.dirty = true; }
                    // phones: no room for a search box; the 🔍 opens the page finder, which also searches page text
                    if search_icon {
                        if ui.add(egui::Button::new(RichText::new("🔍").size(15.0).color(v.weak_text_color())).frame_when_inactive(false)).tip("Find a page (Ctrl+K)").clicked() { self.st.switcher = Some((String::new(), 0)); }
                    } else {
                        // room left for undo, redo and Record
                        let w = (ui.available_width() - rec_w - 2.0 * 34.0 - 30.0).clamp(90.0, 300.0);
                        // The pill, icon and key hint are drawn here; only the typing area is a text box,
                        // vertically centred between them so nothing overlaps.
                        let (pill, pill_resp) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::click());
                        let id = egui::Id::unique("search");
                        let focused = ui.memory(|m| m.has_focus(id));
                        ui.painter().rect_filled(pill, 17.0, v.extreme_bg_color);
                        ui.painter().rect_stroke(pill, 17.0, if focused { EStroke::new(1.5, acc) } else { v.widgets.noninteractive.bg_stroke }, egui::StrokeKind::Inside);
                        ui.painter().text(Pos2::new(pill.left() + 16.0, pill.center().y - 1.0), Align2::CENTER_CENTER, "🔍", FontId::proportional(13.0), v.weak_text_color());
                        let key_w = if self.st.search.is_empty() && !focused && w > 170.0 && !self.st.touch_ui {
                            let g = ui.painter().layout_no_wrap(tr("Ctrl F").into(), FontId::proportional(11.0), v.weak_text_color());
                            let key = Rect::from_center_size(Pos2::new(pill.right() - 14.0 - g.size().x / 2.0 - 5.0, pill.center().y), g.size() + Vec2::new(10.0, 4.0));
                            ui.painter().rect_filled(key, 5.0, v.widgets.inactive.weak_bg_fill);
                            ui.painter().galley(key.center() - g.size() / 2.0, g, v.weak_text_color());
                            key.width() + 10.0
                        } else { 0.0 };
                        // (+2: the font's line box sits a little above the visual middle of the letters)
                        let field = Rect::from_min_max(Pos2::new(pill.left() + 30.0, pill.top() + 2.0), Pos2::new(pill.right() - 14.0 - key_w, pill.bottom() + 2.0));
                        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field).layout(Layout::left_to_right(Align::Center)));
                        let r = child.add(egui::TextEdit::singleline(&mut self.st.search).id(id).hint_text(tr("Search everything")).frame(egui::Frame::NONE)
                            .margin(egui::Margin::symmetric(4, 0)).desired_width(field.width()).font(FontId::proportional(14.0)));
                        if pill_resp.clicked() { r.request_focus(); }
                        want_focus(&mut self.st, &r);
                    }
                    if ui.add_enabled(!self.st.redo.is_empty(), egui::Button::new(RichText::new("↷").size(if self.st.touch_ui { 20.0 } else { 16.0 })).frame_when_inactive(false)).tip("Redo (Ctrl+Y)").clicked() { self.undo(true); }
                    if ui.add_enabled(!self.st.undo.is_empty(), egui::Button::new(RichText::new("↶").size(if self.st.touch_ui { 20.0 } else { 16.0 })).frame_when_inactive(false)).tip("Undo (Ctrl+Z)").clicked() { self.undo(false); }
                    ui.add_space(6.0);
                    let red = Color32::from_rgb(220, 50, 47);
                    let rec_btn = if let Some((_, _, _, start)) = &self.st.recorder {
                        let secs = (now_f() - start) as u64;
                        let pulse = (ui.input(|i| i.time) * 2.0).sin() > 0.0;
                        egui::Button::new(RichText::new(format!("{}  {:02}:{:02}  ■", if pulse { "●" } else { "○" }, secs / 60, secs % 60)).color(Color32::WHITE)).fill(red).corner_radius(17).min_size(Vec2::new(0.0, 34.0))
                    } else {
                        egui::Button::new(RichText::new(tr(if compact { "●" } else { "●  Record" })).color(red)).frame_when_inactive(false).corner_radius(17).min_size(Vec2::new(0.0, 34.0))
                    };
                    if ui.add(rec_btn).tip("Record audio while you take notes; later, tap any note to hear what was said then").clicked() { self.st.rec_toggle = true; }
                    // "this matters" during a lecture: a small marked note at this moment, to find in the recording later
                    if self.st.recorder.is_some() && ui.add(egui::Button::new(RichText::new("⚑").size(16.0)).frame_when_inactive(false)).tip("Mark this moment (a note linked to it)").clicked() {
                        let at = self.st.recorder.as_ref().map_or(0.0, |r| now_f() - r.3);
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
        let due = self.st.due_now;
        let study = if due > 0 { tf!("Study · {due}", due = due) } else { tr("Study").to_string() };
        [tr("Home").into(), tr("Insert").into(), tr("Draw").into(), tr("AI").into(), study, tr("View").into()]
    }

    /// The width the tab pill takes with `pad` around each name.
    fn tabs_width(&self, ui: &egui::Ui, pad: f32) -> f32 {
        self.tab_names().iter().map(|n| ui.painter().layout_no_wrap(n.clone(), FontId::proportional(14.0), Color32::WHITE).size().x + pad).sum::<f32>() + 8.0
    }

    /// The ribbon tabs as a segmented pill; clicking the open tab folds the toolbar away.
    fn tabs(&mut self, ui: &mut egui::Ui) {
        let v = ui.visuals().clone();
        let acc = accent(ui);
        let names = [Tab::Home, Tab::Insert, Tab::Draw, Tab::Ai, Tab::Study, Tab::View].into_iter().zip(self.tab_names()).collect::<Vec<_>>();
        let font = FontId::proportional(14.0);
        let pad = if ui.available_width() < 440.0 || self.st.short { 16.0 } else { 30.0 };
        let widths: Vec<f32> = self.tab_names().iter().map(|n| ui.painter().layout_no_wrap(n.to_string(), font.clone(), Color32::WHITE).size().x + pad).collect();
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
            // only the open tab has something to say; an empty tooltip shows as a blank bubble
            let resp = if sel && open { resp.tip("Click again to fold the toolbar away") } else { resp };
            if resp.clicked() {
                if sel { self.st.p.ribbon_open ^= true; } else { self.st.tab = *t; self.st.p.ribbon_open = true; }
                self.st.dirty = true;
            }
        }
    }

    pub(crate) fn ribbon(&mut self, ui: &mut egui::Ui) {
        if !self.st.p.ribbon_open || self.st.typing_kb { return; }
        let desk = ui.visuals().panel_fill;
        let touch = self.st.touch_ui;
        egui::Panel::top("ribbon").show_separator_line(false).frame(egui::Frame::new().fill(desk).inner_margin(egui::Margin { left: 14, right: 14, top: 0, bottom: if touch { 6 } else { 12 } })).show(ui, |ui| {
            card(ui).inner_margin(egui::Margin { left: 10, right: 10, top: if touch { 3 } else { 6 }, bottom: if touch { 1 } else { 4 } }).show(ui, |ui| {
                // touch screens: slightly smaller rows and no group captions (the buttons have labels)
                if touch { ui.spacing_mut().interact_size.y = 30.0; }
                ui.set_width(ui.available_width());
                // each tab keeps its own sideways scroll; a solid bar shows when there's more to the right
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                ui.spacing_mut().scroll.bar_width = 4.0;
                ui.horizontal_top(|ui| {
                // every group is two rows tall (see `group`); the scroll bar's room is kept even when it isn't needed
                let (row, gap, bar) = (ui.spacing().interact_size.y, ui.spacing().item_spacing.y, ui.spacing().scroll);
                let groups = (row * 2.0 + 6.0).max(52.0) + if row < 30.0 { 13.0 + gap } else { 0.0 };
                ui.set_min_height(groups + bar.bar_width + bar.bar_inner_margin + bar.bar_outer_margin);
                let fold = 16.0 + 2.0 * ui.spacing().button_padding.x + ui.spacing().item_spacing.x;
                let w = ui.available_width() - fold;
                egui::ScrollArea::horizontal().id_salt(("ribbon-scroll", self.st.tab as u8)).max_width(w).auto_shrink([false, true]).show(ui, |ui| {
                    // the ribbon scrolls sideways: text inside it must never wrap into a tall column
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
                // a visible way to fold the toolbar away (clicking the open tab does the same)
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
            if rbtn(ui, icon, label, self.st.tool == *t).tip(match t { Tool::Type => "Type and select (T)", Tool::Pen => "Pen (P)", Tool::Lasso => "Select ink (L)", Tool::Eraser => "Eraser (E)", _ => label }).clicked() { self.st.tool = *t; }
        }
    }

    fn ribbon_home(&mut self, ui: &mut egui::Ui) {
        group(ui, "Lecture", |ui| {
            if rbtn(ui, "🎓", "New lecture", false).tip("A new page titled with today's date").clicked() { self.start_lecture(false); }
            if rbtn(ui, "●", "Record lecture", false).tip("New lecture page, recording from the start").clicked() { self.start_lecture(true); }
        });
        group(ui, "Tools", |ui| self.tool_buttons(ui, &[(Tool::Type, "Aa", "Type"), (Tool::Pen, "✏", "Pen"), (Tool::Lasso, "➰", "Select ink")]));
        let target = self.st.editing.or(self.st.last_text);
        let is_text = target.is_some_and(|id| self.page().blocks.iter().any(|b| b.id == id && matches!(b.kind, Kind::Text { .. })));
        let heading = target.and_then(|id| self.page().blocks.iter().find(|b| b.id == id)).and_then(|b| if let Kind::Text { heading, .. } = b.kind { Some(heading) } else { None }).unwrap_or(0);
        group(ui, "Text", |ui| {
            ui.add_enabled_ui(is_text, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(2.0, 3.0);
                    ui.horizontal(|ui| {
                        let hl = Color32::from_rgb(255, 236, 140);
                        for (t, tip, f) in [
                            (RichText::new("B").strong(), "Bold (Ctrl+B)", Fmt::Wrap("**")), (RichText::new("I").italics(), "Italic (Ctrl+I)", Fmt::Wrap("*")),
                            (RichText::new("S").strikethrough(), "Strikethrough", Fmt::Wrap("~~")), (RichText::new("H").background_color(hl).color(Color32::BLACK), "Highlight (Ctrl+Shift+H)", Fmt::Wrap("==")),
                            (RichText::new("</>").monospace(), "Code (Ctrl+`)", Fmt::Wrap("`")), (RichText::new("$x$"), "Inline math (Ctrl+M)", Fmt::Wrap("$")),
                        ] { if sbtn(ui, t, tip, false).clicked() { self.st.fmt = Some(f); } }
                    });
                    ui.horizontal(|ui| {
                        for (t, tip, f) in [("•", "Bulleted list", Fmt::Prefix("- ")), ("1.", "Numbered list", Fmt::Prefix("1. ")), ("☐", "Checkbox list", Fmt::Prefix("[ ] ")), ("❝", "Quote", Fmt::Prefix("> "))] {
                            if sbtn(ui, RichText::new(t), tip, false).clicked() { self.st.fmt = Some(f); }
                        }
                        ui.add_space(4.0);
                        for (h, t, tip) in [(0u8, "¶", "Normal text"), (1, "H1", "Heading 1"), (2, "H2", "Heading 2")] {
                            if sbtn(ui, RichText::new(t), tip, is_text && heading == h).clicked() { self.st.fmt = Some(Fmt::Heading(h)); }
                        }
                    });
                });
            });
        });
        let tag = target.and_then(|id| self.page().blocks.iter().find(|b| b.id == id)).map(|b| b.tag);
        group(ui, "Tags", |ui| {
            ui.add_enabled_ui(tag.is_some(), |ui| {
                for (t, icon, label) in [(Tag::Todo, "☐", "To Do"), (Tag::Important, "★", "Important"), (Tag::Question, "?", "Question")] {
                    let on = tag == Some(t) || (t == Tag::Todo && tag == Some(Tag::Done));
                    if rbtn(ui, icon, label, on).clicked() {
                        let page = self.page();
                        let snap = page.clone();
                        if let Some(b) = page.blocks.iter_mut().find(|b| Some(b.id) == target) { b.tag = if on { Tag::None } else { t }; }
                        self.st.snap(&snap);
                    }
                }
            });
            if rbtn(ui, "☑", "All tasks", self.st.tasks).tip("Every open to-do from every notebook, in one list").clicked() { self.st.tasks ^= true; self.st.p.show_nav = true; }
            if rbtn(ui, "🔎", "Find tags", self.st.tag_filter.is_some()).tip("See every tagged note in all notebooks").clicked() {
                self.st.tag_filter = if self.st.tag_filter.is_some() { None } else { Some(Tag::None) };
                self.st.p.show_nav = true;
            }
        });
    }

    fn ribbon_insert(&mut self, ui: &mut egui::Ui) {
        group(ui, "Notes", |ui| {
            if rbtn(ui, "¶", "Text", false).clicked() { self.insert(Kind::Text { text: String::new(), heading: 0 }); }
            if rbtn(ui, "☑", "Checklist", false).clicked() { self.insert(Kind::Checklist { items: vec![(false, String::new())] }); }
            if rbtn(ui, "▦", "Table", false).clicked() { self.insert(Kind::Table { rows: vec![vec![String::new(); 3]; 3] }); }
            if rbtn(ui, "{ }", "Code", false).clicked() { self.insert(Kind::Code { lang: "text".into(), code: String::new() }); }
        });
        group(ui, "Math", |ui| {
            if rbtn(ui, "∑", "Equation", false).clicked() { self.insert(Kind::Math { src: String::new() }); }
            if rbtn(ui, "[⋮]", "Matrix", false).clicked() { self.insert(Kind::Math { src: r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}".into() }); }
            if rbtn(ui, "{≡", "Cases", false).clicked() { self.insert(Kind::Math { src: r"f(x) = \begin{cases} 1 & x \ge 0 \\ 0 & x < 0 \end{cases}".into() }); }
        });
        group(ui, "Media", |ui| {
            if rbtn(ui, "🖼", "Picture", false).clicked() { self.pick_file(Ins::Picture); }
            #[cfg(target_os = "android")]
            if rbtn(ui, "📷", "Photo", false).tip("Take a photo with the camera and put it on the page").clicked() {
                self.st.picking_since = now_f(); // keeps the app awake to collect the photo
                if let Err(e) = crate::droid::take_photo() { self.st.say(tf!("⚠ {e}", e = e)); }
            }
            if rbtn(ui, "📄", "Printout", false).tip("Put a PDF's pages (or a picture, Markdown or text file) onto the page so you can write on it").clicked() { self.pick_file(Ins::Printout); }
            if rbtn(ui, "📎", "Attach file", false).tip("Attach any file as an icon you can open later").clicked() { self.pick_file(Ins::Attach); }
            if rbtn(ui, "🔗", "Link", false).clicked() { self.insert(Kind::Link { url: "https://".into(), title: String::new() }); }
        });
        group(ui, "Stamp", |ui| {
            if rbtn(ui, "🕓", "Date & time", false).clicked() { self.insert(Kind::Text { text: format_date(now_secs(), true), heading: 0 }); }
        });
        group(ui, "Page", |ui| {
            if rbtn(ui, "+", "Blank page", false).tip("New page (Ctrl+N)").clicked() { self.new_page(None); }
            let r = rbtn(ui, "▦", "Page on… ▾", false).tip("A new page on the background you pick");
            egui::Popup::menu(&r).show(|ui| {
                for (p, icon, name) in PAPERS { if ui.button(tf!("{icon}  {name}", icon = icon, name = tr(name))).clicked() { self.new_page(Some(p)); ui.close(); } }
                ui.separator();
                for (i, (name, ..)) in crate::TEMPLATES.iter().enumerate() { if ui.button(format!("▦  {}", tr(name))).clicked() { self.new_page_template(i); ui.close(); } }
            });
        });
    }

    fn ribbon_draw(&mut self, ui: &mut egui::Ui) {
        group(ui, "Tools", |ui| {
            self.tool_buttons(ui, &[(Tool::Type, "Aa", "Type"), (Tool::Lasso, "➰", "Select ink")]);
            // (its size and whether it erases parts or whole strokes are in Eraser settings, shown while it's picked)
            if rbtn(ui, "⌫", "Eraser", self.st.tool == Tool::Eraser).tip("Eraser (E)").clicked() { self.st.tool = Tool::Eraser; }
        });
        group(ui, "My pens", |ui| {
            let n = self.st.p.pens.len();
            let mut remove = None;
            for i in 0..n {
                let pen = self.st.p.pens[i];
                let sel = self.st.tool == Tool::Pen && self.st.p.pen == i;
                let r = pen_icon(ui, c4(pen.color), sel, Some(pen.kind)).tip(tf!("{name} · size {width}\nChange it under Pen settings. Right-click to remove", name = tr(pen.kind.name()), width = format!("{:.1}", pen.width)));
                if r.clicked() { self.st.tool = Tool::Pen; self.st.p.pen = i; self.st.dirty = true; }
                r.context_menu(|ui| { if n > 1 && ui.button(tr("Remove this pen")).clicked() { remove = Some(i); ui.close(); } });
            }
            if let Some(i) = remove {
                self.st.p.pens.remove(i);
                if self.st.p.pen >= i && self.st.p.pen > 0 { self.st.p.pen -= 1; }
                self.st.dirty = true;
            }
            if ui.add(egui::Button::new(RichText::new("+").size(18.0)).min_size(Vec2::new(26.0, 50.0)).frame_when_inactive(false)).tip("Add a pen (starts as a copy of the chosen one)").clicked() {
                let p = *self.st.pen();
                self.st.p.pens.push(p);
                self.st.p.pen = self.st.p.pens.len() - 1;
                self.st.tool = Tool::Pen;
                self.st.dirty = true;
            }
        });
        group(ui, "Highlighter", |ui| {
            for c in [[253, 224, 71, 120], [134, 239, 172, 120], [249, 168, 212, 120], [125, 211, 252, 120]] {
                let sel = self.st.tool == Tool::Highlighter && self.st.p.hl_color == c;
                if pen_icon(ui, c4([c[0], c[1], c[2], 255]), sel, None).tip("Highlighter (H): see-through colour over your notes").clicked() { self.st.tool = Tool::Highlighter; self.st.p.hl_color = c; self.st.dirty = true; }
            }
        });
        // The settings of whatever is in hand: the eraser, the highlighter, or the chosen pen (also used by shapes).
        let before = (self.st.p.erase_partial, self.st.p.eraser_size, self.st.p.hl_width, self.st.p.hl_color);
        if self.st.tool == Tool::Eraser {
            group(ui, "Eraser settings", |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let start = ui.cursor().left();
                    let opts = [("Part of a stroke", "Erases only where you rub, like a real eraser"), ("Whole stroke", "Erases every stroke it touches, in one go")];
                    if let Some(i) = segmented(ui, &opts, usize::from(!self.st.p.erase_partial)) { self.st.p.erase_partial = i == 0; }
                    let w = ui.min_rect().right() - start;
                    labelled_slider(ui, "Size", w, &mut self.st.p.eraser_size, 3.0..=40.0, "How big the eraser is");
                });
                // the eraser at its real size
                ui.add_space(4.0);
                let (r, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
                ui.painter().rect_filled(r, 8.0, ui.visuals().faint_bg_color);
                ui.painter().circle_stroke(r.center(), self.st.p.eraser_size.min(24.0), EStroke::new(1.2, ui.visuals().text_color()));
            });
        } else if self.st.tool == Tool::Highlighter {
            group(ui, "Highlighter settings", |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr("Colour")).size(11.5).weak());
                        ui.spacing_mut().interact_size = Vec2::new(32.0, 24.0);
                        let mut c = self.st.p.hl_color;
                        if ui.color_edit_button_srgba_unmultiplied(&mut c).tip("Any colour for the highlighter").changed() { self.st.p.hl_color = [c[0], c[1], c[2], c[3].min(160)]; }
                        for c in [[253, 224, 71, 120], [134, 239, 172, 120], [249, 168, 212, 120], [125, 211, 252, 120], [253, 186, 116, 120], [196, 181, 253, 120]] {
                            let cur = self.st.p.hl_color;
                            if swatch(ui, c, cur == c).clicked() { self.st.p.hl_color = c; }
                        }
                    });
                    labelled_slider(ui, "Size", 150.0, &mut self.st.p.hl_width, 6.0..=40.0, "How thick the highlighter is");
                });
                let (r, _) = ui.allocate_exact_size(Vec2::new(70.0, 52.0), Sense::hover());
                ui.painter().rect_filled(r, 8.0, ui.visuals().faint_bg_color);
                ui.painter().line_segment([r.left_center() + Vec2::new(10.0, 0.0), r.right_center() - Vec2::new(10.0, 0.0)], EStroke::new(self.st.p.hl_width.min(40.0), c4(self.st.p.hl_color)));
            });
        } else {
        group(ui, "Pen settings", |ui| {
            let before = *self.st.pen();
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                // the touch padding would make the first row taller than the toolbar's two rows
                ui.spacing_mut().button_padding.y = ui.spacing().button_padding.y.min(5.0);
                ui.horizontal(|ui| {
                    let kind = self.st.pen().kind;
                    egui::ComboBox::from_id_salt("pen-kind").width(118.0).selected_text(tr(kind.name())).show_ui(ui, |ui| {
                        for k in PenKind::ALL { if ui.selectable_label(kind == k, tr(k.name())).clicked() { self.st.pen().kind = k; } }
                    }).response.tip("What the chosen pen writes like");
                    ui.spacing_mut().interact_size = Vec2::new(32.0, 24.0);
                    let pen = self.st.pen();
                    let mut c = pen.color;
                    if ui.color_edit_button_srgba_unmultiplied(&mut c).tip("Colour and see-through-ness").changed() { pen.color = c; remember_color(&mut self.st.p.recent_colors, c); }
                    // a few colours a tap away (and the last ones picked), instead of the colour picker every time
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let recent = self.st.p.recent_colors.clone();
                    for c in PEN_COLORS.iter().copied().chain(recent) {
                        let cur = self.st.pen().color;
                        if swatch(ui, c, cur == c).clicked() { self.st.pen().color = c; }
                    }
                    // removing a pen shouldn't need a right-click (or a long-press) to find
                    if self.st.p.pens.len() > 1 && ui.add(egui::Button::new("🗑").frame_when_inactive(false)).tip("Remove this pen from My pens").clicked() {
                        let i = self.st.p.pen;
                        self.st.p.pens.remove(i);
                        self.st.p.pen = i.min(self.st.p.pens.len() - 1);
                        self.st.dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    let tip = match self.st.force { Some(f) => tf!("Pen thickness. Pen pressure now: {pct}%", pct = format!("{:.0}", f * 100.0)), None => "Pen thickness. Pressure comes from your pen, or from speed with a mouse".into() };
                    let l = ui.label(RichText::new(tr("Size")).size(11.5).weak());
                    ui.spacing_mut().slider_width = 70.0;
                    ui.add(egui::Slider::new(&mut self.st.pen().width, 0.5..=16.0).show_value(false)).labelled_by(l.id).tip(tip);
                    let l = ui.label(RichText::new(tr("Smooth")).size(11.5).weak());
                    ui.spacing_mut().slider_width = 50.0;
                    ui.add(egui::Slider::new(&mut self.st.p.stabilizer, 0.0..=1.0).show_value(false)).labelled_by(l.id).tip("Smoothing: irons out shaky lines (see the sample). More smoothing makes the line trail the pen a little.");
                    // only once a pen has reported pressure; in this row, so the toolbar doesn't grow when it does
                    if self.st.p.pressure && self.st.force.is_some() {
                        let l = ui.label(RichText::new(tr("Pressure")).size(11.5).weak());
                        ui.spacing_mut().slider_width = 60.0;
                        ui.add(egui::Slider::new(&mut self.st.p.pressure_curve, 0.4..=2.5).show_value(false)).labelled_by(l.id).tip("How hard you need to press: left makes light touches bolder, right needs a firmer hand");
                    }
                });
            });
            // a deliberately shaky line, run through the same smoothing as real ink
            let (stab, pen) = (self.st.p.stabilizer, *self.st.pen());
            let mut pts: Vec<[f32; 3]> = vec![];
            for i in 0..60 {
                let t = i as f32 / 59.0;
                let (x, y) = (t * 84.0, 18.0 - 9.0 * (t * 6.3).sin() + 1.8 * (i as f32 * 2.7).sin() + 1.2 * (i as f32 * 5.3).cos());
                let pr = 0.35 + 0.65 * (t * 3.1).sin().abs();
                let (x, y) = pts.last().map_or((x, y), |l| crate::canvas::trail(*l, x, y, stab));
                pts.push([x, y, pr]);
            }
            crate::canvas::smooth_passes(&mut pts, stab);
            let sample = Stroke { id: 0, points: pts, color: pen.color, width: pen.width.min(6.0), highlighter: false, pen: pen.kind, t: 0.0 };
            let (r, _) = ui.allocate_exact_size(Vec2::new(96.0, 40.0), Sense::hover());
            // the sample sits on paper-white even in dark mode, so dark inks stay visible
            ui.painter().rect_filled(r, 8.0, if ui.visuals().dark_mode { Color32::from_rgb(200, 207, 204) } else { ui.visuals().faint_bg_color });
            crate::canvas::paint_stroke(&ui.painter_at(r), &sample, r.min + Vec2::new(6.0, 2.0));
            if *self.st.pen() != before { self.st.tool = Tool::Pen; self.st.dirty = true; }
        });
        }
        if (self.st.p.erase_partial, self.st.p.eraser_size, self.st.p.hl_width, self.st.p.hl_color) != before { self.st.dirty = true; }
        group(ui, "Shapes", |ui| {
            for (s, icon, label) in [(Shape::Line, "╱", "Line"), (Shape::Arrow, "↗", "Arrow"), (Shape::Rect, "▭", "Box"), (Shape::Ellipse, "◯", "Circle")] {
                if rbtn(ui, icon, label, self.st.tool == Tool::Shape(s)).clicked() { self.st.tool = Tool::Shape(s); }
            }
        });
        let any = !self.st.selected.is_empty();
        if any { group(ui, "Selection", |ui| {
            ui.add_enabled_ui(any, |ui| {
                if rbtn(ui, "🎨", "Recolour", false).tip("Give the selected ink the current pen colour").clicked() {
                    let (sel, col) = (self.st.selected.clone(), self.st.pen().color);
                    let page = self.page();
                    let snap = page.clone();
                    for s in page.strokes.iter_mut().filter(|s| sel.contains(&s.id) && !s.highlighter) { s.color = col; }
                    self.st.snap(&snap);
                }
                if rbtn(ui, "⧉", "Duplicate", false).clicked() {
                    let sel = self.st.selected.clone();
                    let page = self.page();
                    let snap = page.clone();
                    let copies: Vec<Stroke> = page.strokes.iter().filter(|s| sel.contains(&s.id)).map(|s| Stroke { id: new_id(), points: s.points.iter().map(|p| [p[0] + 24.0, p[1] + 24.0, p[2]]).collect(), ..s.clone() }).collect();
                    let ids = copies.iter().map(|s| s.id).collect();
                    page.strokes.extend(copies);
                    self.st.snap(&snap);
                    self.st.selected = ids;
                }
                if rbtn(ui, "⎘", "Copy", false).tip("Copy the selected ink (Ctrl+C); paste with Ctrl+V, also on another page").clicked() { self.copy_ink(false); }
                if rbtn(ui, "🗑", "Delete", false).clicked() { self.delete_selected_ink(); }
                // recolour the selection with one tap (highlights get the highlighter colours)
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for c in PEN_COLORS {
                            if swatch(ui, c, false).clicked() {
                                let sel = self.st.selected.clone();
                                let page = self.page();
                                let snap = page.clone();
                                for s in page.strokes.iter_mut().filter(|s| sel.contains(&s.id)) { s.color = if s.highlighter { [c[0], c[1], c[2], s.color[3]] } else { c }; }
                                self.st.snap(&snap);
                            }
                        }
                    });
                });
            });
        }); }
        if !self.st.ink_clip.is_empty() && !any && self.st.tool == Tool::Lasso { group(ui, "Selection", |ui| { if rbtn(ui, "⎗", "Paste", false).tip("Paste the copied ink (Ctrl+V)").clicked() { self.paste_ink(); } }); }
        group(ui, "Convert", |ui| {
            if rbtn(ui, "✎→A", "Ink to text", false).tip("Turn handwriting (selected, or all on the page) into typed text — works offline").clicked() { self.ink_convert(false); }
            if rbtn(ui, "✎→∑", "Ink to math", false).tip("Turn handwritten math into a LaTeX equation — works offline").clicked() { self.ink_convert(true); }
        });
    }

    fn ribbon_ai(&mut self, ui: &mut egui::Ui) {
        group(ui, "This page", |ui| {
            if rbtn(ui, "✦", "Summarize", false).clicked() { self.chat("Summarize this page in a few bullet points.".into()); }
            if rbtn(ui, "▤", "Flashcards", false).tip("Write flashcards from this page into it").clicked() { self.ai_cards(); }
            if rbtn(ui, "☑", "Action items", false).clicked() { self.chat("List the action items from this page as a checklist.".into()); }
            if rbtn(ui, "?", "Quiz me", false).clicked() { self.chat("Write 3 short quiz questions about this page. Put the answers at the end.".into()); }
            if rbtn(ui, "💡", "Explain", false).clicked() { self.chat("Explain the main idea of this page simply, with one example.".into()); }
        });
        let has_text = self.st.last_text.is_some();
        group(ui, "Writing", |ui| {
            ui.add_enabled_ui(has_text, |ui| {
                if rbtn(ui, "✓", "Fix spelling", false).tip("Fix spelling and grammar in the note you last typed in").clicked() { self.rewrite(Rewrite::Replace, "Fix spelling and grammar. Change nothing else."); }
                if rbtn(ui, "✂", "Shorter", false).clicked() { self.rewrite(Rewrite::Replace, "Make this more concise without losing information."); }
                if rbtn(ui, "➜", "Continue", false).clicked() { self.rewrite(Rewrite::Append, "Continue this note with one or two short paragraphs in the same style. Reply with only the new text."); }
            });
        });
        group(ui, "Equation from words", |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0; // two rows that fit the toolbar's height, also on touch screens
                let r = ui.add(egui::TextEdit::singleline(&mut self.st.eq_prompt).hint_text(tr("e.g. area of a circle")).desired_width(170.0));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if ui.button(tr("∑ Insert equation")).clicked() || enter { self.equation_from_words(); }
            });
        });
        group(ui, "Handwriting", |ui| {
            if rbtn(ui, "✎→A", "Ink to text", false).clicked() { self.ink_convert(false); }
            if rbtn(ui, "✎→∑", "Ink to math", false).clicked() { self.ink_convert(true); }
        });
        group(ui, "Your AI", |ui| {
            if rbtn(ui, "⚙", "Settings", self.st.show_settings).clicked() { self.st.show_settings ^= true; self.st.settings_tab = 3; }
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
        let due_all = self.st.due_now;
        group(ui, "Today", |ui| {
            if ui.add_enabled_ui(due_all > 0, |ui| rbtn(ui, "⏰", &tf!("Review due ({due_all})", due_all = due_all), due_all > 0)).inner.tip("Cards from all your decks that are due for review").clicked() { self.start_review(); }
        });
        group(ui, "Decks", |ui| {
            if rbtn(ui, "+", "New deck", false).tip("A new page of flashcards in this section").clicked() { self.new_deck("New deck", vec![]); }
            ui.add_enabled_ui(is_deck && n_cards > 0, |ui| {
                if rbtn(ui, "▶", &tf!("Study deck ({n_cards})", n_cards = n_cards), false).tip("Go through this deck's cards").clicked() { self.start_deck(); }
            });
        });
        group(ui, "Make a deck from these notes", |ui| {
            ui.add_enabled_ui(!is_deck, |ui| {
                if rbtn(ui, "▤", "Suggest cards", false).tip("Builds a deck from questions, definitions and highlights on this page — you can edit everything after").clicked() { self.deck_from_notes(); }
                if rbtn(ui, "✦", "With AI", false).tip("Your AI writes a deck from this page (needs your AI key)").clicked() { self.ai_cards(); }
            });
        });
    }

    /// Cards already studied that are due again, from every deck.
    /// Cards due again, and up to 20 never studied (a new deck shows up here too, a bit at a time).
    pub(crate) fn due_cards(&self) -> Vec<Card> {
        let now = now_secs();
        let all = self.store.notebooks.iter().flat_map(|n| n.sections.iter()).flat_map(|s| s.pages.iter()).flat_map(|p| p.deck());
        let (mut due, mut fresh) = (vec![], vec![]);
        for c in all {
            match self.store.study.get(&c.key) { Some(s) if s.due <= now => due.push(c), None if fresh.len() < 20 => fresh.push(c), _ => {} }
        }
        due.extend(fresh);
        due
    }

    pub(crate) fn start_review(&mut self) {
        let cards = self.due_cards();
        if cards.is_empty() { self.st.say("Nothing due today — nice! Cards come back here once you've studied them."); return; }
        let total = cards.len();
        self.st.study = Some(Study { title: tr("Due today").into(), queue: { let mut q: Vec<usize> = (0..total).collect(); shuffle(&mut q); q }, cards, total, shown: false, tally: [0; 3], missed: vec![], undo: None });
    }

    /// Study the open deck: cards that are due or new first, then the rest.
    pub(crate) fn start_deck(&mut self) {
        let p = &self.store.notebooks[self.nb].sections[self.sec].pages[self.pg];
        let cards = p.deck();
        if cards.is_empty() { self.st.say("Add a card with both a front and a back first."); return; }
        let title = if p.title.is_empty() { tr("Untitled deck").into() } else { p.title.clone() };
        let now = now_secs();
        let (mut due, mut later): (Vec<usize>, Vec<usize>) = (0..cards.len()).partition(|&i| self.store.study.get(&cards[i].key).is_none_or(|s| s.due <= now));
        shuffle(&mut due);
        shuffle(&mut later);
        due.append(&mut later);
        let total = due.len();
        self.st.study = Some(Study { title, cards, queue: due, total, shown: false, tally: [0; 3], missed: vec![], undo: None });
    }

    /// First use of Record: say what the microphone is for, and ask.
    fn mic_window(&mut self, ctx: &egui::Context) {
        let (mut allow, mut no) = (false, false);
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(24)).corner_radius(18);
        let modal = egui::Modal::new(egui::Id::unique("mic")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 440.0));
            // a microphone in a soft circle
            let (r, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
            let (c, acc) = (r.center(), accent(ui));
            ui.painter().circle_filled(c, 28.0, ui.visuals().selection.bg_fill);
            ui.painter().rect_filled(Rect::from_center_size(c - Vec2::new(0.0, 5.0), Vec2::new(13.0, 22.0)), 6.5, acc);
            let arc: Vec<Pos2> = (0..=16).map(|i| { let t = std::f32::consts::PI * i as f32 / 16.0; c + Vec2::new(-11.0 * t.cos(), -2.0 + 11.0 * t.sin()) }).collect();
            ui.painter().add(egui::Shape::line(arc, EStroke::new(2.2, acc)));
            ui.painter().line_segment([c + Vec2::new(0.0, 9.0), c + Vec2::new(0.0, 15.0)], EStroke::new(2.2, acc));
            ui.painter().line_segment([c + Vec2::new(-6.0, 15.0), c + Vec2::new(6.0, 15.0)], EStroke::new(2.2, acc));
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Allow the microphone?")).font(rich::serif(23.0)).color(ui.visuals().strong_text_color()));
            ui.add_space(4.0);
            ui.label(tr("Leaf Notes records only while the red Record button is on. Each recording is saved with its page, on this device only, and is linked to the notes you write so you can tap a note to hear that moment again."));
            #[cfg(target_os = "android")]
            ui.label(RichText::new(tr("Android will ask you to confirm next.")).weak());
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new(tr("Allow and record")).color(Color32::WHITE)).fill(accent_base()).min_size(Vec2::new(150.0, 34.0))).clicked() { allow = true; }
                if ui.add(egui::Button::new(tr("Not now")).min_size(Vec2::new(100.0, 34.0))).clicked() { no = true; }
            });
        });
        if allow { self.st.mic_prompt = false; self.st.p.mic_ok = true; self.st.dirty = true; self.st.rec_toggle = true; }
        if no || modal.should_close() { self.st.mic_prompt = false; }
    }

    fn study_window(&mut self, ctx: &egui::Context) {
        let Some(mut sd) = self.st.study.take() else { return };
        let mut close = false;
        let mut rate: Option<usize> = None;
        let (space, k1, k2, k3) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Space), i.consume_key(Modifiers::NONE, Key::Num1), i.consume_key(Modifiers::NONE, Key::Num2), i.consume_key(Modifiers::NONE, Key::Num3)));
        let (enter, back) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Backspace) || i.consume_key(Modifiers::COMMAND, Key::Z)));
        // Enter: show the answer, then "Good" (one key all the way through)
        if (space || enter) && !sd.shown { sd.shown = true; } else if enter && sd.shown { rate = Some(1); }
        if sd.shown { if k1 { rate = Some(0) } else if k2 { rate = Some(1) } else if k3 { rate = Some(2) } }
        // Backspace or Ctrl+Z: take the last rating back
        if back {
            if let Some(u) = sd.undo.take() {
                match u.before { Some(b) => { self.store.study.insert(u.key, b); } None => { self.store.study.remove(&u.key); } }
                sd.queue = u.queue;
                sd.missed = u.missed;
                sd.tally[u.rating] = sd.tally[u.rating].saturating_sub(1);
                sd.shown = true;
                rate = None;
                self.st.dirty = true;
            }
        }
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
        let modal = egui::Modal::new(egui::Id::unique("study")).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 600.0));
            let acc = accent(ui);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Study")).font(rich::serif(24.0)).color(ui.visuals().strong_text_color()));
                ui.label(RichText::new(format!("·  {}", sd.title)).size(14.0).weak());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| { if ui.add(egui::Button::new(RichText::new("✕").size(15.0)).frame_when_inactive(false)).clicked() { close = true; } });
            });
            let done = sd.total - sd.queue.len();
            let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
            ui.painter().rect_filled(bar, 3.0, ui.visuals().widgets.inactive.weak_bg_fill);
            ui.painter().rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * done as f32 / sd.total.max(1) as f32, 6.0)), 3.0, acc);
            ui.add_space(12.0);
            let colors = rich::Colors::from(ui, acc);
            match sd.queue.first().copied() {
                None => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(20.0);
                        ui.label(RichText::new(tr("All done 🎉")).font(rich::serif(26.0)));
                        ui.label(RichText::new(tf!("{total} cards · {x} again · {x2} good · {x3} easy", total = sd.total, x = sd.tally[0], x2 = sd.tally[1], x3 = sd.tally[2])).weak());
                        // when these cards come back
                        let next = sd.cards.iter().filter_map(|c| self.store.study.get(&c.key)).map(|s| s.due).filter(|d| *d > now_secs()).min();
                        if let Some(d) = next { ui.label(RichText::new(tf!("Next review: {when}", when = format_date(d, false))).weak()); }
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            ui.add_space((ui.available_width() - 220.0).max(0.0) / 2.0);
                            // again: only the cards that were missed (rating all of them once more would push every card's next review out)
                            if !sd.missed.is_empty() && ui.add(egui::Button::new(RichText::new(tf!("Practise the {n} missed", n = sd.missed.len())).color(Color32::WHITE)).fill(accent_base()).min_size(Vec2::new(110.0, 32.0))).clicked() {
                                sd.queue = std::mem::take(&mut sd.missed); sd.total = sd.queue.len(); sd.tally = [0; 3]; sd.shown = false;
                            }
                            if ui.add(egui::Button::new(tr("Close")).min_size(Vec2::new(100.0, 32.0))).clicked() { close = true; }
                        });
                        ui.add_space(10.0);
                    });
                }
                Some(i) => {
                    let card = sd.cards[i].clone();
                    ui.label(RichText::new(tf!("Card {x} of {total}", x = done + 1, total = sd.total)).size(11.5).weak());
                    egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(16).inner_margin(egui::Margin::same(24)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_min_height(170.0);
                        let _ = rich::view(ui, &card.front, 20.0, 0, &colors);
                        if sd.shown {
                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(10.0);
                            let _ = rich::view(ui, &card.back, 16.0, 0, &colors);
                        }
                    });
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if !sd.shown {
                            if ui.add(egui::Button::new(RichText::new(tr(if self.st.touch_ui { "Show answer" } else { "Show answer   Space" })).color(Color32::WHITE)).fill(accent_base()).min_size(Vec2::new(ui.available_width(), 38.0))).clicked() { sd.shown = true; }
                        } else {
                            let w = (ui.available_width() - 16.0) / 3.0;
                            for (k, (label, col)) in { let t = self.st.touch_ui; [(tr(if t { "Again" } else { "Again   1" }), Color32::from_rgb(214, 82, 62)), (tr(if t { "Good" } else { "Good   2" }), accent_base()), (tr(if t { "Easy" } else { "Easy   3" }), Color32::from_rgb(21, 128, 61))] }.into_iter().enumerate() {
                                if ui.add(egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(col).min_size(Vec2::new(w, 38.0))).clicked() { rate = Some(k); }
                            }
                        }
                    });
                }
            }
        });
        if let (Some(k), Some(i)) = (rate, sd.queue.first().copied()) {
            let key = sd.cards[i].key.clone();
            sd.undo = Some(StudyUndo { key: key.clone(), before: self.store.study.get(&key).copied(), rating: k, queue: sd.queue.clone(), missed: sd.missed.clone() });
            let state = self.store.study.entry(key).or_default();
            sd.queue.remove(0);
            match k {
                // a slip on a well-known card costs two boxes, not all of them; it's due again in ten minutes
                0 => { state.bx = state.bx.saturating_sub(2); state.due = now_secs() + 600; let at = sd.queue.len().min(3); sd.queue.insert(at, i); if !sd.missed.contains(&i) { sd.missed.push(i); } }
                n => { state.bx = (state.bx + n as u8).min(5); state.due = now_secs() + STUDY_DAYS[state.bx as usize] * 86_400; }
            }
            sd.tally[k] += 1;
            sd.shown = false;
            self.st.dirty = true;
        }
        if !close && !modal.should_close() { self.st.study = Some(sd); }
    }

    fn ribbon_view(&mut self, ui: &mut egui::Ui) {
        let paper = self.page().paper;
        let mut set = None;
        group(ui, "Paper", |ui| {
            for (p, icon, label) in PAPERS {
                if rbtn(ui, icon, label, paper == p).clicked() { set = Some(p); }
            }
        });
        if let Some(p) = set { self.page().paper = p; self.st.dirty = true; }
        group(ui, "Page zoom", |ui| {
            if rbtn(ui, "−", "Zoom out", false).tip("Or pinch / Ctrl+scroll on the page").clicked() { self.st.zoom_req = Some(ZoomReq::By(1.0 / 1.25)); }
            if rbtn(ui, &format!("{:.0}%", self.st.zoom_now * 100.0), "Actual size", false).clicked() { self.st.zoom_req = Some(ZoomReq::To(1.0)); }
            if rbtn(ui, "+", "Zoom in", false).clicked() { self.st.zoom_req = Some(ZoomReq::By(1.25)); }
            if rbtn(ui, "⇔", "Fit width", false).clicked() { self.st.zoom_req = Some(ZoomReq::FitWidth); }
        });
        let ctx = ui.ctx().clone();
        group(ui, "Interface", |ui| {
            let z = ctx.zoom_factor();
            let mut set = |z: f32| { let z = (z * 10.0).round() / 10.0; ctx.set_zoom_factor(z.clamp(0.6, 2.5)); self.st.p.zoom = z.clamp(0.6, 2.5); self.st.dirty = true; };
            if rbtn(ui, "−", "Smaller", false).clicked() { set(z - 0.1); }
            if rbtn(ui, &format!("{:.0}%", z * 100.0), "Reset", false).clicked() { set(1.0); }
            if rbtn(ui, "+", "Larger", false).clicked() { set(z + 0.1); }
        });
        group(ui, "Look", |ui| {
            let dark = self.st.p.dark;
            // (an explicit choice: stop following the system, or the system's theme would switch it straight back)
            if rbtn(ui, if dark { "☀" } else { "☾" }, if dark { "Light mode" } else { "Dark mode" }, false).clicked() { self.st.p.dark ^= true; self.st.p.follow_system = false; apply_theme(ui.ctx(), self.st.p.dark); self.st.dirty = true; }
            let focus = !self.st.p.show_nav && !self.st.p.show_ai;
            if rbtn(ui, "⛶", "Focus", focus).tip("Hide the side panels to concentrate on the page").clicked() {
                self.st.p.show_nav = focus;
                self.st.p.show_ai = focus;
                self.st.dirty = true;
            }
            if rbtn(ui, "☰", "Notebooks", self.st.p.show_nav).clicked() { self.st.p.show_nav ^= true; self.st.dirty = true; }
            if rbtn(ui, "✦", "Assistant", self.st.p.show_ai).clicked() { self.st.p.show_ai ^= true; self.st.dirty = true; }
        });
        group(ui, "Help", |ui| {
            if rbtn(ui, "⌨", "Shortcuts", self.st.show_shortcuts).clicked() { self.st.show_shortcuts ^= true; }
            if rbtn(ui, "🗑", &tf!("Deleted ({trash_n})", trash_n = self.store.trash.len()), self.st.show_trash).clicked() { self.st.show_trash ^= true; }
        });
    }

    /// A study timer: 25 minutes of focus, then a 5-minute break.
    fn focus_timer(&mut self, ui: &mut egui::Ui) {
        let now = now_f();
        if let Some((end, brk)) = self.st.focus {
            if now >= end {
                if brk { self.st.focus = None; self.st.say("Break's over — start another focus round when you're ready."); }
                else { self.st.focus = Some((now + 5.0 * 60.0, true)); self.st.say("Nice work! Take a 5-minute break."); }
                return;
            }
            let left = (end - now) as u64;
            let label = format!("{}  {:02}:{:02}  ■", tr(if brk { "☕ Break" } else { "⏱ Focus" }), left / 60, left % 60);
            if ui.add(egui::Button::new(RichText::new(label).size(11.5).color(accent(ui))).frame_when_inactive(false)).tip("Stop the timer").clicked() { self.st.focus = None; }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        } else if ui.add(egui::Button::new(RichText::new(tr("⏱ Focus 25")).size(11.5)).frame_when_inactive(false)).tip("25 minutes of focused study, then a 5-minute break").clicked() {
            self.st.focus = Some((now + 25.0 * 60.0, false));
            self.st.say("Focus time: 25 minutes. You've got this.");
        }
    }

    /// Every open to-do in every notebook: checklist items and "[ ]" lines in text notes.
    /// (page place, page title, block id, line/item index, text, done)
    pub(crate) fn open_tasks(&self) -> Vec<((usize, usize, usize), String, u64, usize, String, bool)> {
        let mut out = vec![];
        for (a, n) in self.store.notebooks.iter().enumerate() { for (b, s) in n.sections.iter().enumerate() { for (c, p) in s.pages.iter().enumerate() {
            let title = if p.title.trim().is_empty() { tr("Untitled page").to_string() } else { p.title.clone() };
            for bl in &p.blocks {
                match &bl.kind {
                    Kind::Checklist { items } => for (i, (d, t)) in items.iter().enumerate() { if !t.trim().is_empty() && !d { out.push(((a, b, c), title.clone(), bl.id, i, t.clone(), *d)); } },
                    Kind::Text { text, .. } => for (i, l) in text.lines().enumerate() {
                        if let (rich::LineKind::Check(false), plen) = rich::classify(l) { if !l[plen..].trim().is_empty() { out.push(((a, b, c), title.clone(), bl.id, i, l[plen..].to_string(), false)); } }
                    },
                    _ => {}
                }
                // a whole note tagged To Do is a task too (index MAX: tick it by tagging it Done)
                if bl.tag == Tag::Todo { out.push(((a, b, c), title.clone(), bl.id, usize::MAX, bl.kind.summary().lines().next().unwrap_or("").to_string(), false)); }
            }
        }}}
        out
    }

    fn tasks_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("All tasks")).font(rich::serif(18.0)).color(ui.visuals().strong_text_color()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| { if ui.add(egui::Button::new("✕").frame_when_inactive(false)).tip("Back to pages").clicked() { self.st.tasks = false; } });
        });
        ui.label(RichText::new(tr("Open to-dos from every notebook. Tick them off here.")).size(11.5).weak());
        ui.separator();
        let mut tasks = self.open_tasks();
        // tasks with a date ("@2026-11-20") first, soonest first
        tasks.sort_by_key(|t| due_day(&t.4).unwrap_or(i64::MAX));
        let today = (now_secs() as i64 + local_offset(now_secs())).div_euclid(86_400);
        let mut tick = None;
        let mut go = None;
        egui::ScrollArea::vertical().id_salt("tasks-scroll").auto_shrink([false; 2]).show(ui, |ui| {
            if tasks.is_empty() { ui.add_space(20.0); ui.vertical_centered(|ui| ui.label(RichText::new(tr("Nothing to do 🎉\nAdd a checklist, or start a line with [ ].")).weak())); }
            let mut last_page = None;
            for (at, title, bid, idx, text, _) in &tasks {
                if last_page != Some(*at) {
                    ui.add_space(6.0);
                    if ui.add(egui::Label::new(RichText::new(title).size(12.0).color(accent(ui))).sense(Sense::click())).tip("Open this page").clicked() { go = Some(*at); }
                    last_page = Some(*at);
                }
                ui.horizontal(|ui| {
                    let mut d = false;
                    if ui.checkbox(&mut d, "").changed() { tick = Some((*at, *bid, *idx)); }
                    if let Some(day) = due_day(text) {
                        let (label, col) = if day < today { (tr("overdue"), Color32::from_rgb(214, 70, 60)) } else if day == today { (tr("today"), accent(ui)) } else { ("", ui.visuals().weak_text_color()) };
                        if !label.is_empty() { ui.label(RichText::new(label).size(11.0).color(col)); }
                    }
                    if ui.add(egui::Label::new(RichText::new(text).size(13.5)).wrap().sense(Sense::click())).tip("Open this page").clicked() { go = Some(*at); }
                });
            }
        });
        if let Some(((a, b, c), bid, idx)) = tick {
            let page = &mut self.store.notebooks[a].sections[b].pages[c];
            let snap = page.clone();
            if let Some(bl) = page.blocks.iter_mut().find(|bl| bl.id == bid) {
                match &mut bl.kind {
                    _ if idx == usize::MAX => bl.tag = Tag::Done,
                    Kind::Checklist { items } => if let Some(it) = items.get_mut(idx) { it.0 = true; },
                    Kind::Text { text, .. } => {
                        let lines: Vec<String> = text.lines().enumerate().map(|(i, l)| if i == idx { l.replacen("[ ]", "[x]", 1) } else { l.to_string() }).collect();
                        *text = lines.join("\n");
                    }
                    _ => {}
                }
            }
            self.st.snap(&snap);
            self.st.say("Done ✓ (Undo brings it back)");
        }
        if let Some((a, b, c)) = go { self.go_to(a, b, c); self.st.tasks = false; }
    }

    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").show_separator_line(false).frame(egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin { left: 18, right: 18, top: 0, bottom: 3 })).show(ui, |ui| {
            // one thin line of small text, even with touch-sized controls elsewhere
            ui.spacing_mut().interact_size.y = 18.0;
            ui.spacing_mut().button_padding = Vec2::new(6.0, 1.0);
            ui.horizontal(|ui| {
                let page = &self.store.notebooks[self.nb].sections[self.sec].pages[self.pg];
                let words: usize = page.blocks.iter().map(|b| b.kind.summary().split_whitespace().count().max(match &b.kind { Kind::Text { text, .. } => text.split_whitespace().count(), _ => 0 })).sum();
                let info = if page.kind == PageKind::Deck { tf!("{n} cards · {ready} ready to study", n = page.cards.len(), ready = page.deck().len()) } else { [(words, tr("{n} word"), tr("{n} words")), (page.blocks.len(), tr("{n} note"), tr("{n} notes")), (page.strokes.len(), tr("{n} ink stroke"), tr("{n} ink strokes"))]
                    .iter().map(|(n, one, many)| crate::i18n::fill(if *n == 1 { one } else { many }, &[("n", n)])).collect::<Vec<_>>().join(" · ") };
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(tr(if self.st.dirty { "Saving…" } else { "✓ Saved" })).size(11.5).weak());
                    if let Some((v, url, notes)) = self.st.update.clone() {
                        let tip = if notes.is_empty() { tr("Download it").to_string() } else { format!("{notes}\n\n{}", tr("Click to download it. Right-click: not this version.")) };
                        let b = ui.add(egui::Button::new(RichText::new(tf!("⬆ Leaf Notes {v} is available", v = v.trim_start_matches('v'))).size(11.5).color(accent(ui))).frame_when_inactive(false)).tip(tip);
                        if b.clicked() { ui.ctx().open_url(egui::OpenUrl::new_tab(url)); }
                        b.context_menu(|ui| { if ui.button(tr("Don't tell me about this version again")).clicked() { self.st.p.skip_version = v.clone(); self.st.update = None; self.st.dirty = true; ui.close(); } });
                    }
                    if self.st.persist && self.st.sync.on() {
                        let err = !self.st.sync_err.is_empty();
                        let retry = (self.st.sync_at + 40.0 - now_f()).max(0.0);
                        let (t, tip) = if self.st.sync_rx.is_some() { ("🔄 Syncing…".to_string(), "Syncing with your other devices".to_string()) }
                            else if err { ("⚠ Not synced".into(), tf!("{sync_err}\nYour notes are safe on this device. Trying again in about {s} s. Click for sync settings.", sync_err = self.st.sync_err, s = format!("{retry:.0}"))) }
                            // edited since the last sync: say it's on its way
                            else if self.st.sync_pending && self.st.sync_ok > 0 { ("🔄 Changes waiting".into(), tf!("Synced {x}. The latest changes go out shortly; click to sync now.", x = ago(self.st.sync_ok))) }
                            else if self.st.sync_ok > 0 { (tf!("🔄 Synced {x}", x = ago(self.st.sync_ok)), "Click to sync now".into()) }
                            else { ("🔄 Sync".into(), "Click to sync now".into()) };
                        let col = if err { Color32::from_rgb(220, 80, 60) } else { ui.visuals().weak_text_color() };
                        if ui.add(egui::Button::new(RichText::new(tr(&t)).size(11.5).color(col)).frame_when_inactive(false)).tip(tip).clicked() {
                            if err { self.st.show_settings = true; self.st.settings_tab = 5; } else { self.st.sync_now = true; }
                        }
                    }
                    self.focus_timer(ui);
                    // a mistaken delete is one click away from undone (for a few seconds)
                    if self.st.undo_delete.is_some_and(|t| now_f() - t < 10.0) && !self.store.trash.is_empty() {
                        if ui.add(egui::Button::new(RichText::new(tr("↶ Undo delete")).size(11.5).color(accent(ui))).frame_when_inactive(false)).clicked() {
                            self.st.undo_delete = None;
                            let i = self.store.trash.len() - 1;
                            self.restore(i);
                        }
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                    }
                    if (self.st.zoom_now - 1.0).abs() > 0.02 && ui.add(egui::Button::new(RichText::new(format!("{:.0}%", self.st.zoom_now * 100.0)).size(11.5)).frame_when_inactive(false)).tip("Page zoom — click for actual size").clicked() { self.st.zoom_req = Some(ZoomReq::To(1.0)); }
                    if !self.st.ai_rx.is_empty() { ui.label(RichText::new(tr("AI working…")).size(11.5).color(accent(ui))); ui.spinner(); }
                    if self.st.tool != Tool::Type {
                        // the tool in hand, and every other one a tap away (the toolbar may be folded away on a tablet)
                        let name = tr(match self.st.tool { Tool::Pen => "✏ Pen", Tool::Highlighter => "Highlighter", Tool::Eraser => "Eraser", Tool::Lasso => "Select ink", Tool::Shape(_) => "Shape", Tool::Type => "" });
                        ui.menu_button(RichText::new(format!("{name} ▴")).size(11.5).weak(), |ui| {
                            for (t, label) in [(Tool::Type, "Aa  Type"), (Tool::Pen, "✏  Pen"), (Tool::Highlighter, "▮  Highlighter"), (Tool::Eraser, "⌫  Eraser"), (Tool::Lasso, "➰  Select ink"),
                                (Tool::Shape(Shape::Line), "╱  Line"), (Tool::Shape(Shape::Arrow), "→  Arrow"), (Tool::Shape(Shape::Rect), "▭  Box"), (Tool::Shape(Shape::Ellipse), "◯  Circle")] {
                                if ui.selectable_label(self.st.tool == t, tr(label)).clicked() { self.st.tool = t; ui.close(); }
                            }
                        });
                    }
                    // what's left on the left: the page counts, and the latest message, shortened to fit (never under the items on the right)
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let msg = !self.st.status.is_empty() && self.st.status_age < 10.0;
                        if !msg || ui.available_width() > 520.0 { ui.add(egui::Label::new(RichText::new(info).size(11.5).weak()).truncate()); }
                        if msg {
                            ui.add_space(12.0);
                            ui.add(egui::Label::new(RichText::new(&self.st.status).size(11.5).color(if self.st.status.starts_with('⚠') { Color32::from_rgb(220, 80, 60) } else { accent(ui) })).truncate());
                        }
                    });
                });
            });
        });
    }

    pub(crate) fn nav(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_width();
        let phone = full < 560.0;
        let sec_w = if phone { 150.0 } else { 204.0 };
        egui::Panel::left("sections").show_separator_line(false).resizable(false).exact_size(sec_w).frame(egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin { left: 14, right: 10, top: 2, bottom: 12 })).show(ui, |ui| {
            self.notebook_switcher(ui);
            ui.add_space(6.0);
            let jump = ui.add(egui::Button::new(RichText::new(tr("↗  Jump to a page…")).size(13.0).color(ui.visuals().weak_text_color())).frame_when_inactive(false).min_size(Vec2::new(ui.available_width(), 30.0)))
                .tip("Quick switcher (Ctrl+K)");
            if jump.clicked() { self.st.switcher = Some((String::new(), 0)); }
            let open_tasks = self.st.open_tasks_n;
            let t = ui.add(egui::Button::new(RichText::new(tf!("☑  All tasks{x}", x = if open_tasks > 0 { format!("  ·  {open_tasks}") } else { String::new() })).size(13.0).color(if self.st.tasks { accent(ui) } else { ui.visuals().weak_text_color() }))
                .frame_when_inactive(false).selected(self.st.tasks).min_size(Vec2::new(ui.available_width(), 30.0))).tip("Every open to-do from every notebook");
            if t.clicked() { self.st.tasks ^= true; }
            // favourite pages, wherever they live
            let favs: Vec<(u64, String, (usize, usize, usize))> = self.st.p.favorites.iter().filter_map(|id| self.locate(*id).map(|at| {
                let p = &self.store.notebooks[at.0].sections[at.1].pages[at.2];
                (p.id, if p.title.trim().is_empty() { tr("Untitled page").to_string() } else { p.title.clone() }, at)
            })).collect();
            if !favs.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(tr("FAVOURITES")).size(10.5).weak());
                for (id, title, at) in favs {
                    let cur = at == (self.nb, self.sec, self.pg);
                    let r = nav_item(ui, &mut self.st, id, &format!("★  {title}"), None, cur, 0, None);
                    if r.clicked() { self.go_to(at.0, at.1, at.2); }
                    r.context_menu(|ui| { if ui.button(tr("Remove from favourites")).clicked() { self.st.p.favorites.retain(|f| *f != id); self.st.dirty = true; ui.close(); } });
                }
            }
            // pages kept twice by sync (changed on two devices at once): easy to find, not only by stumbling on them
            let conflicts: Vec<(usize, usize, usize)> = self.store.notebooks.iter().enumerate().flat_map(|(a, n)| n.sections.iter().enumerate().flat_map(move |(b, s)| s.pages.iter().enumerate().filter(|(_, p)| p.other_of.is_some()).map(move |(c, _)| (a, b, c)))).collect();
            if let Some(&(a, b, c)) = conflicts.first() {
                if ui.add(egui::Button::new(RichText::new(tf!("⚠  Sync conflicts · {n}", n = conflicts.len())).size(13.0).color(Color32::from_rgb(214, 120, 40))).frame_when_inactive(false).min_size(Vec2::new(ui.available_width(), 30.0)))
                    .tip("Pages changed on two devices at once: open one to compare and keep a version").clicked() { self.go_to(a, b, c); }
            }
            // what you had open lately (not the favourites, not this page)
            let here = self.store.notebooks[self.nb].sections[self.sec].pages.get(self.pg).map(|p| p.id);
            let recent: Vec<(u64, String, (usize, usize, usize))> = self.st.p.recent.iter().filter(|id| Some(**id) != here && !self.st.p.favorites.contains(id)).filter_map(|id| self.locate(*id).map(|at| {
                let p = &self.store.notebooks[at.0].sections[at.1].pages[at.2];
                (p.id, if p.title.trim().is_empty() { tr("Untitled page").to_string() } else { p.title.clone() }, at)
            })).take(4).collect();
            if !recent.is_empty() && !self.st.narrow {
                ui.add_space(6.0);
                ui.label(RichText::new(tr("RECENT")).size(10.5).weak());
                for (id, title, at) in recent {
                    if nav_item(ui, &mut self.st, id, &format!("🕓  {title}"), None, false, 0, None).clicked() { self.go_to(at.0, at.1, at.2); }
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("SECTIONS")).size(10.5).weak());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("+").size(15.0).color(accent(ui))).frame_when_inactive(false)).tip("New section").clicked() { self.add_section(); }
                });
            });
            let nb_count = self.store.notebooks.len();
            let names: Vec<(u64, String)> = self.store.notebooks.iter().map(|n| (n.id, n.name.clone())).collect();
            let nb = &mut self.store.notebooks[self.nb];
            let (mut clicked, mut recolor, mut swap, mut move_to, mut lock_act) = (None, None, None, None, None);
            let mut dup_sec: Option<usize> = None;
            let mut drop_in: Option<(u64, usize, u64)> = None;
            let count = nb.sections.len();
            egui::ScrollArea::vertical().id_salt("sections-scroll").auto_shrink([false, true]).show(ui, |ui| {
                for (i, s) in nb.sections.iter().enumerate() {
                    let r = nav_item(ui, &mut self.st, s.id, &s.name, Some(c3(s.color)), i == self.sec, 0, None);
                    if r.clicked() { clicked = Some(i); }
                    // a page dragged here from the pages column moves into this section
                    if !s.sealed() {
                        if r.dnd_hover_payload::<DragPage>().is_some_and(|x| x.0 != s.id) { ui.painter().rect_stroke(r.rect, 8.0, EStroke::new(2.0, accent(ui)), egui::StrokeKind::Inside); }
                        if let Some(x) = r.dnd_release_payload::<DragPage>() { if x.0 != s.id { drop_in = Some((x.0, x.1, s.id)); } }
                    }
                    r.context_menu(|ui| {
                        if ui.button(tr("Rename")).clicked() { self.st.rename = Some((s.id, s.name.clone())); ui.close(); }
                        ui.horizontal(|ui| for c in PALETTE {
                            let (rect, cr) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                            ui.painter().rect_filled(rect, 4.0, c3(c));
                            if cr.clicked() { recolor = Some((i, c)); ui.close(); }
                        });
                        if i > 0 && ui.button(tr("Move up")).clicked() { swap = Some((i, i - 1)); ui.close(); }
                        if i + 1 < count && ui.button(tr("Move down")).clicked() { swap = Some((i, i + 1)); ui.close(); }
                        if s.lock.is_none() && ui.button(tr("Duplicate section")).clicked() { dup_sec = Some(i); ui.close(); }
                        if nb_count > 1 {
                            ui.menu_button(tr("Move to notebook"), |ui| for (id, name) in &names {
                                if *id != nb.id && ui.button(name).clicked() { move_to = Some((i, *id)); ui.close(); }
                            });
                        }
                        ui.separator();
                        match (&s.lock, self.st.keys.contains_key(&s.id)) {
                            (None, _) => if ui.button(tr("🔒  Lock with a password…")).clicked() { lock_act = Some((s.id, 0)); ui.close(); },
                            (Some(_), true) => {
                                if ui.button(tr("🔒  Lock now")).clicked() { lock_act = Some((s.id, 1)); ui.close(); }
                                if ui.button(tr("Remove lock")).clicked() { lock_act = Some((s.id, 2)); ui.close(); }
                            }
                            (Some(_), false) => {}
                        }
                        if ui.button(RichText::new(tr("Delete section")).color(Color32::from_rgb(200, 60, 50))).clicked() { self.st.rename = None; recolor = None; move_to = None; swap = None; clicked = None; ui.close(); self.st.status = format!("delete:{id}", id = s.id); }
                    });
                }
            });
            if let Some((i, c)) = recolor { nb.sections[i].color = c; self.st.dirty = true; }
            if let Some((from_sec, i, to_sec)) = drop_in {
                let page = self.store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).find(|s| s.id == from_sec).filter(|s| i < s.pages.len()).map(|s| s.pages.remove(i));
                if let Some(p) = page {
                    let title = p.title.clone();
                    if let Some(s) = self.store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).find(|s| s.id == to_sec) { s.pages.push(p); }
                    self.clamp();
                    self.st.dirty = true;
                    self.st.say(tf!("Moved “{title}”.", title = title));
                }
            }
            let nb = &mut self.store.notebooks[self.nb];
            if let Some(i) = dup_sec {
                // a copy with its own ids throughout (study progress of its cards starts fresh)
                let mut s = nb.sections[i].clone();
                s.id = new_id();
                s.name = tf!("{name} (copy)", name = s.name);
                for p in &mut s.pages { p.id = new_id(); for b in &mut p.blocks { b.id = new_id(); } for k in &mut p.strokes { k.id = new_id(); } for c in &mut p.cards { c.id = new_id(); } }
                nb.sections.insert(i + 1, s);
                self.st.dirty = true;
            }
            match lock_act {
                Some((id, 0)) => self.st.lock_prompt = Some(locks::LockPrompt { section: id, ..Default::default() }),
                Some((id, 1)) => { self.relock(id); self.st.say("Section locked."); }
                Some((id, _)) => { self.remove_lock(id); self.st.say("The lock was removed; these pages are ordinary pages again."); }
                None => {}
            }
            let nb = &mut self.store.notebooks[self.nb];
            if let Some((a, b)) = swap { nb.sections.swap(a, b); if self.sec == a { self.sec = b } else if self.sec == b { self.sec = a } self.st.dirty = true; }
            if let Some((i, to)) = move_to {
                let s = nb.sections.remove(i);
                if let Some(n) = self.store.notebooks.iter_mut().find(|n| n.id == to) { n.sections.push(s); }
                self.st.dirty = true;
                self.st.say("Section moved.");
            }
            if let Some(i) = clicked { let nb = self.nb; self.open_section(nb, i); self.st.search.clear(); self.st.tag_filter = None; }
        });
        self.take_pending_delete();
        self.clamp();
        let sec_color = c3(self.store.notebooks[self.nb].sections[self.sec].color);
        let pages = egui::Panel::left("pages").show_separator_line(false);
        let pages = if phone { pages.resizable(false).exact_size(full - sec_w - 6.0) } else { pages.resizable(true).default_size(236.0).size_range(170.0..=400.0) };
        pages.frame(crate::card(ui).inner_margin(egui::Margin { left: 10, right: 10, top: 12, bottom: 10 }).outer_margin(egui::Margin { left: 0, right: 6, top: 0, bottom: 12 })).show(ui, |ui| {
            if self.st.tasks { self.tasks_panel(ui); return; }
            if !self.st.search.trim().is_empty() || self.st.tag_filter.is_some() { self.search_panel(ui); return; }
            let name = self.store.notebooks[self.nb].sections[self.sec].name.clone();
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 5.0, sec_color);
                // the buttons first, then the section's name in what's left (shortened on narrow screens)
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let t = ui.add(egui::Button::new("▾").frame_when_inactive(false)).tip("New page on a background, or a flashcard deck");
                    egui::Popup::menu(&t).show(|ui| {
                        for (p, icon, name) in PAPERS { if ui.button(tf!("{icon}  {name}", icon = icon, name = tr(name))).clicked() { self.new_page(Some(p)); ui.close(); } }
                        ui.separator();
                        for (i, (name, ..)) in crate::TEMPLATES.iter().enumerate() { if ui.button(format!("▦  {}", tr(name))).clicked() { self.new_page_template(i); ui.close(); } }
                        ui.separator();
                        if ui.button(tr("▤  Flashcard deck")).clicked() { self.new_deck("New deck", vec![]); ui.close(); }
                    });
                    if ui.add(egui::Button::new(RichText::new(tr("+ Page")).color(accent(ui))).frame_when_inactive(false)).tip("New page (Ctrl+N)").clicked() { self.new_page(None); }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| { ui.add(egui::Label::new(RichText::new(name).font(rich::serif(18.0)).color(ui.visuals().strong_text_color())).truncate()); });
                });
            });
            ui.add_space(2.0);
            let r = ui.cursor();
            ui.painter().line_segment([Pos2::new(r.left() + 2.0, r.top()), Pos2::new(r.left() + 40.0, r.top())], EStroke::new(3.0, sec_color));
            ui.painter().hline(r.left() + 44.0..=r.right(), r.top(), EStroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
            ui.add_space(6.0);
            self.page_list(ui);
        });
        self.take_pending_delete();
    }

    /// Deletes are requested from inside menus (where the store is borrowed) and carried out here.
    fn take_pending_delete(&mut self) {
        if let Some(id) = self.st.status.strip_prefix("delete:").and_then(|s| s.parse::<u64>().ok()) { self.st.status.clear(); self.delete(id); }
    }

    fn notebook_switcher(&mut self, ui: &mut egui::Ui) {
        let n = &self.store.notebooks[self.nb];
        let (id, name, color) = (n.id, n.name.clone(), c3(n.color));
        if self.st.rename.as_ref().is_some_and(|r| r.0 == id) { nav_item(ui, &mut self.st, id, &name, None, true, 0, None); return; }
        let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
        let v = ui.visuals();
        ui.painter().rect_filled(rect, 8.0, if r.hovered() { v.widgets.hovered.weak_bg_fill } else { v.extreme_bg_color });
        ui.painter().rect_stroke(rect, 8.0, v.widgets.noninteractive.bg_stroke, egui::StrokeKind::Inside);
        let badge = Rect::from_min_size(rect.min + Vec2::new(8.0, 8.0), Vec2::splat(24.0));
        ui.painter().rect_filled(badge, 6.0, color);
        ui.painter().text(badge.center(), Align2::CENTER_CENTER, name.chars().next().unwrap_or('N').to_uppercase().to_string(), FontId::proportional(14.0), Color32::WHITE);
        let p = ui.painter_at(rect.shrink(2.0));
        p.text(Pos2::new(badge.right() + 8.0, rect.center().y), Align2::LEFT_CENTER, &name, FontId::proportional(14.5), v.strong_text_color());
        p.text(Pos2::new(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, "⏷", FontId::proportional(12.0), v.weak_text_color());
        if r.double_clicked() { self.st.rename = Some((id, name.clone())); }
        let r = r.tip("Switch notebook");
        let mut pick = None;
        let mut action = 0;
        egui::Popup::menu(&r).show(|ui| {
            ui.set_min_width(200.0);
            ui.label(RichText::new(tr("NOTEBOOKS")).size(10.5).weak());
            for (i, n) in self.store.notebooks.iter().enumerate() {
                let r = nav_item(ui, &mut self.st, n.id, &n.name, Some(c3(n.color)), i == self.nb, 0, None);
                if r.clicked() { pick = Some(i); ui.close(); }
            }
            ui.separator();
            if ui.button(tr("+  New notebook")).clicked() { action = 1; ui.close(); }
            if ui.button(tr("✎  Rename this notebook")).clicked() { action = 2; ui.close(); }
            if ui.button(tr("🗑  Recently deleted")).clicked() { action = 3; ui.close(); }
            // this notebook's colour and place in the list
            ui.horizontal(|ui| for c in PALETTE {
                let (rect, cr) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                ui.painter().rect_filled(rect, 4.0, c3(c));
                if cr.clicked() { action = 10 + PALETTE.iter().position(|x| *x == c).unwrap_or(0); ui.close(); }
            });
            if self.nb > 0 && ui.button(tr("Move up")).clicked() { action = 5; ui.close(); }
            if self.nb + 1 < self.store.notebooks.len() && ui.button(tr("Move down")).clicked() { action = 6; ui.close(); }
            if self.store.notebooks.len() > 1 && ui.button(RichText::new(tr("Delete this notebook")).color(Color32::from_rgb(200, 60, 50))).clicked() { action = 4; ui.close(); }
        });
        if let Some(i) = pick { self.open_section(i, 0); }
        match action {
            1 => {
                let c = PALETTE[self.store.notebooks.len() % PALETTE.len()];
                let n = Notebook { id: new_id(), name: tr("New notebook").into(), color: c, sections: vec![] };
                self.st.rename = Some((n.id, n.name.clone()));
                self.store.notebooks.push(n);
                self.go_to(self.store.notebooks.len() - 1, 0, 0);
            }
            2 => self.st.rename = Some((id, name)),
            3 => self.st.show_trash = true,
            4 => self.delete(id),
            5 | 6 => { let (a, b) = if action == 5 { (self.nb, self.nb - 1) } else { (self.nb, self.nb + 1) }; self.store.notebooks.swap(a, b); self.nb = b; self.st.dirty = true; }
            c if c >= 10 => { self.store.notebooks[self.nb].color = PALETTE[(c - 10).min(PALETTE.len() - 1)]; self.st.dirty = true; }
            _ => {}
        }
    }

    fn add_section(&mut self) {
        let nb = &mut self.store.notebooks[self.nb];
        let c = PALETTE[(nb.sections.len() + 1) % PALETTE.len()];
        let s = Section { id: new_id(), name: "New section".into(), color: c, pages: vec![], lock: None };
        self.st.rename = Some((s.id, s.name.clone()));
        nb.sections.push(s);
        let (n, i) = (self.nb, nb.sections.len() - 1);
        self.go_to(n, i, 0);
    }

    fn page_list(&mut self, ui: &mut egui::Ui) {
        // a closed locked section can't take pages (saving would drop them next to its placeholder)
        let targets: Vec<(u64, String)> = self.store.notebooks.iter().flat_map(|n| n.sections.iter().filter(|s| !s.sealed()).map(move |s| (s.id, format!("{} › {}", n.name, s.name)))).collect();
        let (nb, sec) = (self.nb, self.sec);
        let sec_id = self.store.notebooks[nb].sections[sec].id;
        let mut drop_to: Option<(usize, usize)> = None;
        let mut clicked = None;
        let mut act: Option<(usize, u8)> = None; // 1 indent, 2 up, 3 down, 4 duplicate, 5 copy
        let mut move_to = None;
        egui::ScrollArea::vertical().id_salt("pages-scroll").auto_shrink([false; 2]).show(ui, |ui| {
            let section = &self.store.notebooks[nb].sections[sec];
            let n = section.pages.len();
            for (i, p) in section.pages.iter().enumerate() {
                let title = if p.title.trim().is_empty() { tr("Untitled page") } else { &p.title };
                // a one-line preview of the page, like the first line of a letter
                let first = p.blocks.iter().filter(|b| matches!(&b.kind, Kind::Text { heading: 0, .. } | Kind::Checklist { .. })).min_by(|a, b| a.pos[1].total_cmp(&b.pos[1]));
                let preview = first.map(|b| b.kind.summary().replace(['*', '=', '$', '`'], "")).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| format_date(p.created, false));
                let preview = if p.kind == PageKind::Deck {
                    // what needs studying, at a glance
                    let now = now_secs();
                    let (due, new) = p.deck().iter().fold((0, 0), |(d, n), c| match self.store.study.get(&c.key) { None => (d, n + 1), Some(s) if s.due <= now => (d + 1, n), _ => (d, n) });
                    if due + new > 0 { tf!("▤ {due} due · {new} new · {n} cards", due = due, new = new, n = p.cards.len()) } else { tf!("▤ Flashcards · {n} cards", n = p.cards.len()) }
                } else if p.kind == PageKind::Locked { tr("Encrypted").to_string() } else { preview };
                let sub = Some(if preview.chars().count() > 48 { format!("{}…", preview.chars().take(48).collect::<String>()) } else { preview });
                let r = nav_item(ui, &mut self.st, p.id, title, None, i == self.pg, p.level, sub);
                if r.clicked() { clicked = Some(i); }
                if i == self.pg && self.st.reveal { r.scroll_to_me(None); self.st.reveal = false; }
                // with a mouse, drag a page up or down the list, or onto a section (on touch a drag scrolls)
                if self.st.input == Input::Mouse && p.kind != PageKind::Locked && self.st.rename.is_none() {
                    let d = r.interact(Sense::drag());
                    d.dnd_set_drag_payload(DragPage(sec_id, i));
                    // the line shows where it lands: above this page, or below the last one when dragged down onto it
                    let last = |from: usize| i + 1 == n && from < i;
                    if let Some(x) = d.dnd_hover_payload::<DragPage>().filter(|x| x.0 == sec_id && x.1 != i) { ui.painter().hline(r.rect.x_range(), if last(x.1) { r.rect.bottom() } else { r.rect.top() }, EStroke::new(2.0, accent(ui))); }
                    if let Some(x) = d.dnd_release_payload::<DragPage>() { if x.0 == sec_id && x.1 != i { drop_to = Some((x.1, if last(x.1) { n } else { i })); } }
                }
                if p.kind == PageKind::Locked { continue; } // the placeholder of a closed locked section has nothing to rename, move or delete
                r.context_menu(|ui| {
                    if ui.button(tr("Rename")).clicked() { self.st.rename = Some((p.id, title.to_string())); ui.close(); }
                    if ui.button(tr("Duplicate")).clicked() { act = Some((i, 4)); ui.close(); }
                    if ui.button(tr(if p.level == 0 { "Make subpage" } else { "Promote to page" })).clicked() { act = Some((i, 1)); ui.close(); }
                    if i > 0 && ui.button(tr("Move up")).clicked() { act = Some((i, 2)); ui.close(); }
                    if i + 1 < n && ui.button(tr("Move down")).clicked() { act = Some((i, 3)); ui.close(); }
                    ui.menu_button(tr("Move to section"), |ui| {
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| for (id, label) in &targets {
                            if *id != section.id && ui.button(label).clicked() { move_to = Some((i, *id)); ui.close(); }
                        });
                    });
                    if ui.button(tr("Copy as Markdown")).clicked() { act = Some((i, 5)); ui.close(); }
                    if ui.button(tr("Copy link")).tip("Copies [[the page's name]], to paste into another page").clicked() { ui.ctx().copy_text(format!("[[{title}]]")); ui.close(); }
                    let fav = self.st.p.favorites.contains(&p.id);
                    if ui.button(tr(if fav { "Remove from favourites" } else { "Add to favourites" })).clicked() { act = Some((i, 6)); ui.close(); }
                    if ui.button(tr("Page history…")).clicked() { act = Some((i, 7)); ui.close(); }
                    ui.separator();
                    if ui.button(RichText::new(tr("Delete page")).color(Color32::from_rgb(200, 60, 50))).clicked() { self.st.status = format!("delete:{id}", id = p.id); ui.close(); }
                });
            }
        });
        let pages = &mut self.store.notebooks[nb].sections[sec].pages;
        match act {
            Some((i, 1)) => { pages[i].level = if pages[i].level == 0 { 1 } else { 0 }; self.st.dirty = true; }
            Some((i, 2)) => { pages.swap(i, i - 1); if self.pg == i { self.pg -= 1 } else if self.pg == i - 1 { self.pg += 1 } self.st.dirty = true; }
            Some((i, 3)) => { pages.swap(i, i + 1); if self.pg == i { self.pg += 1 } else if self.pg == i + 1 { self.pg -= 1 } self.st.dirty = true; }
            Some((i, 4)) => self.duplicate_page(nb, sec, i),
            Some((i, 5)) => { let md = pages[i].plain_text(); ui.ctx().copy_text(md); self.st.say("Copied as Markdown."); }
            Some((i, 6)) => { let id = pages[i].id; if self.st.p.favorites.contains(&id) { self.st.p.favorites.retain(|f| *f != id); } else { self.st.p.favorites.push(id); } self.st.dirty = true; }
            Some((i, 7)) => { self.go_to(nb, sec, i); self.show_history(); }
            _ => {}
        }
        // dropped onto another row: it goes there (the open page stays open)
        if let Some((from, to)) = drop_to.filter(|(f, _)| *f < self.store.notebooks[nb].sections[sec].pages.len()) {
            let pages = &mut self.store.notebooks[nb].sections[sec].pages;
            let open = pages.get(self.pg).map(|p| p.id);
            crate::model::move_pages(pages, from, to);
            if let Some(id) = open { if let Some(k) = pages.iter().position(|p| p.id == id) { self.pg = k; } }
            self.st.dirty = true;
        }
        if let Some((i, to)) = move_to {
            let p = self.store.notebooks[nb].sections[sec].pages.remove(i);
            let title = p.title.clone();
            if let Some(s) = self.store.notebooks.iter_mut().flat_map(|n| n.sections.iter_mut()).find(|s| s.id == to) { s.pages.push(p); }
            self.st.dirty = true;
            self.st.say(tf!("Moved “{title}”.", title = title));
            self.clamp();
        }
        if let Some(i) = clicked { self.go_to(nb, sec, i); }
    }

    fn search_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr(if self.st.tag_filter.is_some() && self.st.search.trim().is_empty() { "Tags" } else { "Search" })).size(15.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new("✕").frame_when_inactive(false)).tip("Close (Esc)").clicked() { self.st.search.clear(); self.st.tag_filter = None; }
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            for (t, label) in [(None, tr("All")), (Some(Tag::None), tr("Any tag")), (Some(Tag::Todo), tr("☐ To Do")), (Some(Tag::Important), tr("★ Important")), (Some(Tag::Question), tr("? Question"))] {
                if ui.add(egui::Button::new(RichText::new(label).size(12.0)).selected(self.st.tag_filter == t).corner_radius(10)).clicked() { self.st.tag_filter = t; }
            }
        });
        // where to look: everywhere, this notebook (course), or this section
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (k, label) in [(0u8, "Everywhere"), (1, "This notebook"), (2, "This section")] {
                if ui.add(egui::Button::new(RichText::new(tr(label)).size(11.5)).selected(self.st.search_scope == k).corner_radius(10)).clicked() { self.st.search_scope = k; self.st.search_cache = None; }
            }
        });
        ui.separator();
        let q = self.st.search.trim().to_lowercase();
        let filter = self.st.tag_filter;
        let (scope, here) = (self.st.search_scope, (self.nb, self.sec));
        let in_scope = move |ni: usize, si: usize| match scope { 1 => ni == here.0, 2 => (ni, si) == here, _ => true };
        let acc = accent(ui);
        let mark = if ui.visuals().dark_mode { Color32::from_rgba_unmultiplied(200, 170, 0, 90) } else { Color32::from_rgb(255, 236, 140) };
        let mut go = None;
        let mut toggle = None;
        let mut found = 0;
        // full-text search over every page is the costly part: kept until the query changes (or a second passes)
        let fresh = self.st.search_cache.as_ref().is_some_and(|(cq, at, _)| *cq == q && now_f() - at < 1.0);
        if filter.is_none() && !fresh {
            let mut hits: Vec<SearchHit> = vec![];
            for (ni, n) in self.store.notebooks.iter().enumerate() { for (si, s) in n.sections.iter().enumerate().filter(|(si, _)| in_scope(ni, *si)) { for (pi, p) in s.pages.iter().enumerate() {
                // also handwriting and PDF text read in the background
                let mut text = p.plain_text();
                if let Some(extra) = self.st.index.text(p.id) { text.push('\n'); text.push_str(extra); }
                let lower = text.to_lowercase();
                // exact matches first, then near misses (typos, handwriting read slightly wrong)
                let Some((at, end, exact)) = crate::index::fuzzy_find(&lower, &q) else { continue };
                hits.push((!exact, (ni, si, pi), p.title.clone(), format!("{} › {}", n.name, s.name), text, at, end));
            }}}
            hits.sort_by_key(|h| h.0);
            self.st.search_cache = Some((q.clone(), now_f(), hits));
        }
        let hits: Vec<SearchHit> = if filter.is_none() { self.st.search_cache.as_ref().map(|c| c.2.clone()).unwrap_or_default() } else { vec![] };
        if filter.is_none() && !q.is_empty() { ui.label(RichText::new(tf!("{n} page(s)", n = hits.len())).size(11.0).weak()); }
        egui::ScrollArea::vertical().id_salt("search-scroll").auto_shrink([false; 2]).show(ui, |ui| {
            for (ni, n) in self.store.notebooks.iter().enumerate() {
                for (si, s) in n.sections.iter().enumerate() {
                    for (pi, p) in s.pages.iter().enumerate() {
                        if let Some(f) = filter {
                            for b in &p.blocks {
                                let tag_ok = match f { Tag::None => b.tag != Tag::None, Tag::Todo => matches!(b.tag, Tag::Todo | Tag::Done), t => b.tag == t };
                                let text = b.kind.summary();
                                if !tag_ok || (!q.is_empty() && !text.to_lowercase().contains(&q)) { continue; }
                                found += 1;
                                ui.horizontal(|ui| {
                                    match b.tag {
                                        Tag::Todo | Tag::Done => { let mut d = b.tag == Tag::Done; if ui.checkbox(&mut d, "").changed() { toggle = Some((p.id, b.id)); } }
                                        Tag::Important => { ui.label(RichText::new("★").color(Color32::from_rgb(234, 179, 8))); }
                                        Tag::Question => { ui.label(RichText::new("?").strong().color(Color32::from_rgb(124, 58, 237))); }
                                        Tag::None => {}
                                    }
                                    let label = if text.chars().count() > 60 { format!("{}…", text.chars().take(60).collect::<String>()) } else { text.clone() };
                                    let r = ui.add(egui::Label::new({ let t = RichText::new(label).size(13.0); if b.tag == Tag::Done { t.strikethrough() } else { t } }).sense(Sense::click()).wrap());
                                    if r.tip(format!("{} › {} › {}", n.name, s.name, p.title)).clicked() { go = Some((ni, si, pi)); }
                                });
                            }
                        }
                    }
                }
            }
            for (_, at_page, title, path, text, at, end) in &hits {
                let (at, end) = (*at, (*end).min(text.len()));
                found += 1;
                let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::click());
                if r.hovered() { ui.painter().rect_filled(rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill); }
                let painter = ui.painter_at(rect.shrink(4.0));
                let strong = ui.visuals().strong_text_color();
                painter.galley(rect.min + Vec2::new(8.0, 6.0), one_line(&painter, if title.is_empty() { tr("Untitled page") } else { title.as_str() }, FontId::proportional(14.0), strong, rect.width() - 16.0), strong);
                painter.galley(rect.min + Vec2::new(8.0, 24.0), one_line(&painter, path, FontId::proportional(10.5), acc, rect.width() - 16.0), acc);
                // snippet around the match, with the match highlighted
                let start = text.get(..at).unwrap_or("").char_indices().rev().nth(24).map_or(0, |(i, _)| i);
                let tail_end = text.get(end..).and_then(|t| t.char_indices().nth(40)).map_or(text.len(), |(i, _)| end + i);
                if text.is_char_boundary(start) && text.is_char_boundary(at) && text.is_char_boundary(end) && text.is_char_boundary(tail_end) {
                    let mut job = egui::text::LayoutJob::default();
                    let f = |c: Color32, bg: Color32| egui::TextFormat { font_id: FontId::proportional(11.5), color: c, background: bg, ..Default::default() };
                    let weak = ui.visuals().weak_text_color();
                    let plain = |t: &str| t.replace('\n', " ").replace(['*', '=', '$', '`', '\\', '{', '}', '^', '#'], "");
                    job.append(&plain(&text[start..at]), 0.0, f(weak, Color32::TRANSPARENT));
                    job.append(&text[at..end], 0.0, f(ui.visuals().text_color(), mark));
                    job.append(&plain(&text[end..tail_end]), 0.0, f(weak, Color32::TRANSPARENT));
                    job.wrap.max_width = rect.width() - 16.0;
                    job.wrap.max_rows = 1;
                    let g = painter.layout_job(job);
                    painter.galley(rect.min + Vec2::new(8.0, 39.0), g, weak);
                }
                if r.clicked() { go = Some(*at_page); }
            }
            if found == 0 { ui.add_space(20.0); ui.vertical_centered(|ui| ui.label(RichText::new(tr(if filter.is_some() { "No tagged notes yet.\nTag a note from the Home tab." } else { "No matches." })).weak())); }
        });
        if let Some((pid, bid)) = toggle {
            if let Some(b) = self.find_page(pid).and_then(|p| p.blocks.iter_mut().find(|b| b.id == bid)) {
                b.tag = if b.tag == Tag::Done { Tag::Todo } else { Tag::Done };
            }
            self.st.dirty = true;
        }
        if let Some((a, b, c)) = go { self.go_to(a, b, c); if filter.is_none() { self.st.find_text = Some(q.clone()).filter(|q| !q.is_empty()); self.st.search.clear(); } }
    }

    pub(crate) fn ai_pane(&mut self, ui: &mut egui::Ui) {
        // phones: the assistant takes the whole width (✦ or Back closes it)
        let full = ui.ctx().content_rect().width();
        let panel = egui::Panel::right("ai").show_separator_line(false);
        let panel = if full < 600.0 { panel.exact_size(full - 30.0).resizable(false) } else { panel.default_size(284.0).size_range(240.0..=520.0) };
        panel
            .frame(crate::card(ui).inner_margin(14).outer_margin(egui::Margin { left: 6, right: 14, top: 0, bottom: 12 })).show(ui, |ui| {
            let acc = accent(ui);
            ui.horizontal(|ui| {
                ui.label(RichText::new("✦").size(17.0).color(acc));
                ui.label(RichText::new(tr("Assistant")).size(16.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new("✕").frame_when_inactive(false)).tip("Close").clicked() { self.st.p.show_ai = false; }
                    if ui.add(egui::Button::new("⚙").frame_when_inactive(false)).tip("AI settings").clicked() { self.st.show_settings = true; self.st.settings_tab = 3; }
                    if !self.st.ai_log.is_empty() && ui.add(egui::Button::new("🗑").frame_when_inactive(false)).tip("Clear the conversation").clicked() { self.st.ai_log.clear(); }
                });
            });
            if self.st.ai.ready() {
                ui.label(RichText::new(tf!("{model} · your own key", model = self.st.ai.model)).size(11.0).weak());
            } else {
                egui::Frame::new().fill(ui.visuals().selection.bg_fill).corner_radius(8).inner_margin(10).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(tr("Bring your own AI key")).strong());
                    ui.label(RichText::new(tr("Works with Anthropic, OpenAI, OpenRouter or a local model. Your key stays on this device.")).size(12.0));
                    if ui.button(tr("Add a key…")).clicked() { self.st.show_settings = true; self.st.settings_tab = 3; }
                });
            }
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                for (label, prompt) in [("Summarize", "Summarize this page in a few bullet points."), ("Action items", "List the action items from this page as a checklist."),
                    ("Quiz me", "Write 3 short quiz questions about this page. Put the answers at the end."), ("Explain", "Explain the main idea of this page simply, with one example.")] {
                    if ui.add(egui::Button::new(RichText::new(tr(label)).size(12.0)).corner_radius(10)).clicked() { self.chat(prompt.into()); }
                }
                // the person's own prompts (Settings ▸ AI)
                for p in self.st.p.prompts.clone().lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from) {
                    let label: String = p.chars().take(24).collect();
                    if ui.add(egui::Button::new(RichText::new(label).size(12.0)).corner_radius(10)).tip(&p).clicked() { self.chat(p); }
                }
            });
            ui.add_space(4.0);
            let mut insert = None;
            let colors = rich::Colors::from(ui, acc);
            let h = ui.available_height() - 96.0;
            egui::ScrollArea::vertical().id_salt("ai-scroll").max_height(h.max(60.0)).stick_to_bottom(true).auto_shrink([false; 2]).show(ui, |ui| {
                if self.st.ai_log.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| ui.label(RichText::new(tr("Ask anything about this page.\nAnswers can be inserted as notes,\nincluding equations and checklists.")).weak()));
                }
                let w = ui.available_width();
                for (i, (from_ai, text)) in self.st.ai_log.iter().enumerate() {
                    if text.starts_with("———") {
                        // where the conversation moved to another page
                        ui.vertical_centered(|ui| ui.label(RichText::new(text).size(11.0).weak()));
                    } else if *from_ai {
                        egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(10).inner_margin(10).show(ui, |ui| {
                            ui.set_width(w - 20.0);
                            let _ = rich::view(ui, text, 13.5, 0, &colors);
                            if !text.starts_with('⚠') {
                                ui.horizontal(|ui| {
                                    ui.menu_button(RichText::new(tr("↧ Insert")).small(), |ui| {
                                        if ui.button(tr("At the end of the page")).clicked() { insert = Some(i); ui.close(); }
                                        if self.st.last_text.is_some() && ui.button(tr("Below the note I was writing in")).clicked() { insert = Some(i); self.st.insert_near = true; ui.close(); }
                                    });
                                    if ui.small_button(tr("⧉ Copy")).clicked() { ui.ctx().copy_text(text.clone()); }
                                });
                            }
                        });
                    } else {
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            egui::Frame::new().fill(ui.visuals().selection.bg_fill).corner_radius(10).inner_margin(egui::Margin::symmetric(10, 7)).show(ui, |ui| {
                                ui.set_max_width(w * 0.85);
                                ui.label(RichText::new(text).size(13.5));
                            });
                        });
                    }
                    ui.add_space(6.0);
                }
                if !self.st.ai_rx.is_empty() { ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(tr("Thinking…")).weak());
                    // stop waiting (a slow local model, a long page): the late answer is ignored
                    if ui.small_button(tr("Stop")).clicked() { self.st.ai_rx.clear(); self.st.say("Stopped."); }
                }); }
            });
            if let Some(i) = insert { let t = self.st.ai_log[i].1.clone(); self.insert_markdown(&t); }
            ui.add_space(4.0);
            let r = ui.add(egui::TextEdit::multiline(&mut self.st.ai_prompt).hint_text(tr("Ask about this page…")).desired_rows(2).desired_width(f32::INFINITY));
            let enter = r.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
            ui.horizontal(|ui| {
                if !self.st.touch_ui { ui.label(RichText::new(tr("Shift+Enter for a new line")).size(10.5).weak()); }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let send = ui.add(egui::Button::new(RichText::new(tr("Send ➤")).color(Color32::WHITE)).fill(accent_base())).clicked();
                    if (send || enter) && !self.st.ai_prompt.trim().is_empty() {
                        let p = std::mem::take(&mut self.st.ai_prompt);
                        self.chat(p.trim().to_string());
                    }
                });
            });
        });
    }

    /// Ctrl+K: type a few letters of any page and jump there.
    fn switcher_window(&mut self, ctx: &egui::Context) {
        let Some((mut q, mut sel)) = self.st.switcher.take() else { self.st.switch_cache = None; return };
        let ql = q.trim().to_lowercase();
        // (score, title, path, place); the scan (which reads page text) runs only when the query changes
        let cached = self.st.switch_cache.take().filter(|(cq, _)| *cq == ql).map(|(_, h)| h);
        let mut hits: Vec<(f32, String, String, (usize, usize, usize))> = vec![];
        if let Some(h) = &cached { hits = h.clone(); }
        // ">" lists commands; they also show up when the words match their name
        let (cmd_only, cq) = match ql.strip_prefix('>') { Some(r) => (true, r.trim().to_string()), None => (false, ql.clone()) };
        if cached.is_none() {
            for (i, name) in COMMANDS.iter().enumerate() {
                let n = tr(name).to_lowercase();
                if (cmd_only && (cq.is_empty() || n.contains(&cq))) || (!cmd_only && cq.len() >= 2 && n.contains(&cq)) { hits.push((if cmd_only { 5.0 } else { 0.4 }, format!("› {}", tr(name)), tr("Command").to_string(), (usize::MAX, i, 0))); }
            }
        }
        if cached.is_none() && !cmd_only { for (a, n) in self.store.notebooks.iter().enumerate() { for (b, s) in n.sections.iter().enumerate() { for (c, p) in s.pages.iter().enumerate() {
            let title = if p.title.trim().is_empty() { tr("Untitled page").to_string() } else { p.title.clone() };
            let path = format!("{} › {}", n.name, s.name);
            let score = if ql.is_empty() {
                let r = self.st.p.recent.iter().position(|id| *id == p.id);
                match r { Some(i) => 100.0 - i as f32, None if self.st.p.favorites.contains(&p.id) => 50.0, None => 0.0 }
            } else {
                let t = title.to_lowercase();
                if t.starts_with(&ql) { 3.0 } else if ql.split_whitespace().all(|w| t.contains(w)) { 2.0 }
                else if ql.split_whitespace().all(|w| path.to_lowercase().contains(w) || t.contains(w)) { 1.0 }
                else if crate::index::fuzzy_find(&t, &ql).is_some() { 0.8 } // a typo in the title
                else if p.plain_text().to_lowercase().contains(&ql) { 0.5 } else { continue }
            };
            hits.push((score, title, path, (a, b, c)));
        }}}
        }
        if cached.is_none() {
        hits.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
        hits.truncate(12);
        // always a way to make the page you're looking for, unless one has exactly that name
        if !ql.is_empty() && !cmd_only && !hits.iter().any(|h| h.1.trim().eq_ignore_ascii_case(q.trim())) { hits.push((0.0, tf!("+  Create a page called “{trim}”", trim = q.trim()), String::new(), (usize::MAX - 1, 0, 0))); } }
        self.st.switch_cache = Some((ql.clone(), hits.clone()));
        let (down, up, enter) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::Enter)));
        if down { sel = (sel + 1).min(hits.len().saturating_sub(1)); }
        if up { sel = sel.saturating_sub(1); }
        let mut go = if enter { hits.get(sel).map(|h| h.3) } else { None };
        let mut create = false;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16)).corner_radius(16);
        let id = egui::Id::unique("switcher");
        let top = self.st.bars[0] + if self.st.narrow { 12.0 } else { (ctx.content_rect().height() * 0.12).min(120.0) };
        let room = ctx.content_rect().height() - top - self.st.bars[1] - 140.0; // the bottom bar holds the soft keyboard on Android
        let area = egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, Vec2::new(0.0, top));
        let modal = egui::Modal::new(id).area(area).frame(frame).show(ctx, |ui| {
            ui.set_width(crate::fit_w(ui.ctx(), 520.0));
            let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text(tr("Jump to a page… (> for commands)")).font(FontId::proportional(18.0)).desired_width(f32::INFINITY).frame(egui::Frame::NONE));
            if !r.has_focus() { r.request_focus(); }
            if r.changed() { sel = 0; }
            ui.separator();
            ui.label(RichText::new(tr(if cmd_only { "COMMANDS" } else { "PAGES" })).size(10.5).weak());
            egui::ScrollArea::vertical().max_height(room.max(132.0)).show(ui, |ui| {
            for (i, (_, title, path, at)) in hits.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                if i == sel || resp.hovered() { ui.painter().rect_filled(rect, 8.0, if i == sel { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.hovered.weak_bg_fill }); }
                let (strong, weak) = (ui.visuals().strong_text_color(), ui.visuals().weak_text_color());
                let g = one_line(ui.painter(), title, FontId::proportional(14.5), strong, rect.width() - 24.0);
                ui.painter().galley(rect.min + Vec2::new(12.0, 13.0 - g.size().y / 2.0), g, strong);
                let g = one_line(ui.painter(), path, FontId::proportional(11.5), weak, rect.width() - 24.0);
                ui.painter().galley(rect.min + Vec2::new(12.0, 31.0 - g.size().y / 2.0), g, weak);
                if resp.clicked() { go = Some(*at); }
                if i == sel && (down || up) { ui.scroll_to_rect(rect, None); }
            }
            });
            let _ = &mut create;
            if !self.st.touch_ui { ui.label(RichText::new(tr("↑↓ to choose · Enter to open · Esc to close")).size(10.5).weak()); }
        });
        match go {
            Some((usize::MAX, i, _)) => { self.run_command(i); return; }
            Some((x, ..)) if x == usize::MAX - 1 => { let t = q.trim().to_string(); self.open_title(&t); return; }
            Some((a, b, c)) => { self.go_to(a, b, c); return; }
            None => {}
        }
        if create { let t = q.clone(); self.open_title(&t); return; }
        if !modal.should_close() { self.st.switcher = Some((q, sel)); }
    }

    /// What the Ctrl+K list's commands do (see COMMANDS).
    fn run_command(&mut self, i: usize) {
        match i {
            0 => { self.new_page(None); }
            1 => self.new_deck("New deck", vec![]),
            2 => self.start_review(),
            3 => self.st.tasks = true,
            4 => self.st.show_settings = true,
            5 => self.st.show_trash = true,
            6 => self.show_history(),
            7 => self.st.show_shortcuts = true,
            8 => { self.st.p.dark ^= true; self.st.p.follow_system = false; self.st.dirty = true; self.st.theme_dirty = true; }
            9 => self.export_pdf(true),
            10 => self.start_lecture(false),
            _ => {}
        }
    }

    pub(crate) fn dialogs(&mut self, ctx: &egui::Context) {
        if self.st.show_settings { self.settings_window(ctx); }
        if self.st.sync_ask.is_some() { self.sync_ask_window(ctx); }
        if self.st.history.is_some() { self.history_window(ctx); }
        if self.st.compare.is_some() { self.compare_window(ctx); }
        if self.st.lock_prompt.is_some() { self.lock_window(ctx); }
        if self.st.tour.is_some() { self.tour_window(ctx); }
        if self.st.show_news { self.news_window(ctx); }
        if self.st.show_help { self.help_window(ctx); }
        if self.st.crash.is_some() { self.crash_window(ctx); }
        if self.st.switcher.is_some() { self.switcher_window(ctx); } else { self.st.switch_cache = None; }
        if self.st.study.is_some() { self.study_window(ctx); }
        if self.st.mic_prompt { self.mic_window(ctx); }
        if self.st.restore_ask.is_some() { self.restore_window(ctx); } else { self.st.restore_parsed = None; }
        if let Some(mut r) = self.st.ink_review.take() {
            // the recognizer (or AI) can misread: correct it here before it replaces the handwriting
            let mut done = None;
            let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(20)).corner_radius(16);
            let m = egui::Modal::new(egui::Id::unique("ink-review")).frame(frame).show(ctx, |ui| {
                ui.set_width(crate::fit_w(ui.ctx(), 460.0));
                ui.label(RichText::new(tr(if r.math { "Read as maths" } else { "Read as text" })).font(rich::serif(20.0)));
                ui.label(RichText::new(tr("Correct anything misread, then choose what to do.")).weak());
                ui.add(egui::TextEdit::multiline(&mut r.text).desired_rows(4).desired_width(f32::INFINITY).font(if r.math { egui::TextStyle::Monospace } else { egui::TextStyle::Body }));
                if r.math && !r.text.trim().is_empty() { math::math_label(ui, &r.text, 20.0); }
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new(tr("Replace the ink")).color(Color32::WHITE)).fill(accent_base())).clicked() { done = Some(Some(false)); }
                    if ui.button(tr("Add it, keep the ink")).clicked() { done = Some(Some(true)); }
                    if ui.button(tr("Cancel")).clicked() { done = Some(None); }
                });
            });
            match done {
                Some(Some(keep)) => self.apply_ink(r, keep),
                Some(None) => {}
                None if m.should_close() => {}
                None => self.st.ink_review = Some(r),
            }
        }
        if let Some(id) = self.st.transcribe_ask {
            // how much goes where, before a long lecture is uploaded (it can cost money)
            let secs = self.store.notebooks.iter().flat_map(|n| &n.sections).flat_map(|s| &s.pages).flat_map(|p| &p.recordings).find(|r| r.id == id).map_or(0.0, |r| r.secs);
            let host = self.st.ai.base_url.split('/').nth(2).unwrap_or("").to_string();
            let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
            let mut go = None;
            let m = egui::Modal::new(egui::Id::unique("transcribe-ask")).frame(frame).show(ctx, |ui| {
                ui.set_width(crate::fit_w(ui.ctx(), 420.0));
                ui.label(RichText::new(tr("Make a transcript?")).font(rich::serif(20.0)));
                ui.label(tf!("About {min} minutes of audio (≈{mb} MB) will be sent to {host} to be turned into text. Services charge for this by the minute.", min = format!("{:.0}", (secs / 60.0).ceil()), mb = format!("{:.0}", secs * 32_000.0 / 1e6), host = host));
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new(tr("Send it")).color(Color32::WHITE)).fill(accent_base())).clicked() { go = Some(true); }
                    if ui.button(tr("Cancel")).clicked() { go = Some(false); }
                });
            });
            if m.should_close() { go = Some(false); }
            if let Some(g) = go { self.st.transcribe_ask = None; if g { self.start_transcribe(id); } }
        }
        if let Some(path) = self.st.drop_pdf.clone() {
            let mut how = None;
            let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(22)).corner_radius(18);
            let m = egui::Modal::new(egui::Id::unique("drop-pdf")).frame(frame).show(ctx, |ui| {
                ui.set_width(crate::fit_w(ui.ctx(), 400.0));
                ui.label(RichText::new(path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).strong());
                ui.add_space(6.0);
                if ui.button(tr("Print its pages onto this page (to write on them)")).clicked() { how = Some(Ins::Printout); }
                if ui.button(tr("Attach it as a file")).clicked() { how = Some(Ins::Attach); }
                if ui.button(tr("Cancel")).clicked() { self.st.drop_pdf = None; }
            });
            if m.should_close() { self.st.drop_pdf = None; }
            if let Some(h) = how { self.st.drop_pdf = None; self.insert_file(&path, h); }
        }
        if self.st.show_trash {
            let mut open = true;
            let mut restore = None;
            let mut purge = None;
            egui::Window::new(crate::window_title(tr("Recently deleted"))).frame(crate::window_frame(ctx)).open(&mut open).collapsible(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).default_width(crate::fit_w(ctx, 420.0)).max_width(crate::fit_w(ctx, 2000.0)).show(ctx, |ui| {
                if self.store.trash.is_empty() { ui.label(RichText::new(tr("Nothing here. Deleted pages, sections and notebooks wait here until you empty the bin.")).weak()); }
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for (i, t) in self.store.trash.iter().enumerate().rev() {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| { ui.label(&t.label); ui.label(RichText::new(tf!("Deleted {x}", x = format_date(t.deleted, true))).size(11.0).weak()); });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button(tr("Delete forever")).clicked() { purge = Some(i); }
                                if ui.small_button(tr("Restore")).clicked() { restore = Some(i); }
                            });
                        });
                        ui.separator();
                    }
                });
                // emptying the bin can't be undone: a second, clearly worded click
                let sure_id = egui::Id::unique("empty-bin-sure");
                let sure = ui.data(|d| d.get_temp::<bool>(sure_id)).unwrap_or(false);
                if !self.store.trash.is_empty() {
                    if !sure { if ui.button(tr("Empty the bin")).clicked() { ui.data_mut(|d| d.insert_temp(sure_id, true)); } }
                    else {
                        ui.label(RichText::new(tr("Everything here will be deleted for good. This can't be undone.")).color(Color32::from_rgb(200, 60, 50)));
                        ui.horizontal(|ui| {
                            if ui.button(RichText::new(tr("Delete all for good")).color(Color32::from_rgb(200, 60, 50))).clicked() { purge = Some(usize::MAX); ui.data_mut(|d| d.remove::<bool>(sure_id)); }
                            if ui.button(tr("Cancel")).clicked() { ui.data_mut(|d| d.remove::<bool>(sure_id)); }
                        });
                    }
                }
            });
            if let Some(i) = restore { self.restore(i); }
            match purge {
                Some(usize::MAX) => { let gone = std::mem::take(&mut self.store.trash); self.purge(gone); }
                Some(i) => { let gone = self.store.trash.remove(i); self.purge(vec![gone]); }
                None => {}
            }
            if !open { self.st.show_trash = false; }
        }
        if self.st.show_shortcuts {
            let mut open = true;
            egui::Window::new(crate::window_title(tr("Keyboard shortcuts"))).frame(crate::window_frame(ctx)).open(&mut open).collapsible(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).resizable(false).show(ctx, |ui| {
                egui::Grid::new("keys").num_columns(2).spacing([24.0, 6.0]).striped(true).show(ui, |ui| {
                    for (k, what) in [("Ctrl+N", "New page"), ("Ctrl+F", "Search all notes"), ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
                        ("Ctrl+B / Ctrl+I", "Bold / italic"), ("Ctrl+Shift+H", "Highlight"), ("Ctrl+M", "Inline math"), ("Ctrl+`", "Inline code"),
                        ("T  P  H  E  L", "Type, pen, highlighter, eraser, select ink"), ("Delete", "Delete selected ink"), ("Esc", "Clear selection and search"),
                        ("Ctrl + / Ctrl −", "Make everything larger or smaller"), ("Ctrl+K", "Jump to a page"), ("Two fingers", "Scroll while drawing (touch screens)"), ("Click empty paper", "Start a new note there"),
                        ("- , 1. , [ ] , #", "Start a list, checkbox or heading while typing"), ("/", "Insert menu while typing"), ("[[", "Link to another page while typing"),
                        ("Space · 1 2 3", "Flashcards: show the answer, then rate it")] {
                        ui.label(RichText::new(tr(k)).monospace()); ui.label(tr(what)); ui.end_row();
                    }
                });
            });
            if !open { self.st.show_shortcuts = false; }
        }
        if let Some((how, mut path)) = self.st.path_prompt.clone() {
            let mut go = false;
            let mut cancel = false;
            egui::Window::new(crate::window_title(tr(match how { Ins::Picture => "Insert picture", Ins::Printout => "Insert printout", Ins::Attach => "Attach file" }))).frame(crate::window_frame(ctx)).collapsible(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
                ui.label(tr("File path on this device:"));
                ui.text_edit_singleline(&mut path);
                ui.horizontal(|ui| { go = ui.button(tr("Insert")).clicked(); cancel = ui.button(tr("Cancel")).clicked(); });
            });
            self.st.path_prompt = if go || cancel { None } else { Some((how, path.clone())) };
            if go { self.insert_file(std::path::Path::new(&path), how); }
        }
    }
}

/// A pen or highlighter drawn as a little marker.
/// A pen in the case: pulled up a little when it's the one in hand (None = highlighter).
fn pen_icon(ui: &mut egui::Ui, col: Color32, selected: bool, kind: Option<PenKind>) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(30.0, 54.0), Sense::click());
    if selected || r.hovered() { ui.painter().rect_filled(rect, 8.0, if selected { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.hovered.weak_bg_fill }); }
    draw_pen(ui.painter(), rect.center_top() + Vec2::new(0.0, if selected { 3.0 } else { 8.0 }), 1.0, kind, col);
    focus_ring(ui, &r, rect, 8.0);
    let name = kind.map_or(tr("Highlighter"), |k| tr(k.name())).to_string();
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
    r
}

/// Draw a writing instrument standing upright with its tip at `tip`; 1.0 scale is about 44 px tall.
fn draw_pen(p: &egui::Painter, tip: Pos2, scale: f32, kind: Option<PenKind>, col: Color32) {
    let at = |x: f32, y: f32| tip + Vec2::new(x, y) * scale;
    let poly = |pts: &[(f32, f32)], fill: Color32| { p.add(egui::Shape::convex_polygon(pts.iter().map(|q| at(q.0, q.1)).collect(), fill, EStroke::NONE)); };
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32, fill: Color32| { p.rect_filled(Rect::from_min_max(at(x0, y0), at(x1, y1)), r * scale, fill); };
    let metal = Color32::from_rgb(196, 201, 207);
    let gold = Color32::from_rgb(214, 178, 92);
    let dark = Color32::from_rgb(55, 60, 66);
    let shine = Color32::from_white_alpha(80);
    let body = |w: f32, top: f32, fill: Color32| {
        rect(-w / 2.0, top, w / 2.0, 44.0, 2.5, fill);
        rect(-w / 2.0 + 1.2, top + 1.0, -w / 2.0 + 2.6, 43.0, 1.0, shine);
    };
    match kind {
        Some(PenKind::Fountain) => {
            body(9.0, 17.0, col);
            rect(-4.5, 13.0, 4.5, 17.5, 1.0, dark); // grip
            poly(&[(-4.0, 13.5), (4.0, 13.5), (1.1, 2.0), (0.0, 0.0), (-1.1, 2.0)], gold); // nib
            p.line_segment([at(0.0, 2.5), at(0.0, 9.0)], EStroke::new(0.9 * scale, dark));
            p.circle_filled(at(0.0, 9.6), 1.1 * scale, dark);
            rect(-4.5, 24.0, 4.5, 25.5, 0.0, gold); // cap band
        }
        Some(PenKind::Ballpoint) => {
            body(7.5, 15.0, col);
            poly(&[(-3.75, 15.5), (3.75, 15.5), (1.0, 4.0), (-1.0, 4.0)], metal); // cone
            p.circle_filled(at(0.0, 3.2), 1.3 * scale, col); // ball
            rect(3.0, 22.0, 4.8, 36.0, 0.8, metal); // clip
        }
        Some(PenKind::Pencil) => {
            let wood = Color32::from_rgb(232, 196, 140);
            body(9.0, 16.0, col);
            p.line_segment([at(-1.5, 16.0), at(-1.5, 38.0)], EStroke::new(0.8 * scale, Color32::from_black_alpha(40))); p.line_segment([at(1.5, 16.0), at(1.5, 38.0)], EStroke::new(0.8 * scale, Color32::from_black_alpha(40)));
            poly(&[(-4.5, 16.5), (4.5, 16.5), (0.0, 1.0)], wood); // sharpened wood
            poly(&[(-1.9, 7.0), (1.9, 7.0), (0.0, 1.0)], dark); // graphite
            rect(-4.5, 38.0, 4.5, 40.5, 0.0, metal); rect(-4.5, 40.5, 4.5, 44.0, 2.0, Color32::from_rgb(240, 150, 160)); // ferrule + eraser
        }
        Some(PenKind::Calligraphy) => {
            body(6.5, 18.0, col);
            rect(-4.0, 13.0, 4.0, 18.5, 1.5, dark); // holder ferrule
            poly(&[(-4.0, 13.5), (4.0, 13.5), (4.0, 4.5), (-4.0, 1.0)], metal); // broad, angled nib
            p.line_segment([at(-4.0, 1.2), at(4.0, 4.6)], EStroke::new(1.6 * scale, col)); p.line_segment([at(0.0, 3.5), at(0.0, 10.0)], EStroke::new(0.8 * scale, dark));
        }
        Some(PenKind::Marker) => {
            body(12.0, 15.0, col);
            rect(-6.0, 12.0, 6.0, 15.5, 1.0, dark); // collar
            poly(&[(-3.5, 12.5), (3.5, 12.5), (2.6, 3.5), (-2.6, 3.5)], Color32::from_rgb(235, 235, 235));
            rect(-2.6, 1.5, 2.6, 5.0, 2.5, col); // felt tip
        }
        None => {
            body(13.0, 15.0, col.gamma_multiply(0.85));
            rect(-6.5, 12.0, 6.5, 15.5, 1.0, Color32::from_rgb(70, 75, 80));
            poly(&[(-4.0, 12.5), (4.0, 12.5), (4.0, 5.0), (-4.0, 1.0)], col); // chisel tip
            rect(-6.5, 30.0, 6.5, 33.0, 0.0, Color32::from_white_alpha(110)); // label band
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        // dates are translated: wait for tests that switch the language, then use English
        let _turn = crate::i18n::tests::LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set("en");
        assert_eq!(super::format_utc(1_793_793_555, false), "Wednesday, November 4, 2026");
        assert_eq!(super::due_day("hand in @2026-11-04 please"), Some(1_793_793_555 / 86_400));
        assert_eq!(super::due_day("mail me @ noon"), None);
        // page titles drop the year in every language (not only where there's a comma before it)
        for (lang, want) in [("de", "Mittwoch, 4. November"), ("fr", "mercredi 4 novembre"), ("en", "Wednesday, November 4")] {
            crate::i18n::set(lang);
            assert_eq!(super::without_year(&super::format_utc(1_793_793_555, false)), want);
        }
        // local time: never more than 14 hours from UTC
        assert!(super::local_offset(1_793_793_555).abs() <= 14 * 3600);
    }
}
