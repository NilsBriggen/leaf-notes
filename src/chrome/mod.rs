//! Everything around the page: title bar, ribbon, navigation, search, AI pane, status bar and dialogs.

mod ai_pane;
mod dialogs;
mod nav;
mod ribbon;
mod search_tasks;
mod status_bar;
mod title_bar;

use crate::*;
use eframe::egui::{Align, Align2, FontId, Layout, Rect, RichText, Sense};

pub(crate) fn want_focus(st: &mut St, r: &egui::Response) {
    if st.nav.focus_id == Some(r.id) {
        r.request_focus();
        st.nav.focus_id = None;
    }
}

/// Theme-aware danger colour (high contrast aware).
pub(crate) fn danger_color(ui: &egui::Ui) -> Color32 {
    if crate::HIGH_CONTRAST.load(std::sync::atomic::Ordering::Relaxed) {
        if ui.visuals().dark_mode { Color32::from_rgb(255, 110, 100) } else { Color32::from_rgb(175, 25, 20) }
    } else if ui.visuals().dark_mode {
        Color32::from_rgb(240, 105, 95)
    } else {
        Color32::from_rgb(200, 55, 45)
    }
}

/// Theme-aware warning colour (high contrast aware).
pub(crate) fn warn_color(ui: &egui::Ui) -> Color32 {
    if crate::HIGH_CONTRAST.load(std::sync::atomic::Ordering::Relaxed) {
        if ui.visuals().dark_mode { Color32::from_rgb(255, 195, 70) } else { Color32::from_rgb(140, 75, 0) }
    } else if ui.visuals().dark_mode {
        Color32::from_rgb(245, 175, 65)
    } else {
        Color32::from_rgb(190, 105, 20)
    }
}

/// Ribbon button: icon over a small label.
pub(crate) fn rbtn(ui: &mut egui::Ui, icon: &str, label: &str, selected: bool) -> egui::Response {
    let label = tr(label);
    let lw = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(11.0), Color32::WHITE).size().x;
    let (rect, r) = ui.allocate_exact_size(Vec2::new((lw + 14.0).max(50.0), 50.0), Sense::click());
    let v = ui.visuals();
    let acc = accent(ui);
    if selected {
        ui.painter().rect_filled(rect, 6.0, v.selection.bg_fill);
    } else if r.hovered() {
        ui.painter().rect_filled(rect, 6.0, v.widgets.hovered.weak_bg_fill);
    }
    let col = if !ui.is_enabled() { v.weak_text_color() } else if selected { acc } else { v.text_color() };
    ui.painter().text(rect.center_top() + Vec2::new(0.0, 17.0), Align2::CENTER_CENTER, icon, FontId::proportional(18.0), col);
    ui.painter().text(
        rect.center_bottom() - Vec2::new(0.0, 10.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.0),
        if ui.is_enabled() { v.text_color() } else { v.weak_text_color() },
    );
    focus_ring(ui, &r, rect, 6.0);
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
    r
}

/// A visible ring around the widget that keyboard focus is on (Tab moves it).
pub(crate) fn focus_ring(ui: &egui::Ui, r: &egui::Response, rect: Rect, radius: f32) {
    if r.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), radius + 2.0, EStroke::new(2.0, accent(ui)), egui::StrokeKind::Outside);
    }
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

/// Pen colours a tap away (black, blue, red, green, purple, orange, brown, grey).
pub(crate) const PEN_COLORS: [[u8; 4]; 8] = [
    [30, 30, 30, 255], [37, 99, 235, 255], [220, 38, 38, 255], [22, 163, 74, 255],
    [124, 58, 237, 255], [234, 88, 12, 255], [120, 72, 40, 255], [107, 114, 128, 255],
];

