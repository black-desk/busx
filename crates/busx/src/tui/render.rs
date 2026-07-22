// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure rendering. Reads `&State`; draws breadcrumb + top screen
//! + key-hint. Nothing else.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::tui::copy::Tool;
use crate::tui::state::{
    ActionKind, ActionResult, ClickTarget, CopyAsPopup, DetailFocus, DetailScreen, InterfaceFocus,
    ListenTarget, NavContext, ResultScreen, Screen, ServiceScreen, State,
};

/// `scroll` carries the persisted list-scroll offsets for the *top* screen's
/// list(s), threaded in/out across frames. Slot 0 is the single list on the
/// Service/Objects/Interfaces screens; slots 0/1/2 are the methods/properties/
/// signals columns on the Interface screen. The app loop owns it (like
/// `targets`) and resets it to `[0; 3]` whenever the navigation stack depth
/// changes, so a freshly entered screen starts at the top.
///
/// Without this, each frame builds a fresh `ListState` (offset 0) and ratatui
/// re-anchors the selected item to the *bottom* of the viewport — so after
/// scrolling down, moving the cursor back up keeps the highlight glued to the
/// last row. Seeding `with_offset` from the persisted value lets ratatui keep
/// the cursor stable (vim/less-style: the viewport only scrolls once the cursor
/// reaches an edge).
pub fn render(
    frame: &mut Frame,
    state: &State,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut [usize; 3],
) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    let (crumb, main, footer) = (chunks[0], chunks[1], chunks[2]);

    render_breadcrumb(frame, crumb, state);
    // When the `/` filter is active on a list screen, carve a one-line input box
    // out of the bottom of `main` so the list shrinks by a row to host it.
    let query = state
        .filter
        .as_ref()
        .map(|f| f.value().to_string())
        .unwrap_or_default();
    let (list_area, filter_area) = if state.filter.is_some() {
        let sub = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(main);
        (sub[0], Some(sub[1]))
    } else {
        (main, None)
    };
    match state.top() {
        Screen::Service(s) => render_service(frame, list_area, s, &query, targets, scroll),
        Screen::Objects(o) => render_objects(frame, list_area, o, &query, targets, scroll),
        Screen::Interfaces(i) => render_interfaces(frame, list_area, i, &query, targets, scroll),
        Screen::Interface(i) => render_interface(frame, list_area, i, targets, scroll),
        Screen::Detail(d) => render_detail(frame, list_area, d, &state.nav, targets),
        Screen::Result(r) => render_result(frame, list_area, r),
    }
    render_keyhint(frame, footer, state.top());

    if let (Some(input), Some(area)) = (state.filter.as_ref(), filter_area) {
        render_filter_box(frame, area, input);
    }

    // The copy-as popup overlays the whole frame when open. Drawn last so it sits
    // on top of the screen + keyhint; Clear wipes the underlying area first.
    if let Some(popup) = &state.popup {
        // Clear the whole frame so the popup reads as a true modal — no
        // underlying screen content bleeds in around the centered popup
        // area. Matches the lazygit/htop/bottom convention for modal
        // overlays.
        frame.render_widget(Clear, area);
        render_popup(frame, area, popup, targets);
    }
    // The help overlay sits on top of everything (above the popup too, though the
    // two can't be open at once — help can't open while the popup is up). Not
    // clickable, so it records no click targets.
    if state.help_open {
        // Same modal treatment as the copy-as popup above.
        frame.render_widget(Clear, area);
        render_help(frame, area);
    }
}

fn render_breadcrumb(frame: &mut Frame, area: Rect, state: &State) {
    let mut parts: Vec<String> = Vec::new();
    // The drill path comes from the single source of truth (`nav`), one crumb
    // per level the user has descended — so the breadcrumb reads
    // `service > object > interface > call Method > Result` without any screen
    // re-stating its parent's context.
    let nav = &state.nav;
    if !nav.service.is_empty() {
        parts.push(nav.service.clone());
    }
    if !nav.object.is_empty() {
        parts.push(nav.object.clone());
    }
    if !nav.interface.is_empty() {
        parts.push(nav.interface.clone());
    }
    // The action screens (Detail/Result) layer their own label on top.
    for screen in state.screens() {
        if let Some(label) = screen_action_label(screen) {
            parts.push(label);
        }
    }
    let text = parts.join(" > ");
    frame.render_widget(Paragraph::new(text), area);
}

