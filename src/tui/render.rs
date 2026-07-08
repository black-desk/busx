// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure rendering (spec §6, §8). `render` reads `&State` and draws — nothing else.

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
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let (main, footer) = (chunks[0], chunks[1]);

    match &state.screen {
        Screen::Service(s) => render_service(frame, main, s),
    }
    render_keyhint(frame, footer);
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
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut list_state = ListState::default();
    if !s.services.is_empty() {
        list_state.select(Some(s.selected));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_keyhint(frame: &mut Frame, area: Rect) {
    frame.render_widget(Paragraph::new("↑↓ select · Enter open · q quit · ? help"), area);
}