/// A small round colour button (outlined in dark mode so dark swatches stay visible).
pub(crate) fn swatch(ui: &mut egui::Ui, c: [u8; 4], selected: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
    ui.painter().circle_filled(r.center(), 7.0, c4([c[0], c[1], c[2], 255]));
    let lum = 0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32;
    if ui.visuals().dark_mode && lum < 95.0 {
        ui.painter().circle_stroke(r.center(), 7.0, EStroke::new(1.0, Color32::from_white_alpha(150)));
    }
    if selected || resp.hovered() {
        ui.painter().circle_stroke(r.center(), 8.5, EStroke::new(1.5, ui.visuals().text_color()));
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])));
    resp
}

/// The last colours picked by hand, newest first (four kept).
pub(crate) fn remember_color(v: &mut Vec<[u8; 4]>, c: [u8; 4]) {
    if PEN_COLORS.contains(&c) { return; }
    v.retain(|x| *x != c);
    v.insert(0, c);
    v.truncate(4);
}

pub(crate) fn labelled_slider(ui: &mut egui::Ui, caption: &str, width: f32, value: &mut f32, range: std::ops::RangeInclusive<f32>, tip: &str) {
    ui.horizontal(|ui| {
        let start = ui.cursor().left();
        let label = ui.label(RichText::new(tr(caption)).size(11.5).weak());
        ui.spacing_mut().slider_width = (width - (ui.cursor().left() - start)).max(40.0);
        ui.add(egui::Slider::new(value, range).show_value(false)).labelled_by(label.id).tip(tip);
    });
}

/// Small square ribbon button used for text formatting.
pub(crate) fn sbtn(ui: &mut egui::Ui, text: RichText, tip: &str, selected: bool) -> egui::Response {
    let b = egui::Button::new(text).min_size(Vec2::new(28.0, 22.0)).selected(selected).frame_when_inactive(false);
    ui.add(b).tip(tip)
}

/// A captioned ribbon group optionally followed by a vertical divider.
pub(crate) fn group(ui: &mut egui::Ui, caption: &str, divider: bool, add: impl FnOnce(&mut egui::Ui)) {
    let caption = tr(caption);
    // room for two rows of controls in every group: captions line up and the ribbon keeps its height across tabs
    let h = (ui.spacing().interact_size.y * 2.0 + 6.0).max(52.0);
    let captions = ui.spacing().interact_size.y < 30.0; // the touch ribbon leaves them out
    ui.vertical(|ui| {
        let r = ui.horizontal(|ui| { ui.set_min_height(h); add(ui); }).response.rect;
        if captions {
            let g = ui.painter().layout_no_wrap(caption.to_string(), FontId::proportional(10.5), ui.visuals().weak_text_color());
            let (cr, _) = ui.allocate_exact_size(Vec2::new(r.width().max(g.size().x + 8.0), 13.0), Sense::hover());
            ui.painter().galley(cr.center() - g.size() / 2.0, g, ui.visuals().weak_text_color());
        }
    });
    if divider {
        let (r, _) = ui.allocate_exact_size(Vec2::new(9.0, if captions { h + 12.0 } else { h }), Sense::hover());
        ui.painter().vline(r.center().x, r.y_range().shrink(6.0), EStroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    }
}

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

/// The date without its year, in the reading language, for page titles.
pub(crate) fn short_date(secs: u64) -> String { without_year(&format_date(secs, false)) }
fn without_year(d: &str) -> String { d.rsplit_once(' ').map_or(d.to_string(), |(a, _)| a.trim_end_matches(',').to_string()) }

/// This device's offset from UTC in seconds at that moment (summer time included).
pub(crate) fn local_offset(secs: u64) -> i64 {
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
        if r != u32::MAX { return -((tz.Bias + if r == 2 { tz.DaylightBias } else { tz.StandardBias }) as i64) * 60; }
    }
    #[allow(unreachable_code)]
    { let _ = secs; 0 }
}