/// The label an action screen contributes to the breadcrumb: Detail → its
/// action, Result → "Result". The context screens (Service/Objects/Interfaces/
/// Interface) contribute via `State::nav` instead, so they return `None` here.
fn screen_action_label(s: &Screen) -> Option<String> {
    match s {
        Screen::Detail(d) => Some(action_label(&d.kind)),
        Screen::Result(_) => Some("Result".to_string()),
        _ => None,
    }
}

/// The member a listen targets (signal/method member, or property name).
fn listen_member(target: &ListenTarget) -> String {
    match target {
        ListenTarget::Signal { member } | ListenTarget::Method { member } => member.clone(),
        ListenTarget::Property { property } => property.clone(),
    }
}

/// Short (unqualified) action label for the breadcrumb Detail crumb — the
/// interface is already the preceding crumb, so `call ListUnits` rather than
/// `call org.busx.Test.ListUnits`.
fn action_label(kind: &ActionKind) -> String {
    match kind {
        ActionKind::Call { method, .. } => format!("call {method}"),
        ActionKind::Get { property } => format!("get {property}"),
        ActionKind::Set { property, .. } => format!("set {property}"),
        ActionKind::Listen { target } => format!("listen {}", listen_member(target)),
    }
}

/// Interface-qualified action label for the Detail *block title* — it reads
/// standalone, outside the breadcrumb path: `call org.busx.Test.ListUnits`,
/// `get org.busx.Test.volume`, `listen org.busx.Test.Changed`.
fn action_title(kind: &ActionKind, iface: &str) -> String {
    match kind {
        ActionKind::Call { method, .. } => format!("call {iface}.{method}"),
        ActionKind::Get { property } => format!("get {iface}.{property}"),
        ActionKind::Set { property, .. } => format!("set {iface}.{property}"),
        ActionKind::Listen { target } => format!("listen {iface}.{}", listen_member(target)),
    }
}

