//! Calendar (`CalendarPanel.swift`): Month / Week / Agenda over the backend calendar (CalDAV),
//! natural-language quick add, event editor sheet.
use crate::api::R;
use crate::icons;
use crate::state::{Job, Shared};
use crate::theme::{self, ty, Palette, W};
use crate::widgets::{self, BtnKind};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Rounding, Sense, Stroke, Ui};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum View {
    #[default]
    Month,
    Week,
    Agenda,
}

/// What a background job was for, so its result lands in the right place.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Quick,
    Save,
    Delete,
}

/// One calendar event with its dates parsed once (naive local minutes since the epoch).
#[derive(Clone)]
struct Ev {
    raw: Value,
    uid: String,
    summary: String,
    s: i64,
    e: i64,
    all_day: bool,
    location: String,
}

impl Ev {
    fn from_value(v: &Value) -> Option<Ev> {
        let st = v.get("dtstart").and_then(Value::as_str)?;
        let s = parse_min(st)?;
        let all_day = v.get("all_day").and_then(Value::as_bool).unwrap_or(false) || st.len() <= 10;
        let mut e = v.get("dtend").and_then(Value::as_str).and_then(parse_min).unwrap_or(s + if all_day { 1440 } else { 60 });
        if e <= s {
            e = s + if all_day { 1440 } else { 1 };
        }
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        Some(Ev {
            raw: v.clone(),
            uid: text("uid"),
            summary: {
                let t = text("summary");
                if t.is_empty() { "Untitled event".into() } else { t }
            },
            s,
            e,
            all_day,
            location: text("location"),
        })
    }
    fn multi_day(&self) -> bool {
        self.s.div_euclid(1440) != (self.e - 1).div_euclid(1440)
    }
}

#[derive(Default)]
pub struct CalendarPanel {
    view: View,
    /// Anchor day (days since 1970-01-01).
    anchor: i64,
    inited: bool,
    evs: Vec<Ev>,
    loaded: bool,
    fail: Option<String>,
    load: Option<Job<R<Value>>>,
    status: Option<Job<R<Value>>>,
    caldav_ok: bool,
    quick: String,
    quick_err: Option<String>,
    action: Option<(Act, Job<R<Value>>)>,
    event_form: Option<EventForm>,
    week_scrolled: bool,
}

#[derive(Clone, Default)]
struct EventForm {
    uid: Option<String>,
    summary: String,
    start: String,
    end: String,
    timezone: String,
    all_day: bool,
    location: String,
    description: String,
    error: Option<String>,
}

/// A click inside a view.
enum Pick {
    Edit(usize),
    New(i64),
    Delete(usize),
}

const HOUR_H: f32 = 44.0;
const FIRST_HOUR: i64 = 6;
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

impl CalendarPanel {
    pub fn ui(&mut self, ui: &mut Ui, sh: &mut Shared) {
        if !self.inited {
            self.inited = true;
            if self.anchor == 0 {
                self.anchor = today_days();
            }
            self.reload(sh);
            self.status = Some(sh.spawn(|a| a.calendar_status()));
        }
        self.poll(sh);
        let p = sh.palette;

        let (mut prev, mut next, mut today_b, mut newev, mut refresh) = (false, false, false, false, false);
        let mut view = self.view;
        let title = self.title_label();
        let busy = self.action.as_ref().map(|(a, _)| *a == Act::Quick).unwrap_or(false);
        widgets::panel_chrome(
            ui,
            &p,
            "Calendar",
            "calendar",
            |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                refresh = widgets::icon_btn(ui, &p, "arrow.clockwise", 26.0, false).on_hover_text("Refresh").clicked();
                newev = widgets::icon_btn(ui, &p, "plus", 26.0, false).on_hover_text("New event").clicked();
                today_b = widgets::button_ex(ui, &p, BtnKind::Ghost, true, true, "Today").clicked();
                next = widgets::icon_btn(ui, &p, "chevron.right", 26.0, false).clicked();
                ui.add_sized(
                    [120.0, 20.0],
                    egui::Label::new(ty::sized(title, 12.0, W::Medium).color(p.ink)).selectable(false),
                );
                prev = widgets::icon_btn(ui, &p, "chevron.left", 26.0, false).clicked();
                for (i, (v, label)) in [(View::Agenda, "Agenda"), (View::Week, "Week"), (View::Month, "Month")].into_iter().enumerate() {
                    if i == 1 {
                        ui.spacing_mut().item_spacing.x = 2.0;
                    }
                    let kind = if view == v { BtnKind::Secondary } else { BtnKind::Ghost };
                    if widgets::button_ex(ui, &p, kind, true, true, label).clicked() {
                        view = v;
                    }
                }
            },
            |ui| self.content(ui, sh, &p, busy),
        );

