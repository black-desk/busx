// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure rendering (spec §6, §8). Reads `&State`; draws breadcrumb + top screen
//! + key-hint. Nothing else.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::tui::copy::Tool;
use crate::tui::state::{
    ActionKind, ActionResult, CopyAsPopup, DetailFocus, DetailScreen, InterfaceFocus, ResultScreen,
    Screen, ServiceScreen, State,
};

pub fn render(frame: &mut Frame, state: &State) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let (crumb, main, footer) = (chunks[0], chunks[1], chunks[2]);

    render_breadcrumb(frame, crumb, state);
    match state.top() {
        Screen::Service(s) => render_service(frame, main, s),
        Screen::Objects(o) => render_objects(frame, main, o),
        Screen::Interfaces(i) => render_interfaces(frame, main, i),
        Screen::Interface(i) => render_interface(frame, main, i),
        Screen::Detail(d) => render_detail(frame, main, d),
        Screen::Result(r) => render_result(frame, main, r),
    }
    render_keyhint(frame, footer, state.top());

    // The copy-as popup overlays the whole frame when open. Drawn last so it sits
    // on top of the screen + keyhint; Clear wipes the underlying area first.
    if let Some(popup) = &state.popup {
        render_popup(frame, area, popup);
    }
}

fn render_breadcrumb(frame: &mut Frame, area: Rect, state: &State) {
    let parts: Vec<String> = state.screens.iter().map(screen_crumb).collect();
    let text = parts.join(" > ");
    frame.render_widget(Paragraph::new(text), area);
}

fn screen_crumb(s: &Screen) -> String {
    match s {
        Screen::Service(_) => "services".to_string(),
        Screen::Objects(o) => o.service.clone(),
        Screen::Interfaces(i) => format!("{} {}", i.service, i.object),
        Screen::Interface(i) => format!("{}:{}:{}", i.service, i.object, i.interface),
        Screen::Detail(d) => format!("{}:{}:{} › {}", d.service, d.object, d.interface, action_title(&d.kind)),
        Screen::Result(r) => r.title.clone(),
    }
}

/// Short label for an action kind (breadcrumb / Detail title).
fn action_title(kind: &ActionKind) -> String {
    match kind {
        ActionKind::Call { method, .. } => format!("call {method}"),
        ActionKind::Get { property } => format!("get {property}"),
        ActionKind::Set { property, .. } => format!("set {property}"),
        ActionKind::Listen { .. } => "listen".to_string(),
    }
}

fn render_service(frame: &mut Frame, area: Rect, s: &ServiceScreen) {
    let title = if s.loading { "Services (loading…)" } else { "Services" };
    let block = Block::default().borders(Borders::ALL).title(title);

    if let Some(err) = &s.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")).block(block), area);
        return;
    }

    let items: Vec<ListItem> = s
        .services
        .iter()
        .map(|sv| {
            let pid = sv.pid.map(|p| p.to_string()).unwrap_or_default();
            let proc = sv.process.clone().unwrap_or_default();
            ListItem::new(Line::from(format!("{:<32} {:>7} {}", sv.name, pid, proc)))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut list_state = ListState::default();
    if !s.services.is_empty() {
        list_state.select(Some(s.selected));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_objects(frame: &mut Frame, area: Rect, o: &crate::tui::state::ObjectsScreen) {
    let title = if o.loading { "Objects (loading…)" } else { "Objects" };
    let block = Block::default().borders(Borders::ALL).title(title);
    if let Some(err) = &o.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")).block(block), area);
        return;
    }
    let items: Vec<ListItem> = o.paths.iter().map(|p| ListItem::new(Line::from(p.clone()))).collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut ls = ListState::default();
    if !o.paths.is_empty() {
        ls.select(Some(o.selected));
    }
    frame.render_stateful_widget(list, area, &mut ls);
}

fn render_interfaces(frame: &mut Frame, area: Rect, i: &crate::tui::state::InterfacesScreen) {
    let title = if i.loading { "Interfaces (loading…)" } else { "Interfaces" };
    let block = Block::default().borders(Borders::ALL).title(title);
    if let Some(err) = &i.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")).block(block), area);
        return;
    }
    let items: Vec<ListItem> = i.names.iter().map(|n| ListItem::new(Line::from(n.clone()))).collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut ls = ListState::default();
    if !i.names.is_empty() {
        ls.select(Some(i.selected));
    }
    frame.render_stateful_widget(list, area, &mut ls);
}

fn render_interface(frame: &mut Frame, area: Rect, i: &crate::tui::state::InterfaceScreen) {
    if let Some(err) = &i.error {
        frame.render_widget(Paragraph::new(format!("error: {err}")), area);
        return;
    }
    // Left: the three stacked member lists. Right: the action-button bar for the
    // active column's selected member.
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

    let methods: Vec<ListItem> = i
        .methods
        .iter()
        .map(|m| ListItem::new(Line::from(format!("{}  {}", m.name, m.signature))))
        .collect();
    render_sub_list(frame, chunks[0], "methods", methods, i.selected[0], i.focus == InterfaceFocus::Methods);

    // Properties show the GetAll value alongside name + signature.
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
            ListItem::new(Line::from(format!("{n}  {sig}  {val}")))
        })
        .collect();
    let p_title = if i.loading { "properties (loading…)" } else { "properties" };
    render_sub_list(frame, chunks[1], p_title, properties, i.selected[1], i.focus == InterfaceFocus::Properties);

    let signals: Vec<ListItem> = i
        .signals
        .iter()
        .map(|(n, sig)| ListItem::new(Line::from(format!("{n}  {sig}"))))
        .collect();
    render_sub_list(frame, chunks[2], "signals", signals, i.selected[2], i.focus == InterfaceFocus::Signals);

    // Action-button bar: the buttons offered for the active column's selected member.
    let buttons: Vec<ListItem> = action_buttons(i.active_column)
        .iter()
        .map(|b| ListItem::new(Line::from(*b)))
        .collect();
    let focused = i.focus == InterfaceFocus::Buttons;
    render_sub_list(frame, right, "actions", buttons, i.button_selected, focused);
}