/// Truncate `s` to `cap` display columns, appending `…` when longer — so a long
/// service name doesn't blow past its column and misalign the row.
fn truncate(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        s.to_string()
    } else {
        let head: String = s.chars().take(cap.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

/// Left-align `s` in exactly `width` display columns: truncate (with `…`) if
/// longer, pad with spaces if shorter. Used to align table-like columns (the
/// member name/signature columns on the Interface screen, the arg labels on the
/// Detail screen).
fn pad_col(s: &str, width: usize) -> String {
    format!("{:<width$}", truncate(s, width))
}

/// Build `name  signature` rows with the name column padded to the widest
/// member (capped to the column's inner width) so signatures line up. Shared by
/// the methods and signals columns.
fn member_rows(rows: &[(String, String)], inner_w: usize) -> Vec<ListItem<'_>> {
    let sig_w = rows
        .iter()
        .map(|(_, s)| s.chars().count())
        .max()
        .unwrap_or(0);
    let name_w = rows
        .iter()
        .map(|(n, _)| n.chars().count())
        .max()
        .unwrap_or(0)
        .min(inner_w.saturating_sub(sig_w + 2));
    rows.iter()
        .map(|(n, s)| ListItem::new(Line::from(format!("{}  {}", pad_col(n, name_w), s))))
        .collect()
}

/// The one-line `/` filter input at the bottom of a list screen. A yellow `/`
/// prompt, the query, and a block cursor at the input's cursor position (a
/// reversed cell — the next char under the cursor, or a trailing space when the
/// cursor sits past the end). The list above shrinks by one row to host this.
fn render_filter_box(frame: &mut Frame, area: Rect, input: &tui_input::Input) {
    let value = input.value();
    let cursor = input.cursor().min(value.len());
    let (before, after) = value.split_at(cursor);
    let rev = Style::default().add_modifier(Modifier::REVERSED);
    let prompt = Style::default().fg(Color::Yellow);

    let mut spans = vec![Span::styled("/ ", prompt), Span::raw(before.to_string())];
    // Block cursor: the first char of `after` (reversed), or a reversed space
    // when the cursor is past the last char.
    let mut rest = "";
    match after.chars().next() {
        Some(_) => {
            let end = after
                .char_indices()
                .nth(1)
                .map(|(i, _)| i)
                .unwrap_or(after.len());
            let (cur, tail) = after.split_at(end);
            spans.push(Span::styled(cur.to_string(), rev));
            rest = tail;
        }
        None => spans.push(Span::styled(" ", rev)),
    }
    spans.push(Span::raw(rest.to_string()));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_service(
    frame: &mut Frame,
    area: Rect,
    s: &ServiceScreen,
    query: &str,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut [usize; 3],
) {
    let title = if s.loading {
        "Services (loading…)"
    } else {
        "Services"
    };
    let block = Block::default().borders(Borders::ALL).title(title);

    if let Some(err) = &s.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")).block(block), area);
        return;
    }

    // The filtered view (raw indices); empty query ⇒ all rows.
    let view = crate::tui::state::filter_view(&s.services, query, |sv| sv.name.as_str());

    // Fixed column widths driven by hard kernel limits rather than the data
    // on screen, so the layout is identical across runs / machines / CI:
    // - PID column: `/proc/sys/kernel/pid_max` defaults to 4194304 → 7 digits.
    // - PROCESS column: `TASK_COMM_LEN` is 16 (15 chars + NUL) → 15 chars,
    //   matching what `/proc/<pid>/comm` (the source of this field) can hold.
    // NAME gets whatever's left (two 2-space separators = 4 cols) and is
    // truncated with `…`. NAME left-aligned; PID/PROCESS right-aligned.
    // Fixed widths (not auto-sized to the data) are required by the e2e
    // snapshot tests — insta filters mask the PID / process-name *content*
    // to `<PID>` / `<PROC>` after render, and with auto-sized columns the
    // mask's width would differ from the original's, shifting every
    // subsequent column. Fixed widths let the filter replace a
    // column-width-wide run with a column-width-wide placeholder.
    let inner_w = area.width.saturating_sub(2) as usize; // inside the borders
    const PID_W: usize = 7;
    const PROC_W: usize = 15;
    let name_w = inner_w.saturating_sub(PID_W + PROC_W + 4);

    let items: Vec<ListItem> = view
        .iter()
        .map(|&i| {
            let sv = &s.services[i];
            let pid = sv.pid.map(|p| p.to_string()).unwrap_or_default();
            let proc = sv.process.clone().unwrap_or_default();
            ListItem::new(Line::from(format!(
                "{name:<name_w$}  {pid:>pid_w$}  {proc:>proc_w$}",
                name = truncate(&sv.name, name_w),
                pid = pid,
                proc = proc,
                name_w = name_w,
                pid_w = PID_W,
                proc_w = PROC_W,
            )))
        })
        .collect();
    let list = List::new(items)
        .block(block.clone())
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut list_state = ListState::default().with_offset(scroll[0]);
    // Highlight the selected raw index if it's in the view; otherwise nothing.
    if let Some(pos) = view.iter().position(|&i| i == s.selected) {
        list_state.select(Some(pos));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
    // Persist the offset ratatui computed so the cursor stays put next frame
    // (rather than re-anchoring to the viewport bottom from offset 0).
    scroll[0] = list_state.offset();

    // Record click targets for the visible rows only, offset by the scroll so a
    // click maps to the row actually rendered under the cursor.
    push_list_rows(targets, area, scroll[0], &view, ClickTarget::ServiceRow);
}

fn render_objects(
    frame: &mut Frame,
    area: Rect,
    o: &crate::tui::state::ObjectsScreen,
    query: &str,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut [usize; 3],
) {
    let title = if o.loading {
        "Objects (loading…)"
    } else {
        "Objects"
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    if let Some(err) = &o.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")).block(block), area);
        return;
    }
    let view = crate::tui::state::filter_view(&o.paths, query, |p| p.as_str());
    let items: Vec<ListItem> = view
        .iter()
        .map(|&i| ListItem::new(Line::from(o.paths[i].clone())))
        .collect();
    let list = List::new(items)
        .block(block.clone())
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut ls = ListState::default().with_offset(scroll[0]);
    if let Some(pos) = view.iter().position(|&i| i == o.selected) {
        ls.select(Some(pos));
    }
    frame.render_stateful_widget(list, area, &mut ls);
    scroll[0] = ls.offset();

    push_list_rows(targets, area, scroll[0], &view, ClickTarget::ObjectsRow);
}

fn render_interfaces(
    frame: &mut Frame,
    area: Rect,
    i: &crate::tui::state::InterfacesScreen,
    query: &str,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut [usize; 3],
) {
    let title = if i.loading {
        "Interfaces (loading…)"
    } else {
        "Interfaces"
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    if let Some(err) = &i.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")).block(block), area);
        return;
    }
    let view = crate::tui::state::filter_view(&i.names, query, |n| n.as_str());
    let items: Vec<ListItem> = view
        .iter()
        .map(|&idx| ListItem::new(Line::from(i.names[idx].clone())))
        .collect();
    let list = List::new(items)
        .block(block.clone())
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut ls = ListState::default().with_offset(scroll[0]);
    if let Some(pos) = view.iter().position(|&idx| idx == i.selected) {
        ls.select(Some(pos));
    }
    frame.render_stateful_widget(list, area, &mut ls);
    scroll[0] = ls.offset();

    push_list_rows(targets, area, scroll[0], &view, ClickTarget::InterfacesRow);
}

fn render_interface(
    frame: &mut Frame,
    area: Rect,
    i: &crate::tui::state::InterfaceScreen,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut [usize; 3],
) {
    // Left: the three stacked member lists. Right: the action-button bar for the
    // focused column's selected member.
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
        .split(area);
    let (left, right) = (cols[0], cols[1]);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(left);

    let method_rows: Vec<(String, String)> = i
        .methods
        .iter()
        .map(|m| (m.name.clone(), m.signature.clone()))
        .collect();
    let methods = member_rows(&method_rows, chunks[0].width.saturating_sub(2) as usize);
    scroll[0] = render_sub_list(
        frame,
        chunks[0],
        "methods",
        methods,
        i.selected[0],
        !i.in_buttons && i.focus == InterfaceFocus::Methods,
        scroll[0],
    );
    push_list_rows(
        targets,
        chunks[0],
        scroll[0],
        &(0..i.methods.len()).collect::<Vec<_>>(),
        ClickTarget::MethodRow,
    );

    // Properties show the GetAll value alongside name + signature. If GetAll
    // failed for this object/interface, show that scoped to this column (some
    // objects' GetAll rejects interfaces they don't track — e.g. the standard
    // org.freedesktop.DBus.* ones) instead of blanking the whole screen: the
    // methods and signals columns stay visible.
    if let Some(err) = &i.error {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("properties (unavailable)");
        frame.render_widget(Paragraph::new(err.clone()).block(block), chunks[1]);
    } else {
        // Align the name and signature columns so the GetAll values line up.
        let inner = chunks[1].width.saturating_sub(2) as usize;
        let sig_w = i
            .properties
            .iter()
            .map(|(_, s, _)| s.chars().count())
            .max()
            .unwrap_or(0);
        let name_w = i
            .properties
            .iter()
            .map(|(n, _, _)| n.chars().count())
            .max()
            .unwrap_or(0)
            .min(inner.saturating_sub(sig_w + 4));
        let properties: Vec<ListItem> = i
            .properties
            .iter()
            .map(|(n, sig, _access)| {
                let val = i
                    .prop_values
                    .iter()
                    .find(|(k, _)| k == n)
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("");
                ListItem::new(Line::from(format!(
                    "{}  {:<sig_w$}  {}",
                    pad_col(n, name_w),
                    sig,
                    val,
                    sig_w = sig_w,
                )))
            })
            .collect();
        let p_title = if i.loading {
            "properties (loading…)"
        } else {
            "properties"
        };
        scroll[1] = render_sub_list(
            frame,
            chunks[1],
            p_title,
            properties,
            i.selected[1],
            !i.in_buttons && i.focus == InterfaceFocus::Properties,
            scroll[1],
        );
        push_list_rows(
            targets,
            chunks[1],
            scroll[1],
            &(0..i.properties.len()).collect::<Vec<_>>(),
            ClickTarget::PropertyRow,
        );
    }

    let signals = member_rows(&i.signals, chunks[2].width.saturating_sub(2) as usize);
    scroll[2] = render_sub_list(
        frame,
        chunks[2],
        "signals",
        signals,
        i.selected[2],
        !i.in_buttons && i.focus == InterfaceFocus::Signals,
        scroll[2],
    );
    push_list_rows(
        targets,
        chunks[2],
        scroll[2],
        &(0..i.signals.len()).collect::<Vec<_>>(),
        ClickTarget::SignalRow,
    );

    // Action-button bar: the buttons offered for the focused column's selected
    // member. Highlighted (focused) when `in_buttons`. Never grows past a few
    // rows, so its offset isn't persisted (seed 0; return ignored).
    let buttons: Vec<ListItem> = action_buttons(i.focus)
        .iter()
        .map(|b| ListItem::new(Line::from(*b)))
        .collect();
    let n_buttons = buttons.len();
    let _ = render_sub_list(
        frame,
        right,
        "actions",
        buttons,
        i.button_selected,
        i.in_buttons,
        0,
    );
    push_list_rows(
        targets,
        right,
        0,
        &(0..n_buttons).collect::<Vec<_>>(),
        ClickTarget::ActionButton,
    );
}

/// Push one click target per row of a bordered list rendered into `area`. The
/// list renders inside its block's inner area (inside the border); row `i` is at
/// Record one click target per VISIBLE list row, accounting for the scroll
/// `offset`: row `i` renders at `inner.y + (i - offset)`. Only the visible
/// window `[offset, offset+height)` gets targets. Without the offset a scrolled
/// list desyncs clicks from the rendered rows — e.g. clicking the top visible
/// row (index `offset`) would hit row 0 instead.
/// Record click targets for the visible rows of a (possibly filtered) list.
/// `rows` is the raw-index view (see `state::filter_view`); `offset` is the
/// scroll position within that view. Each target carries the row's *raw* index
/// so the mouse handler (which sets `selected = i`) works unchanged whether or
/// not a filter is active.
fn push_list_rows(
    targets: &mut Vec<(Rect, ClickTarget)>,
    area: Rect,
    offset: usize,
    rows: &[usize],
    make: impl Fn(usize) -> ClickTarget,
) {
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let vh = inner.height as usize;
    let start = offset.min(rows.len());
    let end = offset.saturating_add(vh).min(rows.len());
    for (rel, &raw) in rows[start..end].iter().enumerate() {
        targets.push((
            Rect::new(inner.x, inner.y + rel as u16, inner.width, 1),
            make(raw),
        ));
    }
}

/// The action buttons offered for a given column (mirrors `update`).
fn action_buttons(column: InterfaceFocus) -> &'static [&'static str] {
    match column {
        InterfaceFocus::Methods => &["Call", "Listen"],
        InterfaceFocus::Properties => &["Get", "Set", "Listen"],
        InterfaceFocus::Signals => &["Listen"],
    }
}

/// The action form: one row per input field (label + value), then a `[Trigger]`
/// trigger button. The focused field / trigger is REVERSED (trigger is BOLD too).
/// Zero-arg calls render just the trigger row.
fn render_detail(
    frame: &mut Frame,
    area: Rect,
    d: &DetailScreen,
    nav: &NavContext,
    targets: &mut Vec<(Rect, ClickTarget)>,
) {
    let title = if d.loading {
        format!("{} (loading…)", action_title(&d.kind, &nav.interface))
    } else {
        action_title(&d.kind, &nav.interface)
    };
    let block = Block::default().borders(Borders::ALL).title(title);

    if let Some(err) = &d.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")).block(block), area);
        return;
    }

    // Fields chunk (one row per input + a little breathing room) + a 1-line
    // trigger chunk pinned to the bottom of the block.
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 1 {
        return;
    }
    let trigger_h = 1u16;
    let fields_h = inner.height.saturating_sub(trigger_h);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(fields_h), Constraint::Length(trigger_h)])
        .split(inner);
    let (fields_area, trigger_area) = (chunks[0], chunks[1]);

    // Render each field: "label  value" on its own line; the focused field is
    // REVERSED. With more fields than rows, the lower ones scroll off (fine for
    // now; methods rarely have many IN-args). The `name  sig` label is split
    // into two padded columns so both the member-name and signature line up
    // across args. A bare label (no "  " — Set's signature, a Listen preview, a
    // nameless arg) lands in the signature column; if no field has a name the
    // name column is dropped entirely (no stray indent).
    let split: Vec<(&str, &str)> = d
        .field_labels
        .iter()
        .map(|l| match l.split_once("  ") {
            Some((n, s)) => (n, s),
            None => ("", l.as_str()),
        })
        .collect();
    let width = fields_area.width as usize;
    let sig_w = split
        .iter()
        .map(|(_, s)| s.chars().count())
        .max()
        .unwrap_or(0)
        .min(width);
    let name_w = split
        .iter()
        .map(|(n, _)| n.chars().count())
        .max()
        .unwrap_or(0)
        .min(width.saturating_sub(sig_w + 6));
    for (i, (name, sig)) in split.iter().copied().enumerate() {
        if i as u16 >= fields_area.height {
            break;
        }
        let focused = d.focus == DetailFocus::Field && i == d.field_selected;
        let row_area = Rect {
            x: fields_area.x,
            y: fields_area.y + i as u16,
            width: fields_area.width,
            height: 1,
        };
        let input = d.inputs.get(i);
        let value = input.map(|v| v.value()).unwrap_or("");
        let label = if name_w == 0 {
            sig.to_string()
        } else {
            format!("{}  {:<sig_w$}", pad_col(name, name_w), sig, sig_w = sig_w)
        };
        // Focused field: a `▶` marker + the label, then the whole input slot
        // (value + cursor + trailing pad) drawn as one REVERSED block so the
        // field reads as an interactive box even when empty — not just a lone
        // cursor. The cursor is a yellow-on-reversed cell so it stands out
        // within the block. Unfocused: plain, indented to align with the `▶`.
        let line = if focused {
            let cursor = input.map(|v| v.cursor()).unwrap_or(0).min(value.len());
            let (before, after) = value.split_at(cursor);
            let rev = Style::default().add_modifier(Modifier::REVERSED);
            let prefix_w = 2 + label.chars().count() + 2;
            let input_w = (fields_area.width as usize).saturating_sub(prefix_w);
            let used = before.chars().count() + 1 + after.chars().count();
            let pad = input_w.saturating_sub(used);
            Line::from(vec![
                Span::styled("▶ ", Style::default().fg(Color::Yellow)),
                Span::raw(format!("{label}  ")),
                Span::styled(before.to_string(), rev),
                Span::styled(
                    "▏",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::REVERSED),
                ),
                Span::styled(after.to_string(), rev),
                Span::styled(" ".repeat(pad), rev),
            ])
        } else {
            Line::raw(format!("  {label}  {value}"))
        };
        frame.render_widget(Paragraph::new(line), row_area);
        targets.push((row_area, ClickTarget::DetailField(i)));
    }

    // The trigger button, centered, BOLD + REVERSED when focused.
    let trigger_focused = d.focus == DetailFocus::Trigger;
    let mut style = Style::default();
    if trigger_focused {
        style = style.add_modifier(Modifier::BOLD | Modifier::REVERSED);
    }
    frame.render_widget(
        Paragraph::new("[Trigger]")
            .style(style)
            .alignment(Alignment::Center),
        trigger_area,
    );
    targets.push((trigger_area, ClickTarget::DetailTrigger));
}