fn format_utc(secs: u64, time: bool) -> String {
    let days = (secs / 86_400) as i64;
    let (y, m, d) = {
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
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    Some(era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468)
}

/// A page being dragged in the pages column: (its section's id, its place there).
pub(crate) struct DragPage(pub u64, pub usize);

/// The Ctrl+K list's commands (in `run_command`'s order).
pub(crate) const COMMANDS: [&str; 11] = [
    "New page", "New deck", "Review due cards", "All tasks", "Settings",
    "Recently deleted", "Page history", "Keyboard shortcuts", "Dark mode on/off",
    "Export page as PDF", "New lecture",
];

/// A Ctrl+K row: (score, title, notebook › section, where it is — or a command or "create").
pub(crate) type SwitchHit = (f32, String, String, (usize, usize, usize));

/// A search result: (only a near miss, where the page is, title, notebook › section, the page's text, match start, match end).
pub(crate) type SearchHit = (bool, (usize, usize, usize), String, String, String, usize, usize);

/// A study session: the deck, what's left to go, and how it went.
pub(crate) struct Study {
    pub title: String,
    pub cards: Vec<Card>,
    pub queue: Vec<usize>,
    pub total: usize,
    pub shown: bool,
    pub tally: [u32; 3],
    pub missed: Vec<usize>,
    pub undo: Option<StudyUndo>,
}

/// The last rating, to take back (a wrong key).
pub(crate) struct StudyUndo {
    pub key: String,
    pub before: Option<CardState>,
    pub rating: usize,
    pub queue: Vec<usize>,
    pub missed: Vec<usize>,
}

pub(crate) fn shuffle(v: &mut [usize]) {
    let mut x = now_f().to_bits() | 1;
    for i in (1..v.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.swap(i, (x % (i as u64 + 1)) as usize);
    }
}

/// "just now", "5 min ago", "3 h ago"…
pub(crate) fn ago(t: u64) -> String {
    let d = now_secs().saturating_sub(t);
    if d < 60 { tr("just now").into() }
    else if d < 3600 { tf!("{n} min ago", n = d / 60) }
    else if d < 86_400 { tf!("{n} h ago", n = d / 3600) }
    else { tf!("{n} days ago", n = d / 86_400) }
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
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.horizontal_wrapped(|ui| { ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend); control(ui) });
            });
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
pub(crate) fn nav_item(
    ui: &mut egui::Ui,
    st: &mut St,
    panel: u8,
    id: u64,
    name: &str,
    color: Option<Color32>,
    selected: bool,
    indent: u8,
    sub: Option<String>,
) -> egui::Response {
    if let Some((rpanel, rid, buf)) = &mut st.nav.rename {
        if *rpanel == panel && *rid == id {
            let r = ui.add(egui::TextEdit::singleline(buf).desired_width(f32::INFINITY).id(egui::Id::unique(("rename", panel, id))));
            if !r.has_focus() && !r.lost_focus() { r.request_focus(); }
            if r.lost_focus() { st.nav.commit_rename = true; }
            return r;
        }
    }
    let h = if sub.is_some() { 52.0 } else { 38.0 };
    let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    let v = ui.visuals();
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
    if indent > 0 {
        ui.painter().vline(x - 9.0, rect.y_range().shrink(9.0), EStroke::new(1.5, v.widgets.noninteractive.bg_stroke.color));
    }
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
    if r.double_clicked() { st.nav.rename = Some((panel, id, name.to_string())); }
    focus_ring(ui, &r, rect, 10.0);
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, name));
    r
}