        let mut dirty = false;
        if view != self.view {
            self.view = view;
            self.week_scrolled = false;
            dirty = true;
        }
        if prev || next {
            self.step(if next { 1 } else { -1 });
            dirty = true;
        }
        if today_b {
            self.anchor = today_days();
            dirty = true;
        }
        if newev {
            self.event_form = Some(self.new_event_form(self.anchor.max(today_days().min(self.anchor)), 9));
        }
        if dirty || refresh {
            self.reload(sh);
        }
        self.event_sheet(ui, sh);
    }

    fn poll(&mut self, sh: &Shared) {
        if let Some(job) = &mut self.status {
            if let Some(result) = job.take() {
                self.caldav_ok = result.ok().and_then(|v| v.get("configured").and_then(Value::as_bool)).unwrap_or(false);
                self.status = None;
            }
        }
        if let Some(job) = &mut self.load {
            if let Some(result) = job.take() {
                match result {
                    Ok(v) => {
                        let arr = v.as_array().cloned().or_else(|| v.get("events").and_then(Value::as_array).cloned()).unwrap_or_default();
                        self.evs = arr.iter().filter_map(Ev::from_value).collect();
                        self.evs.sort_by_key(|e| (e.s, e.e));
                        self.fail = None;
                    }
                    Err(e) => self.fail = Some(e),
                }
                self.loaded = true;
                self.load = None;
            }
        }
        if let Some((kind, job)) = &mut self.action {
            let kind = *kind;
            if let Some(result) = job.take() {
                self.action = None;
                let err = match &result {
                    Ok(v) if v.get("ok").and_then(Value::as_bool) == Some(false) => Some(
                        v.get("detail").or_else(|| v.get("error")).and_then(Value::as_str).unwrap_or("Calendar action failed").to_owned(),
                    ),
                    Ok(_) => None,
                    Err(e) => Some(e.clone()),
                };
                match (kind, err) {
                    (Act::Quick, Some(e)) => self.quick_err = Some(e),
                    (Act::Quick, None) => {
                        self.quick.clear();
                        self.quick_err = None;
                        self.reload(sh);
                    }
                    (Act::Save, Some(e)) => {
                        if let Some(f) = &mut self.event_form {
                            f.error = Some(e);
                        }
                    }
                    (Act::Save, None) => {
                        self.event_form = None;
                        self.reload(sh);
                    }
                    (Act::Delete, Some(e)) => {
                        self.fail = Some(e);
                        self.loaded = true;
                    }
                    (Act::Delete, None) => {
                        self.event_form = None;
                        self.reload(sh);
                    }
                }
            }
        }
    }

    // ---- ranges -----------------------------------------------------------------------

    fn range_start(&self) -> i64 {
        match self.view {
            View::Month => {
                let (y, m, _) = civil_from_days(self.anchor);
                let first = days_from_civil(y, m, 1);
                first - wd(first) as i64
            }
            View::Week => self.anchor - wd(self.anchor) as i64,
            View::Agenda => self.anchor,
        }
    }

    fn range_end(&self) -> i64 {
        self.range_start()
            + match self.view {
                View::Month => 42,
                View::Week => 7,
                View::Agenda => 30,
            }
    }

    fn title_label(&self) -> String {
        match self.view {
            View::Week => {
                let s = self.range_start();
                format!("{} \u{2013} {}", day_month_abbr(s), day_month_abbr(s + 6))
            }
            _ => {
                let (y, m, _) = civil_from_days(self.anchor);
                format!("{} {y}", MONTHS[(m - 1) as usize])
            }
        }
    }

    fn step(&mut self, dir: i64) {
        match self.view {
            View::Month => {
                let (y, m, d) = civil_from_days(self.anchor);
                let idx = y * 12 + (m as i32 - 1) + dir as i32;
                let (ny, nm) = (idx.div_euclid(12), (idx.rem_euclid(12) + 1) as u32);
                self.anchor = days_from_civil(ny, nm, d.min(days_in_month(ny, nm)));
            }
            View::Week => self.anchor += 7 * dir,
            View::Agenda => self.anchor += 30 * dir,
        }
    }

    fn reload(&mut self, sh: &Shared) {
        let (s, e) = (fmt_day(self.range_start()), fmt_day(self.range_end()));
        self.load = Some(sh.spawn(move |a| a.calendar_events(&format!("{s}T00:00:00"), &format!("{e}T00:00:00"))));
    }

    fn new_event_form(&self, day: i64, hour: u32) -> EventForm {
        let date = fmt_day(day);
        EventForm {
            start: format!("{date}T{hour:02}:00"),
            end: format!("{date}T{:02}:00", (hour + 1).min(23)),
            timezone: "Z".into(),
            ..Default::default()
        }
    }

    // ---- content ----------------------------------------------------------------------

    fn content(&mut self, ui: &mut Ui, sh: &Shared, p: &Palette, busy: bool) {
        let rect = ui.max_rect();
        ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
        // Quick-add bar (`QuickAddBar`).
        let bar = Rect::from_min_size(rect.min, vec2(rect.width(), 34.0));
        ui.allocate_rect(bar, Sense::hover());
        let submit = self.quick_bar(ui, p, bar, busy);
        if let Some(text) = submit {
            self.quick_err = None;
            self.action = Some((
                Act::Quick,
                sh.spawn(move |a| {
                    let parsed = a.quick_parse_event(&text)?;
                    if parsed.get("ok").and_then(Value::as_bool) == Some(false) {
                        return Err(parsed
                            .get("detail")
                            .or_else(|| parsed.get("error"))
                            .and_then(Value::as_str)
                            .unwrap_or("Couldn't parse that")
                            .to_owned());
                    }
                    a.create_event(parsed.get("event").cloned().unwrap_or(parsed))
                }),
            ));
        }
        // The surface below.
        let body = Rect::from_min_max(pos2(rect.left(), bar.bottom() + 10.0), rect.max);
        if body.height() < 20.0 {
            return;
        }
        ui.allocate_rect(body, Sense::hover());
        let r = Rounding::same(theme::R_PANEL);
        ui.painter().rect_filled(body, r, p.panel_fill);
        ui.painter().rect_stroke(body.shrink(0.5), r, Stroke::new(1.0, p.line));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        child.set_clip_rect(body.intersect(ui.clip_rect()));
        let ui = &mut child;
        if !self.loaded {
            widgets::panel_loading(ui, p);
            return;
        }
        if let Some(e) = &self.fail {
            widgets::panel_failure(ui, p, "Couldn't load events", Some(e));
            return;
        }
        if self.evs.is_empty() && !self.caldav_ok {
            crate::ascii::empty_state(ui, p, crate::ascii::Art::Plug, "Connect a calendar in Settings");
            return;
        }
        let pick = match self.view {
            View::Month => self.month_grid(ui, p, body),
            View::Week => self.week_view(ui, p, body),
            View::Agenda => self.agenda_view(ui, sh, p),
        };
        match pick {
            Some(Pick::Edit(i)) => self.event_form = Some(EventForm::from_value(&self.evs[i].raw)),
            Some(Pick::New(day)) => self.event_form = Some(self.new_event_form(day, 9)),
            Some(Pick::Delete(i)) => {
                let uid = self.evs[i].uid.clone();
                self.action = Some((Act::Delete, sh.spawn(move |a| a.delete_event(&uid))));
            }
            None => {}
        }
    }

    fn quick_bar(&mut self, ui: &mut Ui, p: &Palette, bar: Rect, busy: bool) -> Option<String> {
        let id = ui.id().with("quick-add");
        let focused = ui.memory(|m| m.has_focus(id));
        let r = Rounding::same(theme::R_CTRL);
        let line = if focused { p.accent_a(0.7) } else { p.line };
        ui.painter().rect_filled(bar, r, p.raised);
        ui.painter().rect_stroke(bar.shrink(0.5), r, Stroke::new(1.0, line));
        let ready = !self.quick.trim().is_empty();
        let mut fire = false;
        let inner = Rect::from_min_max(pos2(bar.left() + 12.0, bar.top()), pos2(bar.right() - 6.0, bar.bottom()));
        let mut child = ui.new_child(
            egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 8.0;
        if busy {
            crate::ascii::spinner(ui, 12.0, p.ink2);
        } else if let Some(e) = &self.quick_err {
            ui.label(ty::caption(e.as_str()).color(p.danger));
        } else {
            let resp = widgets::icon_btn(ui, p, "plus.circle", 24.0, ready);
            if resp.clicked() && ready {
                fire = true;
            }
            if focused && ready {
                widgets::kbd(ui, p, "\u{21a9}");
            }
        }
        let te = ui.add_enabled(
            !busy,
            egui::TextEdit::singleline(&mut self.quick)
                .id(id)
                .frame(false)
                .desired_width(ui.available_width())
                .hint_text(ty::sized("Lunch with Sara friday 1pm", 13.0, W::Regular).color(p.ink3))
                .font(theme::jb(13.0, W::Regular))
                .text_color(p.ink),
        );
        if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && ready {
            fire = true;
        }
        (fire && !busy).then(|| self.quick.trim().to_owned())
    }

    /// Events overlapping each of `n` days from `start`.
    fn bucket(&self, start: i64, n: usize) -> Vec<Vec<usize>> {
        let mut out = vec![Vec::new(); n];
        for (i, ev) in self.evs.iter().enumerate() {
            for (k, slot) in out.iter_mut().enumerate() {
                let day = (start + k as i64) * 1440;
                if ev.s < day + 1440 && ev.e > day {
                    slot.push(i);
                }
            }
        }
        out
    }

    // ---- month ------------------------------------------------------------------------

    fn month_grid(&self, ui: &mut Ui, p: &Palette, rect: Rect) -> Option<Pick> {
        let start = self.range_start();
        let by_day = self.bucket(start, 42);
        let (pad, head) = (8.0, 26.0);
        let gw = (rect.width() - pad * 2.0).max(1.0);
        let gh = (rect.height() - pad * 2.0 - head).max(1.0);
        let (cw, ch) = (gw / 7.0, gh / 6.0);
        let origin = rect.min + vec2(pad, pad);
        let grid = pos2(origin.x, origin.y + head);
        let painter = ui.painter().clone();
        let mut pick = None;
        for (i, w) in WEEKDAYS.iter().enumerate() {
            painter.text(
                pos2(origin.x + cw * (i as f32 + 0.5), origin.y + head / 2.0),
                Align2::CENTER_CENTER,
                *w,
                theme::jb(12.0, W::Semibold),
                p.ink2,
            );
        }
        let stroke = Stroke::new(1.0, p.line);
        for c in 0..=7 {
            let x = (grid.x + cw * c as f32).round() + 0.5;
            painter.vline(x, grid.y..=grid.y + ch * 6.0, stroke);
        }
        for r in 0..=6 {
            let y = (grid.y + ch * r as f32).round() + 0.5;
            painter.hline(grid.x..=grid.x + cw * 7.0, y, stroke);
        }
        let (_, anchor_m, _) = civil_from_days(self.anchor);
        let today = today_days();
        for idx in 0..42usize {
            let day = start + idx as i64;
            let cell = Rect::from_min_size(pos2(grid.x + cw * (idx % 7) as f32, grid.y + ch * (idx / 7) as f32), vec2(cw, ch));
            let resp = ui.interact(cell, ui.id().with(("mcell", idx)), Sense::click());
            if resp.double_clicked() {
                pick = Some(Pick::New(day));
            }
            let clip = painter.with_clip_rect(cell.intersect(ui.clip_rect()));
            let (_, m, d) = civil_from_days(day);
            let is_today = day == today;
            let in_month = m == anchor_m;
            let c = pos2(cell.left() + 3.0 + 10.0, cell.top() + 3.0 + 10.0);
            if is_today {
                clip.circle_filled(c, 10.0, p.accent);
            }
            clip.text(
                c,
                Align2::CENTER_CENTER,
                d.to_string(),
                theme::jb(11.0, if is_today { W::Semibold } else { W::Regular }),
                if is_today { p.on_accent } else if in_month { p.ink } else { theme::with_alpha(p.ink3, 0.7) },
            );
            let evs = &by_day[idx];
            let mut y = cell.top() + 3.0 + 22.0 + 2.0;
            for &i in evs.iter().take(3) {
                let ev = &self.evs[i];
                let chip = Rect::from_min_size(pos2(cell.left() + 3.0, y), vec2(cw - 6.0, 16.0));
                let strong = ev.all_day || ev.multi_day();
                clip.rect_filled(chip, Rounding::same(4.0), p.accent_a(if strong { 0.24 } else { 0.12 }));
                clip.rect_stroke(chip.shrink(0.5), Rounding::same(4.0), Stroke::new(1.0, p.accent_a(0.25)));
                let font = theme::jb(10.0, W::Medium);
                let text = fit(ui, &ev.summary, &font, chip.width() - 8.0);
                clip.text(pos2(chip.left() + 4.0, chip.center().y), Align2::LEFT_CENTER, text, font, p.ink);
                let cr = ui.interact(chip.intersect(cell), ui.id().with(("mchip", idx, i)), Sense::click());
                if cr.clicked() {
                    pick = Some(Pick::Edit(i));
                }
                cr.on_hover_cursor(egui::CursorIcon::PointingHand);
                y += 18.0;
            }
            if evs.len() > 3 {
                clip.text(
                    pos2(cell.left() + 3.0, y + 6.0),
                    Align2::LEFT_CENTER,
                    format!("+{} more", evs.len() - 3),
                    theme::jb(9.0, W::Regular),
                    p.ink3,
                );
            }
        }
        pick
    }

    // ---- week -------------------------------------------------------------------------

    fn week_view(&mut self, ui: &mut Ui, p: &Palette, rect: Rect) -> Option<Pick> {
        let start = self.range_start();
        let by_day = self.bucket(start, 7);
        let today = today_days();
        let gutter = 44.0;
        let cw = ((rect.width() - gutter - 8.0) / 7.0).max(1.0);
        let mut pick = None;
        // Header: weekday, day number, up to two all-day chips per day.
        let mut head_h = 0.0f32;
        for k in 0..7 {
            let n = by_day[k].iter().filter(|&&i| self.evs[i].all_day).count().min(2);
            head_h = head_h.max(12.0 + 3.0 + 24.0 + n as f32 * 21.0);
        }
        let head_h = head_h + 16.0;
        let painter = ui.painter().clone();
        for k in 0..7usize {
            let day = start + k as i64;
            let x0 = rect.left() + gutter + cw * k as f32;
            let cx = x0 + cw / 2.0;
            let mut y = rect.top() + 8.0;
            painter.text(pos2(cx, y + 7.0), Align2::CENTER_CENTER, WEEKDAYS[k], theme::jb(12.0, W::Semibold), p.ink2);
            y += 12.0 + 3.0;
            let is_today = day == today;
            let (_, _, d) = civil_from_days(day);
            if is_today {
                painter.circle_filled(pos2(cx, y + 12.0), 12.0, p.accent);
            }
            painter.text(
                pos2(cx, y + 12.0),
                Align2::CENTER_CENTER,
                d.to_string(),
                theme::jb(13.0, if is_today { W::Semibold } else { W::Regular }),
                if is_today { p.on_accent } else { p.ink },
            );
            y += 24.0 + 3.0;
            for &i in by_day[k].iter().filter(|&&i| self.evs[i].all_day).take(2) {
                let chip = Rect::from_min_size(pos2(x0 + 2.0, y), vec2(cw - 4.0, 18.0));
                painter.rect_filled(chip, Rounding::same(5.0), p.accent_a(0.22));
                painter.rect_stroke(chip.shrink(0.5), Rounding::same(5.0), Stroke::new(1.0, p.accent_a(0.3)));
                let font = theme::jb(9.5, W::Medium);
                let text = fit(ui, &self.evs[i].summary, &font, chip.width() - 10.0);
                painter.text(chip.center(), Align2::CENTER_CENTER, text, font, p.ink);
                if ui.interact(chip, ui.id().with(("wad", k, i)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    pick = Some(Pick::Edit(i));
                }
                y += 21.0;
            }
        }
        let line_y = rect.top() + head_h;
        painter.hline(rect.x_range(), line_y, Stroke::new(1.0, p.line));

        // Scrollable time grid, opens at 08:00.
        let body = Rect::from_min_max(pos2(rect.left(), line_y + 1.0), rect.max);
        let total_h = HOUR_H * (24 - FIRST_HOUR) as f32;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        child.set_clip_rect(body.intersect(ui.clip_rect()));
        let mut area = egui::ScrollArea::vertical().id_salt("cal-week").auto_shrink([false; 2]);
        if !self.week_scrolled {
            area = area.vertical_scroll_offset(HOUR_H * 2.0);
            self.week_scrolled = true;
        }
        area.show(&mut child, |ui| {
            let (grid, _) = ui.allocate_exact_size(vec2(body.width(), total_h), Sense::hover());
            let painter = ui.painter().clone();
            for h in FIRST_HOUR..24 {
                let y = grid.top() + (h - FIRST_HOUR) as f32 * HOUR_H;
                painter.text(
                    pos2(grid.left() + 40.0, y),
                    Align2::RIGHT_CENTER,
                    format!("{h:02}:00"),
                    theme::jb(9.5, W::Regular),
                    p.ink3,
                );
            }
            let now = now_minutes();
            for k in 0..7usize {
                let day = start + k as i64;
                let col = Rect::from_min_size(pos2(grid.left() + gutter + cw * k as f32, grid.top()), vec2(cw, total_h));
                let clip = painter.with_clip_rect(col.intersect(ui.clip_rect()));
                if day == today {
                    clip.rect_filled(col, 0.0, p.accent_a(0.05));
                }
                for h in 0..(24 - FIRST_HOUR) {
                    let y = (col.top() + h as f32 * HOUR_H).round() + 0.5;
                    clip.hline(col.x_range(), y, Stroke::new(1.0, p.line));
                }
                clip.vline(col.left() + 0.5, col.y_range(), Stroke::new(1.0, p.line));
                let grid_start = day * 1440 + FIRST_HOUR * 60;
                if day == today {
                    let mins = (now - grid_start) as f32;
                    if mins > 0.0 {
                        let y = col.top() + mins / 60.0 * HOUR_H;
                        clip.rect_filled(Rect::from_min_size(pos2(col.left(), y - 0.75), vec2(cw, 1.5)), 0.0, p.accent);
                        clip.circle_filled(pos2(col.left(), y), 3.0, p.accent);
                    }
                }
                for &i in by_day[k].iter().filter(|&&i| !self.evs[i].all_day) {
                    let ev = &self.evs[i];
                    let s = ev.s.max(grid_start);
                    let e = ev.e.min((day + 1) * 1440);
                    let y = col.top() + (s - grid_start) as f32 / 60.0 * HOUR_H;
                    let h = ((e - s) as f32 / 60.0 * HOUR_H).max(18.0);
                    let r = Rect::from_min_size(pos2(col.left() + 2.0, y), vec2(cw - 4.0, h));
                    let rr = Rounding::same(6.0);
                    clip.rect_filled(r, rr, p.accent_a(0.14));
                    clip.rect_stroke(r.shrink(0.5), rr, Stroke::new(1.0, p.accent_a(0.28)));
                    clip.rect_filled(
                        Rect::from_min_size(r.min, vec2(2.5, r.height())),
                        Rounding { nw: 6.0, sw: 6.0, ne: 0.0, se: 0.0 },
                        p.accent,
                    );
                    let inner = clip.with_clip_rect(r.intersect(col));
                    let g = ui.painter().layout(
                        ev.summary.clone(),
                        theme::jb(10.5, W::Medium),
                        p.ink,
                        r.width() - 10.0,
                    );
                    let max_rows = if h > 34.0 { 2 } else { 1 };
                    let shown = g.rows.iter().take(max_rows).map(|r| r.rect.height()).sum::<f32>();
                    let text_pos = pos2(r.left() + 5.0, r.top() + 3.0);
                    inner.with_clip_rect(Rect::from_min_size(text_pos, vec2(r.width() - 8.0, shown.max(12.0)))
                        .intersect(r))
                        .galley(text_pos, g, p.ink);
                    if h > 30.0 {
                        inner.text(
                            pos2(r.left() + 5.0, r.top() + 3.0 + shown + 1.0),
                            Align2::LEFT_TOP,
                            hhmm(s),
                            theme::jb(9.5, W::Regular),
                            p.ink2,
                        );
                    }
                    let resp = ui.interact(r.intersect(col), ui.id().with(("wev", k, i)), Sense::click());
                    if resp.clicked() {
                        pick = Some(Pick::Edit(i));
                    }
                    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                }
                let colr = ui.interact(col, ui.id().with(("wcol", k)), Sense::click());
                if colr.double_clicked() {
                    pick = Some(Pick::New(day));
                }
            }
        });
        pick
    }

    // ---- agenda -----------------------------------------------------------------------

    fn agenda_view(&mut self, ui: &mut Ui, _sh: &Shared, p: &Palette) -> Option<Pick> {
        let mut pick = None;
        let today = today_days();
        let mut days: Vec<i64> = self.evs.iter().map(|e| e.s.div_euclid(1440)).collect();
        days.sort_unstable();
        days.dedup();
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::Frame::none().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if days.is_empty() {
                    ui.allocate_ui(vec2(ui.available_width(), 240.0), |ui| {
                        crate::ascii::empty_state(ui, p, crate::ascii::Art::Done, "No events");
                    });
                }
                for (n, &day) in days.iter().enumerate() {
                    if n > 0 {
                        ui.add_space(14.0);
                    }
                    let label = day_label(day, today).to_uppercase();
                    ui.label(
                        ty::sized(label, 11.0, W::Medium)
                            .extra_letter_spacing(0.6)
                            .color(if day == today { p.accent } else { p.ink3 }),
                    );
                    ui.add_space(6.0);
                    for (i, ev) in self.evs.iter().enumerate().filter(|(_, e)| e.s.div_euclid(1440) == day) {
                        let resp = widgets::hover_row(ui, p, false, theme::R_CTRL, |ui| {
                            ui.set_width(ui.available_width());
                            egui::Frame::none().inner_margin(egui::Margin::symmetric(10.0, 6.0)).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 10.0;
                                    let (bar, _) = ui.allocate_exact_size(vec2(3.0, 30.0), Sense::hover());
                                    ui.painter().rect_filled(bar, Rounding::same(1.5), p.accent);
                                    ui.vertical(|ui| {
                                        ui.spacing_mut().item_spacing.y = 1.0;
                                        ui.label(ty::sized(ev.summary.as_str(), 13.0, W::Medium).color(p.ink));
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 6.0;
                                            let when = if ev.all_day {
                                                if ev.multi_day() { "All day \u{b7} multi-day".to_string() } else { "All day".to_string() }
                                            } else {
                                                format!("{}\u{2013}{}", hhmm(ev.s), hhmm(ev.e))
                                            };
                                            ui.label(ty::sized(when, 11.0, W::Regular).color(p.ink2));
                                            if !ev.location.is_empty() {
                                                icons::symbol(ui, "mappin", 11.0, p.ink2);
                                                ui.add(egui::Label::new(ty::sized(ev.location.as_str(), 11.0, W::Regular).color(p.ink2)).truncate());
                                            }
                                        });
                                    });
                                });
                            });
                        });
                        if resp.clicked() {
                            pick = Some(Pick::Edit(i));
                        }
                        resp.context_menu(|ui| {
                            widgets::floating_frame(p, theme::R_CARD).inner_margin(egui::Margin::same(6.0)).show(ui, |ui| {
                                ui.set_min_width(140.0);
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if widgets::menu_item(ui, p, Some("pencil"), "Edit", false, true).clicked() {
                                    pick = Some(Pick::Edit(i));
                                    ui.close_menu();
                                }
                                if widgets::menu_item(ui, p, Some("trash"), "Delete", false, true).clicked() {
                                    pick = Some(Pick::Delete(i));
                                    ui.close_menu();
                                }
                            });
                        });
                    }
                }
            });
        });
        pick
    }

    // ---- editor sheet (`EventEditorSheet`) --------------------------------------------

    fn event_sheet(&mut self, ui: &Ui, sh: &Shared) {
        let Some(form) = &mut self.event_form else { return };
        let p = sh.palette;
        let ctx = ui.ctx().clone();
        let saving = self.action.as_ref().map(|(a, _)| *a != Act::Quick).unwrap_or(false);
        let (mut cancel, mut save, mut delete) = (false, false, false);
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        // Dim scrim.
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("cal-scrim")).order(egui::Order::Middle).fixed_pos(screen.min).show(&ctx, |ui| {
            ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
            ui.allocate_rect(screen, Sense::click());
        });
        egui::Window::new("cal-event-sheet")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(440.0, 440.0))
            .frame(widgets::floating_frame(&p, theme::R_PANEL).inner_margin(egui::Margin::ZERO))
            .show(&ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                egui::Frame::none().inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(ty::sized(if form.uid.is_none() { "New event" } else { "Edit event" }, 14.0, W::Semibold).color(p.ink));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let ok = !saving && !form.summary.trim().is_empty();
                            if widgets::button_ex(ui, &p, BtnKind::Primary, false, ok, if form.uid.is_none() { "Add" } else { "Save" }).clicked() {
                                save = true;
                            }
                            if widgets::button_ex(ui, &p, BtnKind::Ghost, false, true, "Cancel").clicked() {
                                cancel = true;
                            }
                            if form.uid.is_some() && widgets::icon_btn(ui, &p, "trash", 26.0, false).clicked() {
                                delete = true;
                            }
                        });
                    });
                });
                let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                ui.painter().rect_filled(line, 0.0, p.line);
                egui::Frame::none().inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    let label = |ui: &mut Ui, t: &str| {
                        ui.add_sized([70.0, 28.0], egui::Label::new(ty::sized(t, 13.0, W::Regular).color(p.ink2)).selectable(false));
                    };
                    ui.horizontal(|ui| {
                        label(ui, "Title");
                        widgets::field(ui, &p, &mut form.summary, "Title");
                    });
                    ui.horizontal(|ui| {
                        label(ui, "All day");
                        let mut on = form.all_day;
                        if widgets::jb_switch(ui, &p, &mut on).changed() {
                            form.all_day = on;
                            let sd: String = form.start.chars().take(10).collect();
                            let ed: String = form.end.chars().take(10).collect();
                            if on {
                                form.end = if ed == sd { next_date(&sd).unwrap_or(ed) } else { ed };
                                form.start = sd;
                            } else {
                                form.start = format!("{sd}T09:00");
                                form.end = format!("{ed}T10:00");
                            }
                        }
                    });
                    let (ph_s, ph_e) = if form.all_day { ("2026-10-09", "2026-10-10") } else { ("2026-10-09T09:00", "2026-10-09T10:00") };
                    ui.horizontal(|ui| {
                        label(ui, "Starts");
                        widgets::field(ui, &p, &mut form.start, ph_s);
                    });
                    ui.horizontal(|ui| {
                        label(ui, "Ends");
                        widgets::field(ui, &p, &mut form.end, ph_e);
                    });
                    if !form.all_day {
                        ui.horizontal(|ui| {
                            label(ui, "Timezone");
                            widgets::field(ui, &p, &mut form.timezone, "Z or +02:00");
                        });
                    }
                    ui.horizontal(|ui| {
                        label(ui, "Location");
                        widgets::field(ui, &p, &mut form.location, "Location");
                    });
                    ui.horizontal_top(|ui| {
                        label(ui, "Notes");
                        widgets::field_frame(&p, false).show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut form.description)
                                    .frame(false)
                                    .desired_rows(3)
                                    .desired_width(f32::INFINITY)
                                    .hint_text(ty::sized("Notes", 13.0, W::Regular).color(p.ink3))
                                    .font(theme::jb(13.0, W::Regular)),
                            );
                        });
                    });
                    if let Some(e) = &form.error {
                        ui.label(ty::caption(e.as_str()).color(p.danger));
                    }
                });
            });
        if cancel {
            self.event_form = None;
        } else if delete {
            if let Some(uid) = form.uid.clone() {
                self.action = Some((Act::Delete, sh.spawn(move |a| a.delete_event(&uid))));
            }
        } else if save {
            let summary = form.summary.trim().to_owned();
            let start = form.start.trim().to_owned();
            let end = form.end.trim().to_owned();
            let timezone = form.timezone.trim().to_owned();
            let dtstart = if form.all_day { valid_date(&start).then(|| format!("{start}T00:00:00")) } else { with_timezone(&start, &timezone) };
            let dtend = if form.all_day {
                let ex = if start == end { next_date(&start).unwrap_or_default() } else { end.clone() };
                valid_date(&ex).then(|| format!("{ex}T00:00:00"))
            } else {
                with_timezone(&end, &timezone)
            };
            if summary.is_empty() || dtstart.is_none() || dtend.is_none() || (!form.all_day && end <= start) {
                form.error = Some("Enter a title and valid start/end times. End must be after start; timezone must be Z or \u{b1}HH:MM.".into());
            } else {
                let payload = serde_json::json!({"summary": summary, "dtstart": dtstart.unwrap(), "dtend": dtend.unwrap(),
                    "all_day": form.all_day, "location": form.location, "description": form.description});
                form.error = None;
                let job = match form.uid.clone() {
                    Some(uid) => sh.spawn(move |a| a.update_event(&uid, payload)),
                    None => sh.spawn(move |a| a.create_event(payload)),
                };
                self.action = Some((Act::Save, job));
            }
        }
    }

    #[cfg(feature = "snapshot")]
    pub fn demo_agenda(&mut self) {
        self.anchor = days_from_civil(2026, 11, 18);
        self.view = View::Agenda;
    }

    #[cfg(feature = "snapshot")]
    pub fn demo_week(&mut self) {
        self.anchor = days_from_civil(2026, 11, 24);
        self.view = View::Week;
    }

    #[cfg(feature = "snapshot")]
    pub fn demo_month(&mut self) {
        self.anchor = days_from_civil(2026, 11, 24);
    }
}