/// The outcome of a one-shot action. Loading → "…" (the title carries the
/// context); error → the message; `Call(lines)` → one reply value per line
/// (offset by `scroll` — clamped). `Get`/`Set` render their payload
/// too.
fn render_result(frame: &mut Frame, area: Rect, r: &ResultScreen) {
    // Result screens are read-only (scroll-only) — no click targets.
    let title = if r.loading {
        format!("{} (loading…)", r.title)
    } else {
        r.title.clone()
    };
    let block = Block::default().borders(Borders::ALL).title(title);

    // Streaming-listen mode takes precedence over the one-shot result/loading:
    // if any message block has arrived, show the joined blocks (skipped by scroll).
    let body = if let Some(err) = &r.error {
        format!("error: {err}")
    } else if !r.messages.is_empty() {
        r.messages
            .iter()
            .skip(r.scroll)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    } else if r.loading {
        "…".to_string()
    } else {
        match &r.result {
            Some(ActionResult::Call(lines)) => {
                // Skip `scroll` leading lines (update clamps the scroll value).
                lines
                    .iter()
                    .skip(r.scroll)
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Some(ActionResult::Get(v)) => v.clone(),
            Some(ActionResult::Set) => "ok".to_string(),
            None => String::new(),
        }
    };
    frame.render_widget(Paragraph::new(body).block(block), area);
}

/// A titled list. The focused column gets a `▶` title prefix + bold border; the
/// selected row is REVERSED in every column (so selection is visible everywhere).
///
/// `offset` seeds the list's scroll position (persisted across frames by the
/// caller via `render`'s `scroll` param); the returned offset is what ratatui
/// recomputed to keep `selected` visible, for the caller to persist.
fn render_sub_list(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    items: Vec<ListItem>,
    selected: usize,
    focused: bool,
    offset: usize,
) -> usize {
    let display_title = if focused {
        format!("▶ {title}")
    } else {
        title.to_string()
    };
    let mut block = Block::default().borders(Borders::ALL).title(display_title);
    if focused {
        block = block.border_style(Style::default().add_modifier(Modifier::BOLD));
    }
    let mut ls = ListState::default().with_offset(offset);
    if !items.is_empty() {
        ls.select(Some(selected));
    }
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, area, &mut ls);
    ls.offset()
}