/// The action buttons offered for a given active column (mirrors `update`).
fn action_buttons(column: InterfaceFocus) -> &'static [&'static str] {
    match column {
        InterfaceFocus::Methods => &["调用", "监听"],
        InterfaceFocus::Properties => &["读取", "设置", "监听"],
        InterfaceFocus::Signals => &["监听"],
        InterfaceFocus::Buttons => &[],
    }
}

/// The action form: one row per input field (label + value), then a `[触发]`
/// trigger button. The focused field / trigger is REVERSED (trigger is BOLD too).
/// Zero-arg calls render just the trigger row.
fn render_detail(frame: &mut Frame, area: Rect, d: &DetailScreen) {
    let title = if d.loading {
        format!("{} (loading…)", action_title(&d.kind))
    } else {
        action_title(&d.kind)
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
    // now; methods rarely have many IN-args).
    for (i, label) in d.field_labels.iter().enumerate() {
        if i as u16 >= fields_area.height {
            break;
        }
        let value = d.inputs.get(i).map(|v| v.value()).unwrap_or("");
        let focused = d.focus == DetailFocus::Field && i == d.field_selected;
        let row_area = Rect {
            x: fields_area.x,
            y: fields_area.y + i as u16,
            width: fields_area.width,
            height: 1,
        };
        let mut style = Style::default();
        if focused {
            style = style.add_modifier(Modifier::REVERSED);
        }
        frame.render_widget(
            Paragraph::new(format!("{label}  {value}")).style(style),
            row_area,
        );
    }

    // The trigger button, centered, BOLD + REVERSED when focused.
    let trigger_focused = d.focus == DetailFocus::Trigger;
    let mut style = Style::default();
    if trigger_focused {
        style = style.add_modifier(Modifier::BOLD | Modifier::REVERSED);
    }
    frame.render_widget(
        Paragraph::new("[触发]").style(style).alignment(Alignment::Center),
        trigger_area,
    );
}

/// The outcome of a one-shot action. Loading → "…" (the title carries the
/// context); error → the message; `Call(lines)` → one reply value per line
/// (offset by `scroll` — clamped in Task 4). `Get`/`Set` render their payload
/// too (Task 3 owns their detail forms).
fn render_result(frame: &mut Frame, area: Rect, r: &ResultScreen) {
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
                lines.iter().skip(r.scroll).map(String::as_str).collect::<Vec<_>>().join("\n")
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
fn render_sub_list(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    items: Vec<ListItem>,
    selected: usize,
    focused: bool,
) {
    let display_title = if focused { format!("▶ {title}") } else { title.to_string() };
    let mut block = Block::default().borders(Borders::ALL).title(display_title);
    if focused {
        block = block.border_style(Style::default().add_modifier(Modifier::BOLD));
    }
    let mut ls = ListState::default();
    if !items.is_empty() {
        ls.select(Some(selected));
    }
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, area, &mut ls);
}

fn render_keyhint(frame: &mut Frame, area: Rect, screen: &Screen) {
    let hint = match screen {
        Screen::Service(_) => "↑↓ select · Enter open · q quit · ? help",
        Screen::Objects(_) => "↑↓ select · Enter open · Esc back · q quit",
        Screen::Interfaces(_) => "↑↓ select · Enter open · Esc back · q quit",
        Screen::Interface(_) => "Tab buttons · Shift+Tab column · ↑↓ select · r refresh · Esc back · q quit",
        Screen::Detail(_) => "Tab move · Enter trigger · c copy-as · Esc back · q quit",
        // A streaming-listen Result is armed when it has streamed messages or a
        // live cancel sender; on those, Esc both pops the screen and stops the
        // listen (the cancel sender drops). One-shot Results keep "Esc back".
        Screen::Result(r) if !r.messages.is_empty() || r.cancel.is_some() => {
            "↑↓ scroll · c copy-as · y copy · Esc back/stop · q quit"
        }
        Screen::Result(_) => "↑↓ scroll · c copy-as · y copy · Esc back · q quit",
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
fn render_popup(frame: &mut Frame, area: Rect, popup: &CopyAsPopup) {
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
        .constraints([Constraint::Length(4), Constraint::Min(1), Constraint::Length(1)])
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
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
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
    Rect { x: r.x + mx, y: r.y + my, width: w, height: h }
}