// ---- helpers ------------------------------------------------------------------------

/// Truncate `text` with an ellipsis so it fits `max_w`.
fn fit(ui: &Ui, text: &str, font: &FontId, max_w: f32) -> String {
    let w = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE).size().x;
    if w(text) <= max_w {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for n in (0..chars.len()).rev() {
        let s: String = chars[..n].iter().collect::<String>().trim_end().to_owned() + "\u{2026}";
        if w(&s) <= max_w {
            return s;
        }
    }
    "\u{2026}".into()
}

fn parse_min(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<i64>().ok());
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let (hh, mm) = if s.len() >= 16 { (num(11, 13).unwrap_or(0), num(14, 16).unwrap_or(0)) } else { (0, 0) };
    Some(days_from_civil(y as i32, m as u32, d as u32) * 1440 + hh * 60 + mm)
}

fn hhmm(minutes: i64) -> String {
    let m = minutes.rem_euclid(1440);
    format!("{:02}:{:02}", m / 60, m % 60)
}

fn fmt_day(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn day_month_abbr(days: i64) -> String {
    let (_, m, d) = civil_from_days(days);
    format!("{d} {}", &MONTHS[(m - 1) as usize][..3])
}

fn day_label(day: i64, today: i64) -> String {
    if day == today {
        return "Today".into();
    }
    if day == today + 1 {
        return "Tomorrow".into();
    }
    const LONG: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let (_, m, d) = civil_from_days(day);
    format!("{} {d} {}", LONG[wd(day) as usize], MONTHS[(m - 1) as usize])
}

/// Weekday of a day number, 0 = Monday.
fn wd(days: i64) -> u32 {
    (days + 3).rem_euclid(7) as u32
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn today_days() -> i64 {
    now_secs().div_euclid(86400)
}

fn now_minutes() -> i64 {
    now_secs().div_euclid(60)
}

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December",
];

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

impl EventForm {
    fn from_value(v: &Value) -> Self {
        let (start, start_tz) =
            split_datetime(v.get("dtstart").and_then(Value::as_str).unwrap_or(""));
        let (end, end_tz) = split_datetime(v.get("dtend").and_then(Value::as_str).unwrap_or(""));
        Self {
            uid: v.get("uid").and_then(Value::as_str).map(str::to_owned),
            summary: v
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            start,
            end,
            timezone: if !start_tz.is_empty() {
                start_tz
            } else {
                end_tz
            },
            all_day: v.get("all_day").and_then(Value::as_bool).unwrap_or(false),
            location: v
                .get("location")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            description: v
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            error: None,
        }
    }
}

fn split_datetime(s: &str) -> (String, String) {
    let text = s.replace(' ', "T");
    let local = text.chars().take(16).collect::<String>();
    let tz = if text.ends_with('Z') {
        "Z".to_owned()
    } else if text.len() >= 6 && matches!(text.as_bytes()[text.len() - 6], b'+' | b'-') {
        text[text.len() - 6..].to_owned()
    } else {
        "Z".to_owned()
    };
    (local, tz)
}

fn with_timezone(local: &str, timezone: &str) -> Option<String> {
    let b = local.as_bytes();
    if b.len() != 16
        || b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || b.get(10) != Some(&b'T')
        || b.get(13) != Some(&b':')
        || ![0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15]
            .iter()
            .all(|i| b[*i].is_ascii_digit())
    {
        return None;
    }
    if !valid_date(&local[..10])
        || local[11..13].parse::<u8>().ok()? > 23
        || local[14..16].parse::<u8>().ok()? > 59
    {
        return None;
    }
    let valid_tz = timezone == "Z"
        || (timezone.len() == 6
            && matches!(timezone.as_bytes()[0], b'+' | b'-')
            && timezone.as_bytes()[3] == b':'
            && timezone.as_bytes()[1..3]
                .iter()
                .chain(timezone.as_bytes()[4..6].iter())
                .all(u8::is_ascii_digit)
            && timezone[1..3].parse::<u8>().ok()? <= 23
            && timezone[4..6].parse::<u8>().ok()? <= 59);
    if !valid_tz {
        return None;
    }
    Some(format!("{local}:00{timezone}"))
}

fn valid_date(date: &str) -> bool {
    let b = date.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || ![0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|i| b[*i].is_ascii_digit())
    {
        return false;
    }
    let year = date[0..4].parse::<u32>().unwrap_or(0);
    let month = date[5..7].parse::<u32>().unwrap_or(0);
    let day = date[8..10].parse::<u32>().unwrap_or(0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => return false,
    };
    day >= 1 && day <= days
}

fn next_date(date: &str) -> Option<String> {
    if !valid_date(date) {
        return None;
    }
    let year = date[0..4].parse::<u32>().ok()?;
    let month = date[5..7].parse::<u32>().ok()?;
    let day = date[8..10].parse::<u32>().ok()?;
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => return None,
    };
    Some(if day < max {
        format!("{year:04}-{month:02}-{:02}", day + 1)
    } else if month < 12 {
        format!("{year:04}-{:02}-01", month + 1)
    } else {
        format!("{:04}-01-01", year + 1)
    })
}