fn render_keyhint(frame: &mut Frame, area: Rect, screen: &Screen) {
    let hint = match screen {
        Screen::Service(_) => "↑↓ select · Enter open · / filter · r refresh · q quit · ? help",
        Screen::Objects(_) => {
            "↑↓ select · Enter open · / filter · r refresh · Esc back · q quit · ? help"
        }
        Screen::Interfaces(_) => {
            "↑↓ select · Enter open · / filter · r refresh · Esc back · q quit · ? help"
        }
        Screen::Interface(_) => {
            "Tab column · ↑↓ select · Enter open · r refresh · Esc back · q quit · ? help"
        }
        Screen::Detail(_) => "Tab move · Enter trigger · c copy-as · Esc back · q quit · ? help",
        // A streaming-listen Result is armed when it has streamed messages or a
        // live cancel sender; on those, Esc both pops the screen and stops the
        // listen (the cancel sender drops). One-shot Results keep "Esc back".
        Screen::Result(r) if !r.messages.is_empty() || r.cancel.is_some() => {
            "↑↓ scroll · c copy-as · y copy · Esc back/stop · q quit · ? help"
        }
        Screen::Result(_) => "↑↓ scroll · c copy-as · y copy · Esc back · q quit · ? help",
    };
    frame.render_widget(Paragraph::new(hint), area);
}

