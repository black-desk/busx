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

use crate::tui::state::{Screen, ServiceScreen, State};

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
        Screen::Objects(_) => render_placeholder(frame, main, "Objects"),
        Screen::Interfaces(_) => render_placeholder(frame, main, "Interfaces"),
        Screen::Interface(_) => render_placeholder(frame, main, "Interface"),
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

fn render_placeholder(frame: &mut Frame, area: Rect, name: &str) {
    frame.render_widget(Paragraph::new(format!("{name} (loading…)")), area);
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

fn render_keyhint(frame: &mut Frame, area: Rect, screen: &Screen) {
    let hint = match screen {
        Screen::Service(_) => "↑↓ select · Enter open · q quit · ? help",
        Screen::Objects(_) => "↑↓/→← navigate · Enter open · Esc back · q quit",
        Screen::Interfaces(_) => "↑↓ select · Enter open · Esc back · q quit",
        Screen::Interface(_) => "Tab switch · ↑↓ select · r refresh · Esc back · q quit",
    };
    frame.render_widget(Paragraph::new(hint), area);
}
