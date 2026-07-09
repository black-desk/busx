// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure rendering (spec §6, §8). Reads `&State`; draws breadcrumb + top screen
//! + key-hint. Nothing else.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::tui::state::{InterfaceFocus, Screen, ServiceScreen, State};

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
    }
    render_keyhint(frame, footer, state.top());
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);

    let methods: Vec<ListItem> = i
        .methods
        .iter()
        .map(|(n, sig)| ListItem::new(Line::from(format!("{n}  {sig}"))))
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
        Screen::Interface(_) => "Tab switch · ↑↓ select · r refresh · Esc back · q quit",
    };
    frame.render_widget(Paragraph::new(hint), area);
}