/// Render the copy-as popup overlay: a centered, bordered block listing the four
/// tools (each with its command or "(unsupported)"), the selected row REVERSED,
/// a preview area below showing the selected tool's full command (or the
/// unsupported reason), and a status line at the bottom showing the result of the
/// last copy attempt ("copying…" / "copied" / "error: …"). `Clear` wipes the
/// underlying screen so the popup reads cleanly on top of it.
///
/// All content (tool rows, preview, status) is laid out from `block.inner(...)`
/// — the area INSIDE the border — so it never paints over the border. (Drawing
/// from the full `popup_area` is what previously let the first row overwrite the
/// top border.)
fn render_popup(
    frame: &mut Frame,
    area: Rect,
    popup: &CopyAsPopup,
    targets: &mut Vec<(Rect, ClickTarget)>,
) {
    let popup_area = centered_rect(80, 50, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title("copy as — ↑↓ choose · Enter copy · Esc");
    frame.render_widget(&block, popup_area);

    // Content sits INSIDE the border. Three regions: the 4-row tool list, a
    // preview area (selected tool's full command / unsupported reason), and a
    // 1-line status line for the last copy attempt.
    let inner = block.inner(popup_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let (list_area, preview_area, status_area) = (chunks[0], chunks[1], chunks[2]);

    // One row per tool: "{name}: {command | (unsupported)}". The selected row is
    // REVERSED; unsupported commands are dimmed grey to signal they can't copy.
    for (i, (tool, cmd)) in popup.commands.iter().enumerate() {
        if i as u16 >= list_area.height {
            break;
        }
        let row_area = Rect {
            x: list_area.x,
            y: list_area.y + i as u16,
            width: list_area.width,
            height: 1,
        };
        let body = match cmd {
            Some(c) => format!("{}: {}", tool.name(), first_line(c)),
            None => format!("{}: (unsupported)", tool.name()),
        };
        let mut style = Style::default();
        if i == popup.selected {
            style = style.add_modifier(Modifier::REVERSED);
        } else if cmd.is_none() {
            style = style.fg(Color::DarkGray);
        }
        frame.render_widget(Paragraph::new(body).style(style), row_area);
        targets.push((row_area, ClickTarget::PopupTool(i)));
    }

    // Preview: the selected tool's full command (commands may be multi-line for
    // `# note` annotations); or why it's unsupported / a degenerate rule.
    let preview = match popup.commands.get(popup.selected) {
        Some((_tool, Some(c))) => c.clone(),
        Some((tool, None)) => {
            if matches!(*tool, Tool::Qdbus) {
                "qdbus has no monitor facility — pick another tool.".to_string()
            } else {
                format!("{} cannot express this operation.", tool.name())
            }
        }
        None => String::new(),
    };
    frame.render_widget(
        Paragraph::new(preview).style(Style::default().fg(Color::Yellow)),
        preview_area,
    );

    // Status line: the result of the last copy attempt, shown inside the popup
    // (never printed to the TTY). Green for success, red for an error; the
    // "copying…" placeholder stays default-colored while the copy is in flight.
    if let Some(status) = &popup.status {
        let style = if status == "copied" {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else if status.starts_with("error") {
            Style::default().fg(Color::Red)
        } else {
            Style::default()
        };
        frame.render_widget(Paragraph::new(status.as_str()).style(style), status_area);
    }
}

/// First line of a (possibly multi-line) command, for the compact tool list. The
/// preview area below shows the full multi-line text.
fn first_line(s: &str) -> &str {
    s.split('\n').next().unwrap_or(s)
}

/// The global keybindings reference shown by the `?` help overlay. Mirrors the
/// keys handled in `update::update_key` (and per-screen `update_*_key`); keep it
/// in sync when a key changes.
const HELP_TEXT: &str = "\
busx — keybindings

global:
  ↑↓ / jk     move selection (or scroll on the Result screen)
  Enter       open / activate / drill in / fire the focused button
  Esc         back (or stop a listen); at the root Service screen, quit
  q           quit
  /           (list screen) filter — type to narrow, ↑↓ pick, Enter open, Esc clear
  Tab         (Interface) cycle the methods/properties/signals columns
  r           refresh the current view (service list / objects / interfaces / property values)
  c           copy-as — generate dbus-send/busctl/qdbus/gdbus for the current op
  y           (Result) copy the result text
  ?           toggle this help
  mouse       click to select / click a button to activate / wheel to scroll

navigation: Service → Objects → Interfaces → Interface → Detail → Result
  (single-item levels auto-skip; Esc unwinds the stack)
";

/// Render the `?` help overlay: a centered, bordered block (titled with its own
/// close hint) wrapping the `HELP_TEXT`. `Clear` wipes the underlying screen so
/// the text reads cleanly on top. Not clickable — records no click targets.
fn render_help(frame: &mut Frame, area: Rect) {
    let popup_area = centered_rect(70, 70, area);
    frame.render_widget(Clear, popup_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("help — Esc or ? to close");
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);
    frame.render_widget(
        Paragraph::new(HELP_TEXT).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

/// The standard ratatui centered-rect helper: a rect of `pct_x`% × `pct_y`% of
/// `r`, centered within it. Used to place the copy-as popup.
fn centered_rect(pct_x: u16, pct_y: u16, r: Rect) -> Rect {
    fn split_rect(len: u16, pct: u16) -> (u16, u16) {
        // (margin, size): two equal margins flank a `pct`% central region.
        let size = len.saturating_mul(pct) / 100;
        let margin = len.saturating_sub(size) / 2;
        (margin, size)
    }
    let (mx, w) = split_rect(r.width, pct_x);
    let (my, h) = split_rect(r.height, pct_y);
    Rect {
        x: r.x + mx,
        y: r.y + my,
        width: w,
        height: h,
    }
}