/// A pen in the case: pulled up a little when it's the one in hand (None = highlighter).
pub(crate) fn pen_icon(ui: &mut egui::Ui, col: Color32, selected: bool, kind: Option<PenKind>) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(30.0, 54.0), Sense::click());
    if selected || r.hovered() {
        ui.painter().rect_filled(rect, 8.0, if selected { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.hovered.weak_bg_fill });
    }
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
            rect(-4.5, 13.0, 4.5, 17.5, 1.0, dark);
            poly(&[(-4.0, 13.5), (4.0, 13.5), (1.1, 2.0), (0.0, 0.0), (-1.1, 2.0)], gold);
            p.line_segment([at(0.0, 2.5), at(0.0, 9.0)], EStroke::new(0.9 * scale, dark));
            p.circle_filled(at(0.0, 9.6), 1.1 * scale, dark);
            rect(-4.5, 24.0, 4.5, 25.5, 0.0, gold);
        }
        Some(PenKind::Ballpoint) => {
            body(7.5, 15.0, col);
            poly(&[(-3.75, 15.5), (3.75, 15.5), (1.0, 4.0), (-1.0, 4.0)], metal);
            p.circle_filled(at(0.0, 3.2), 1.3 * scale, col);
            rect(3.0, 22.0, 4.8, 36.0, 0.8, metal);
        }
        Some(PenKind::Pencil) => {
            let wood = Color32::from_rgb(232, 196, 140);
            body(9.0, 16.0, col);
            p.line_segment([at(-1.5, 16.0), at(-1.5, 38.0)], EStroke::new(0.8 * scale, Color32::from_black_alpha(40)));
            p.line_segment([at(1.5, 16.0), at(1.5, 38.0)], EStroke::new(0.8 * scale, Color32::from_black_alpha(40)));
            poly(&[(-4.5, 16.5), (4.5, 16.5), (0.0, 1.0)], wood);
            poly(&[(-1.9, 7.0), (1.9, 7.0), (0.0, 1.0)], dark);
            rect(-4.5, 38.0, 4.5, 40.5, 0.0, metal);
            rect(-4.5, 40.5, 4.5, 44.0, 2.0, Color32::from_rgb(240, 150, 160));
        }
        Some(PenKind::Calligraphy) => {
            body(6.5, 18.0, col);
            rect(-4.0, 13.0, 4.0, 18.5, 1.5, dark);
            poly(&[(-4.0, 13.5), (4.0, 13.5), (4.0, 4.5), (-4.0, 1.0)], metal);
            p.line_segment([at(-4.0, 1.2), at(4.0, 4.6)], EStroke::new(1.6 * scale, col));
            p.line_segment([at(0.0, 3.5), at(0.0, 10.0)], EStroke::new(0.8 * scale, dark));
        }
        Some(PenKind::Marker) => {
            body(12.0, 15.0, col);
            rect(-6.0, 12.0, 6.0, 15.5, 1.0, dark);
            poly(&[(-3.5, 12.5), (3.5, 12.5), (2.6, 3.5), (-2.6, 3.5)], Color32::from_rgb(235, 235, 235));
            rect(-2.6, 1.5, 2.6, 5.0, 2.5, col);
        }
        None => {
            body(13.0, 15.0, col.gamma_multiply(0.85));
            rect(-6.5, 12.0, 6.5, 15.5, 1.0, Color32::from_rgb(70, 75, 80));
            poly(&[(-4.0, 12.5), (4.0, 12.5), (4.0, 5.0), (-4.0, 1.0)], col);
            rect(-6.5, 30.0, 6.5, 33.0, 0.0, Color32::from_white_alpha(110));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        let _turn = crate::i18n::tests::LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set("en");
        assert_eq!(super::format_utc(1_793_793_555, false), "Wednesday, November 4, 2026");
        assert_eq!(super::due_day("hand in @2026-11-04 please"), Some(1_793_793_555 / 86_400));
        assert_eq!(super::due_day("mail me @ noon"), None);
        for (lang, want) in [("de", "Mittwoch, 4. November"), ("fr", "mercredi 4 novembre"), ("en", "Wednesday, November 4")] {
            crate::i18n::set(lang);
            assert_eq!(super::without_year(&super::format_utc(1_793_793_555, false)), want);
        }
        assert!(super::local_offset(1_793_793_555).abs() <= 14 * 3600);
    }

    #[test]
    fn search_snippet_cleans_latex() {
        let s = super::search_tasks::clean_snippet(r"$$\sum_{k=0}^{\infty} \frac{\sin((2k+1)t)}{2k+1}$$");
        assert!(s.contains('∑') && s.contains('∞'), "{s}");
        assert!(!s.contains("fracsin"), "must not concatenate stripped LaTeX tokens: {s}");
    }
}
