// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UI transitions. Remote work is returned as an effect, never performed here.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use tui_input::backend::crossterm::EventHandler;
use zbus_xml::{ArgDirection, Node};

use crate::dbus::introspect::{access_str, sig_str};
use crate::error::{Error, Result};
use crate::tui::copy::{CopyOp, Tool, generate};
use crate::tui::msg::{Effect, Msg};
use crate::tui::state::*;

pub fn update(state: &mut State, msg: Msg) -> Option<Effect> {
    let effect = update_message(state, msg);
    if state.filter.is_some() {
        clamp_filter(state);
    }
    effect
}

fn update_message(state: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Key(key) => key_event(state, key),
        Msg::Mouse(event) => mouse(state, event),
        Msg::Resize => None,
        Msg::Services(id, result) => {
            if id == state.services_request {
                state.services_loading = false;
                match result {
                    Ok(services) => {
                        for tab in &mut state.tabs {
                            tab.service_selected =
                                tab.service_selected.min(services.len().saturating_sub(1));
                        }
                        state.services = services;
                        state.services_error = None;
                    }
                    Err(error) => state.services_error = Some(error),
                }
            }
            None
        }
        Msg::Objects(id, result) => {
            let show_standard = state.show_standard_interfaces;
            if let Some(browser) = state
                .tabs
                .iter_mut()
                .filter_map(|tab| tab.browser.as_mut())
                .find(|browser| browser.tree_request == id)
            {
                browser.tree_loading = false;
                match result {
                    Ok(root) => {
                        browser.objects = navigation_objects(root, show_standard);
                        browser.nav_selected = browser
                            .nav_selected
                            .min(browser.navigation("").len().saturating_sub(1));
                        browser.error = None;
                    }
                    Err(error) => browser.error = Some(error),
                }
            }
            None
        }
        Msg::Interface(id, result) => {
            let index = state.tabs.iter().position(|tab| {
                tab.browser
                    .as_ref()
                    .is_some_and(|browser| browser.interface_request == id)
            })?;
            let browser = state.tabs[index].browser.as_mut()?;
            browser.interface_loading = false;
            match result {
                Ok(node) => {
                    browser.members = members_of(&node, &browser.context.interface);
                    browser.category = if !browser.members.properties.is_empty() {
                        Category::Properties
                    } else if !browser.members.methods.is_empty() {
                        Category::Methods
                    } else {
                        Category::Signals
                    };
                    browser.select_member();
                    if browser.members.properties.iter().any(Property::readable) {
                        return fetch_properties_for(state, index);
                    }
                }
                Err(error) => browser.error = Some(error),
            }
            None
        }
        Msg::Properties(id, result) => {
            if let Some(browser) = state
                .tabs
                .iter_mut()
                .filter_map(|tab| tab.browser.as_mut())
                .find(|browser| browser.properties_request == id)
            {
                browser.properties_loading = false;
                match result {
                    Ok(values) => {
                        browser.values = values
                            .into_iter()
                            .map(|(name, value)| (name, crate::value::pretty::pretty(&value)))
                            .collect();
                        browser.properties_error = None;
                    }
                    Err(error) => browser.properties_error = Some(error),
                }
            }
            None
        }
        Msg::Action(id, tab_id, context, op, result) => {
            let index = state.tabs.iter().position(|tab| tab.id == tab_id)?;
            let browser = state.tabs[index].browser.as_mut()?;
            if browser.context != context {
                return None;
            }
            let refresh = matches!((&op, &result), (CopyOp::Set { .. }, Ok(_)));
            if let (CopyOp::Get { property, .. }, Ok(ActionResult::Get(value))) = (&op, &result) {
                if let Some((_, previous)) =
                    browser.values.iter_mut().find(|(name, _)| name == property)
                {
                    *previous = value.clone();
                } else {
                    browser.values.push((property.clone(), value.clone()));
                }
            }
            if browser.editor.pending == Some(id) {
                browser.editor.pending = None;
                browser.editor.last_op = Some(op);
                browser.editor.outcome = Some(match result {
                    Ok(result) => Outcome {
                        result: Some(result),
                        error: None,
                    },
                    Err(error) => Outcome {
                        result: None,
                        error: Some(error),
                    },
                });
            }
            if refresh && browser.members.properties.iter().any(Property::readable) {
                return fetch_properties_for(state, index);
            }
            None
        }
        Msg::WatchReady(id, cancel) => {
            if let Some(watch) = state
                .tabs
                .iter_mut()
                .filter_map(|tab| tab.watch.as_mut())
                .find(|watch| watch.id == id && !watch.stopped)
            {
                watch.cancel = Some(cancel);
            }
            None
        }
        Msg::WatchStarted(id, rule) => {
            if let Some(watch) = state
                .tabs
                .iter_mut()
                .filter_map(|tab| tab.watch.as_mut())
                .find(|watch| watch.id == id && !watch.stopped)
            {
                watch.starting = false;
                watch.rule = rule;
            }
            None
        }
        Msg::WatchMessage(id, body) => {
            if let Some(watch) = state
                .tabs
                .iter_mut()
                .filter_map(|tab| tab.watch.as_mut())
                .find(|watch| watch.id == id && !watch.stopped)
            {
                watch.total += 1;
                if watch.events.len() == EVENT_LIMIT {
                    watch.events.pop_front();
                }
                watch.events.push_back(body);
            }
            None
        }
        Msg::WatchEnded(id, result) => {
            if let Some(watch) = state
                .tabs
                .iter_mut()
                .filter_map(|tab| tab.watch.as_mut())
                .find(|watch| watch.id == id)
            {
                watch.starting = false;
                watch.stopped = true;
                watch.cancel = None;
                if let Err(error) = result {
                    watch.error = Some(error);
                }
            }
            None
        }
        Msg::Clipboard(id, result) => {
            if let Some(tab) = state
                .tabs
                .iter_mut()
                .find(|tab| tab.clipboard_request == Some(id))
            {
                tab.clipboard_request = None;
                let status = match result {
                    Ok(()) => "Copied".into(),
                    Err(error) => format!("Copy failed: {error}"),
                };
                if let Some(popup) = &mut tab.popup {
                    popup.status = Some(status);
                } else {
                    tab.status = Some(status);
                }
            }
            None
        }
    }
}

fn key_event(state: &mut State, key: KeyEvent) -> Option<Effect> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.close_tab();
        return None;
    }
    if state.popup.is_some() {
        state.key_prefix = None;
        return popup_key(state, key.code);
    }
    if state.help_open {
        state.key_prefix = None;
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => state.help_open = false,
            KeyCode::Down | KeyCode::Char('j') => {
                state.help_scroll = (state.help_scroll + 1).min(30)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.help_scroll = state.help_scroll.saturating_sub(1)
            }
            KeyCode::PageDown => state.help_scroll = (state.help_scroll + 10).min(30),
            KeyCode::PageUp => state.help_scroll = state.help_scroll.saturating_sub(10),
            _ => {}
        }
        return None;
    }
    if state.filter.is_some() {
        state.key_prefix = None;
        return filter_key(state, key);
    }
    // Editing owns printable keys, including q, c, r, ?, j and k. Only terminal
    // control, focus movement, and Escape bypass tui-input.
    if state.page == Page::Browser
        && state.browser.as_ref().is_some_and(|browser| {
            browser.editor.editing && matches!(browser.focus, Focus::Field(_))
        })
    {
        state.key_prefix = None;
        let browser = state.browser.as_mut()?;
        let Focus::Field(index) = browser.focus else {
            return None;
        };
        match key.code {
            KeyCode::Esc => browser.editor.editing = false,
            KeyCode::Tab | KeyCode::BackTab => cycle_focus(
                browser,
                key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT),
            ),
            KeyCode::Enter => {
                browser.editor.editing = false;
                browser.focus = Focus::Action(0);
            }
            _ => {
                if let Some(field) = browser.editor.fields.get_mut(index) {
                    field.input.handle_event(&Event::Key(key));
                }
            }
        }
        return None;
    }
    state.status = None;
    if vim_sequence(state, key) {
        return None;
    }
    match key.code {
        KeyCode::Char('q') => {
            state.close_tab();
            return None;
        }
        KeyCode::Char('t') => {
            state.new_tab();
            return None;
        }
        KeyCode::Char('?') => {
            state.help_open = true;
            state.help_scroll = 0;
            return None;
        }
        KeyCode::Char('/')
            if state.page == Page::Services
                || (state.page == Page::Browser
                    && state.browser.as_ref().is_some_and(|browser| {
                        matches!(browser.focus, Focus::Navigation | Focus::Members)
                    })) =>
        {
            state.filter = Some(tui_input::Input::default());
            return None;
        }
        KeyCode::Esc => {
            back(state);
            return None;
        }
        KeyCode::Char('c') => {
            if let Some(op) = current_op(state) {
                open_copy(state, op);
            }
            return None;
        }
        KeyCode::Char('y') => {
            let text = current_text(state);
            return text
                .filter(|text| !text.is_empty())
                .map(|text| copy_text(state, text));
        }
        KeyCode::Char('r') => return refresh(state),
        _ => {}
    }
    match state.page {
        Page::Services => match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                let count = state.services.len();
                move_index(&mut state.service_selected, count, 1)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let count = state.services.len();
                move_index(&mut state.service_selected, count, -1)
            }
            KeyCode::Enter => return open_service(state),
            _ => {}
        },
        Page::Browser => return browser_key(state, key),
        Page::Watch => return watch_key(state, key.code),
        Page::Expanded => {
            if let Some(expanded) = &mut state.expanded {
                scroll_text(
                    &expanded.text,
                    &mut expanded.scroll,
                    &mut expanded.hscroll,
                    key.code,
                );
            }
        }
    }
    None
}

/// Tab commands preserve each tab's browser, inputs, scroll, and subscription.
fn vim_sequence(state: &mut State, key: KeyEvent) -> bool {
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        state.key_prefix = None;
        return false;
    }
    match (state.key_prefix.take(), key.code) {
        (Some(KeyPrefix::Count(count)), KeyCode::Char(digit @ '0'..='9')) => {
            state.key_prefix = Some(KeyPrefix::Count(
                count
                    .saturating_mul(10)
                    .saturating_add(digit.to_digit(10).unwrap_or(0) as usize),
            ));
            true
        }
        (Some(KeyPrefix::Count(count)), KeyCode::Char('g')) => {
            state.key_prefix = Some(KeyPrefix::G(Some(count)));
            true
        }
        (Some(KeyPrefix::G(count)), KeyCode::Char(command @ ('t' | 'T'))) => {
            let length = state.tabs.len();
            let next = if command == 'T' {
                (state.active_tab + length - count.unwrap_or(1) % length) % length
            } else if let Some(count) = count {
                if count == 0 || count > length {
                    return true;
                }
                count - 1
            } else {
                (state.active_tab + 1) % length
            };
            state.select_tab(next);
            true
        }
        (Some(KeyPrefix::G(count)), KeyCode::Char('g')) => {
            jump_selection(state, count.unwrap_or(1).saturating_sub(1));
            true
        }
        (Some(_), KeyCode::Esc) => true,
        (_, KeyCode::Char('g')) => {
            state.key_prefix = Some(KeyPrefix::G(None));
            true
        }
        (_, KeyCode::Char(digit @ '1'..='9')) => {
            state.key_prefix = Some(KeyPrefix::Count(digit.to_digit(10).unwrap_or(1) as usize));
            true
        }
        (_, KeyCode::Char('G')) => {
            jump_selection(state, usize::MAX);
            true
        }
        _ => false,
    }
}

fn jump_selection(state: &mut State, index: usize) {
    match state.page {
        Page::Services => {
            state.service_selected = index.min(state.services.len().saturating_sub(1))
        }
        Page::Browser => {
            if let Some(browser) = &mut state.browser {
                match browser.focus {
                    Focus::Navigation => {
                        browser.nav_selected =
                            index.min(browser.navigation("").len().saturating_sub(1))
                    }
                    Focus::Members => {
                        let selected = index.min(browser.member_count().saturating_sub(1));
                        if browser.selected() != selected {
                            browser.selected[browser.category as usize] = selected;
                            browser.select_member();
                        }
                    }
                    Focus::Result => {
                        if let Some(outcome) = &browser.editor.outcome {
                            browser.editor.scroll =
                                index.min(outcome.text().lines().count().saturating_sub(1));
                        }
                    }
                    _ => {}
                }
            }
        }
        Page::Watch => {
            if let Some(watch) = state.watch_mut() {
                watch.follow = index == usize::MAX;
                watch.scroll = index.min(watch.text().lines().count().saturating_sub(1));
            }
        }
        Page::Expanded => {
            if let Some(expanded) = &mut state.expanded {
                expanded.scroll = index.min(expanded.text.lines().count().saturating_sub(1));
            }
        }
    }
}

fn back(state: &mut State) {
    match state.page {
        Page::Services => state.close_tab(),
        Page::Browser => {
            if let Some(browser) = &mut state.browser {
                if browser.focus == Focus::Navigation {
                    state.page = Page::Services;
                } else {
                    browser.focus = Focus::Navigation;
                }
            }
        }
        Page::Watch => {
            state.stop_watch();
            state.page = if state.browser.is_some() {
                Page::Browser
            } else {
                Page::Services
            }
        }
        Page::Expanded => {
            if let Some(expanded) = state.expanded.take() {
                state.page = expanded.origin;
            }
        }
    }
}

fn open_service(state: &mut State) -> Option<Effect> {
    let service = state.services.get(state.service_selected)?.name.clone();
    if state
        .browser
        .as_ref()
        .is_some_and(|browser| browser.context.service == service)
    {
        state.page = Page::Browser;
        state.filter = None;
        return None;
    }
    let id = state.request_id();
    state.browser = Some(Browser::new(service.clone(), id));
    state.page = Page::Browser;
    state.filter = None;
    Some(Effect::Objects(id, service))
}

fn open_navigation(state: &mut State) -> Option<Effect> {
    let id = state.request_id();
    let query = state.query().to_string();
    let browser = state.browser.as_mut()?;
    let row = *browser.navigation(&query).get(browser.nav_selected)?;
    let (object, interface) = match row {
        NavRow::Object(index) => {
            let object = browser.objects.get_mut(index)?;
            object.expanded = !object.expanded;
            browser.reveal_object(index, false);
            clear_filter(state);
            return None;
        }
        NavRow::Interface(object, interface) => {
            browser.reveal_object(object, true);
            (
                browser.objects[object].path.clone(),
                browser.objects[object].interfaces[interface].clone(),
            )
        }
    };
    browser.nav_selected = browser
        .navigation("")
        .iter()
        .position(|candidate| *candidate == row)
        .unwrap_or(0);
    browser.context.object = object;
    browser.context.interface = interface;
    browser.interface_request = id;
    browser.properties_request = 0;
    browser.interface_loading = true;
    browser.properties_loading = false;
    browser.error = None;
    browser.properties_error = None;
    browser.members = Members::default();
    browser.values.clear();
    browser.selected = [0; 3];
    browser.editor = Editor::default();
    browser.focus = Focus::Members;
    let context = browser.context.clone();
    state.filter = None;
    Some(Effect::Interface(id, context))
}

fn fetch_properties(state: &mut State) -> Option<Effect> {
    fetch_properties_for(state, state.active_tab)
}

fn fetch_properties_for(state: &mut State, index: usize) -> Option<Effect> {
    let id = state.request_id();
    let browser = state.tabs.get_mut(index)?.browser.as_mut()?;
    browser.properties_request = id;
    browser.properties_loading = true;
    browser.properties_error = None;
    Some(Effect::Properties(id, browser.context.clone()))
}

fn refresh(state: &mut State) -> Option<Effect> {
    let id = state.request_id();
    match state.page {
        Page::Services => {
            state.services_request = id;
            state.services_loading = true;
            state.services_error = None;
            Some(Effect::Services(id))
        }
        Page::Browser => {
            let browser = state.browser.as_mut()?;
            if browser.focus == Focus::Navigation {
                browser.tree_request = id;
                browser.tree_loading = true;
                browser.error = None;
                Some(Effect::Objects(id, browser.context.service.clone()))
            } else if browser.error.is_some() && !browser.context.interface.is_empty() {
                browser.interface_request = id;
                browser.interface_loading = true;
                browser.error = None;
                Some(Effect::Interface(id, browser.context.clone()))
            } else if !browser.context.interface.is_empty() {
                fetch_properties(state)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn browser_key(state: &mut State, key: KeyEvent) -> Option<Effect> {
    let browser = state.browser.as_mut()?;
    match key.code {
        KeyCode::Tab | KeyCode::BackTab => cycle_focus(
            browser,
            key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT),
        ),
        KeyCode::Char('n') => browser.focus = Focus::Navigation,
        KeyCode::Char('i') if matches!(browser.focus, Focus::Field(_)) => {
            browser.editor.editing = true;
        }
        KeyCode::Enter => match browser.focus {
            Focus::Navigation => return open_navigation(state),
            Focus::Members => {
                if browser.member_count() > 0 {
                    browser.focus = if browser.editor.fields.is_empty() {
                        Focus::Action(0)
                    } else {
                        Focus::Field(0)
                    };
                }
            }
            Focus::Action(index) => return activate(state, index),
            Focus::Result => return expand(state),
            Focus::Field(_) => browser.focus = Focus::Action(0),
        },
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l')
            if browser.focus == Focus::Members =>
        {
            let index = browser.category as usize;
            let next = if matches!(key.code, KeyCode::Right | KeyCode::Char('l')) {
                (index + 1) % 3
            } else {
                (index + 2) % 3
            };
            browser.category = Category::ALL[next];
            browser.select_member();
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l')
            if browser.focus == Focus::Navigation =>
        {
            let rows = browser.navigation("");
            if let Some(row) = rows.get(browser.nav_selected) {
                match *row {
                    NavRow::Object(index) => {
                        browser.objects[index].expanded =
                            matches!(key.code, KeyCode::Right | KeyCode::Char('l'))
                    }
                    NavRow::Interface(index, _)
                        if matches!(key.code, KeyCode::Left | KeyCode::Char('h')) =>
                    {
                        if let Some(parent) =
                            rows.iter().position(|row| *row == NavRow::Object(index))
                        {
                            browser.nav_selected = parent;
                        }
                        browser.objects[index].expanded = false;
                    }
                    NavRow::Interface(_, _) => return open_navigation(state),
                }
            }
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l')
            if matches!(browser.focus, Focus::Action(_)) =>
        {
            let count = browser.actions().len();
            let Focus::Action(index) = &mut browser.focus else {
                return None;
            };
            if matches!(key.code, KeyCode::Left | KeyCode::Char('h')) {
                *index = index.saturating_sub(1);
            } else {
                *index = (*index + 1).min(count.saturating_sub(1));
            }
        }
        KeyCode::Down | KeyCode::Char('j') => move_browser(browser, 1),
        KeyCode::Up | KeyCode::Char('k') => move_browser(browser, -1),
        _ if browser.focus == Focus::Result => {
            if let Some(outcome) = &browser.editor.outcome {
                scroll_text(
                    &outcome.text(),
                    &mut browser.editor.scroll,
                    &mut browser.editor.hscroll,
                    key.code,
                );
            }
        }
        _ => {}
    }
    None
}

fn move_browser(browser: &mut Browser, delta: i32) {
    match browser.focus {
        Focus::Navigation => {
            let count = browser.navigation("").len();
            move_index(&mut browser.nav_selected, count, delta);
        }
        Focus::Members => {
            let count = browser.member_count();
            let index = browser.category as usize;
            let previous = browser.selected[index];
            move_index(&mut browser.selected[index], count, delta);
            if previous != browser.selected[index] {
                browser.select_member();
            }
        }
        Focus::Action(_) => {
            if delta < 0 {
                browser.focus = browser
                    .editor
                    .fields
                    .len()
                    .checked_sub(1)
                    .map_or(Focus::Members, Focus::Field);
            } else if browser.editor.outcome.is_some() {
                browser.focus = Focus::Result;
            }
        }
        Focus::Result => {
            if let Some(outcome) = &browser.editor.outcome {
                scroll_text(
                    &outcome.text(),
                    &mut browser.editor.scroll,
                    &mut browser.editor.hscroll,
                    if delta > 0 {
                        KeyCode::Down
                    } else {
                        KeyCode::Up
                    },
                );
            }
        }
        Focus::Field(mut index) => {
            if delta > 0 && index + 1 >= browser.editor.fields.len() {
                if !browser.actions().is_empty() {
                    browser.focus = Focus::Action(0);
                }
            } else if delta < 0 && index == 0 {
                browser.focus = Focus::Members;
            } else {
                move_index(&mut index, browser.editor.fields.len(), delta);
                browser.focus = Focus::Field(index);
            }
        }
    }
}

fn cycle_focus(browser: &mut Browser, reverse: bool) {
    let mut sequence = vec![Focus::Navigation, Focus::Members];
    sequence.extend((0..browser.editor.fields.len()).map(Focus::Field));
    sequence.extend((0..browser.actions().len()).map(Focus::Action));
    if browser.editor.outcome.is_some() {
        sequence.push(Focus::Result);
    }
    let index = sequence
        .iter()
        .position(|focus| *focus == browser.focus)
        .unwrap_or(0);
    browser.focus =
        sequence[(index + if reverse { sequence.len() - 1 } else { 1 }) % sequence.len()];
    browser.editor.editing &= matches!(browser.focus, Focus::Field(_));
}

fn activate(state: &mut State, index: usize) -> Option<Effect> {
    let action = *state.browser.as_ref()?.actions().get(index)?;
    match action {
        Action::Copy => {
            match draft_op(state.browser.as_ref()?, None) {
                Ok(op) => open_copy(state, op),
                Err(error) => set_editor_error(state, error),
            }
            None
        }
        Action::Expand => expand(state),
        Action::Watch => start_watch(state),
        Action::Call | Action::Read | Action::Write => {
            let browser = state.browser.as_ref()?;
            if browser.editor.pending.is_some() {
                return None;
            }
            let op = match draft_op(browser, Some(action)) {
                Ok(op) => op,
                Err(error) => {
                    set_editor_error(state, error);
                    return None;
                }
            };
            let id = state.request_id();
            let tab_id = state.id;
            let browser = state.browser.as_mut()?;
            browser.editor.pending = Some(id);
            browser.editor.outcome = None;
            browser.editor.scroll = 0;
            browser.editor.hscroll = 0;
            browser.editor.last_op = Some(op.clone());
            Some(Effect::Execute(id, tab_id, browser.context.clone(), op))
        }
    }
}

fn set_editor_error(state: &mut State, error: Error) {
    if let Some(browser) = &mut state.browser {
        browser.editor.outcome = Some(Outcome {
            result: None,
            error: Some(error),
        });
    }
}

/// Basic values occupy one field verbatim (spaces and empty strings included).
/// Containers use the same count-prefixed tokens as busx's CLI; quoting only
/// groups container tokens, never invokes a shell.
fn field_tokens(signature: &str, value: &str) -> Result<Vec<String>> {
    if signature.len() == 1 && signature != "v" {
        return Ok(vec![value.to_string()]);
    }
    let mut tokens = vec![];
    let mut token = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;
    for ch in value.chars() {
        if escaped {
            token.push(ch);
            escaped = false;
            started = true;
            continue;
        }
        match (quote, ch) {
            (Some('\''), '\'') => quote = None,
            (Some('\''), ch) => token.push(ch),
            (_, '\\') => {
                escaped = true;
                started = true;
            }
            (Some('"'), '"') => quote = None,
            (Some(_), ch) => token.push(ch),
            (None, '\'' | '"') => {
                quote = Some(ch);
                started = true;
            }
            (None, ch) if ch.is_whitespace() => {
                if started {
                    tokens.push(std::mem::take(&mut token));
                    started = false;
                }
            }
            (None, ch) => {
                token.push(ch);
                started = true;
            }
        }
    }
    if quote.is_some() || escaped {
        return Err(Error::Msg(
            "Unclosed quote or trailing escape in value".into(),
        ));
    }
    if started {
        tokens.push(token);
    }
    Ok(tokens)
}

fn draft_op(browser: &Browser, action: Option<Action>) -> Result<CopyOp> {
    let context = &browser.context;
    match browser.category {
        Category::Methods => {
            let method = browser
                .members
                .methods
                .get(browser.selected())
                .ok_or_else(|| Error::Msg("No method selected".into()))?;
            let mut args = vec![];
            for ((_, signature), field) in method.args.iter().zip(&browser.editor.fields) {
                args.extend(field_tokens(signature, field.input.value())?);
            }
            Ok(CopyOp::Call {
                service: context.service.clone(),
                object: context.object.clone(),
                iface: context.interface.clone(),
                method: method.name.clone(),
                signature: method.signature.clone(),
                args,
            })
        }
        Category::Properties => {
            let property = browser
                .members
                .properties
                .get(browser.selected())
                .ok_or_else(|| Error::Msg("No property selected".into()))?;
            if action == Some(Action::Write)
                || (action.is_none()
                    && property.writable()
                    && (!property.readable()
                        || browser
                            .editor
                            .fields
                            .first()
                            .is_some_and(|field| !field.input.value().is_empty())))
            {
                let value = browser
                    .editor
                    .fields
                    .first()
                    .map(|field| field.input.value())
                    .unwrap_or_default();
                Ok(CopyOp::Set {
                    service: context.service.clone(),
                    object: context.object.clone(),
                    iface: context.interface.clone(),
                    property: property.name.clone(),
                    signature: property.signature.clone(),
                    value: field_tokens(&property.signature, value)?,
                })
            } else {
                Ok(CopyOp::Get {
                    service: context.service.clone(),
                    object: context.object.clone(),
                    iface: context.interface.clone(),
                    property: property.name.clone(),
                })
            }
        }
        Category::Signals => {
            let signal = browser
                .members
                .signals
                .get(browser.selected())
                .ok_or_else(|| Error::Msg("No signal selected".into()))?;
            let rule =
                listen_rule(context, &ListenTarget::Signal(signal.name.clone()))?.to_string();
            Ok(CopyOp::Listen { rule })
        }
    }
}

fn current_op(state: &State) -> Option<CopyOp> {
    match state.page {
        Page::Browser => {
            let browser = state.browser.as_ref()?;
            if browser.focus == Focus::Result {
                browser.editor.last_op.clone()
            } else {
                draft_op(browser, None).ok()
            }
        }
        Page::Watch => state.watch().map(|watch| CopyOp::Listen {
            rule: watch.rule.clone(),
        }),
        Page::Expanded => state.expanded.as_ref()?.op.clone(),
        _ => None,
    }
}

fn current_text(state: &State) -> Option<String> {
    match state.page {
        Page::Browser => state
            .browser
            .as_ref()?
            .editor
            .outcome
            .as_ref()
            .map(Outcome::text),
        Page::Watch => state.watch().map(Watch::text),
        Page::Expanded => state
            .expanded
            .as_ref()
            .map(|expanded| expanded.text.clone()),
        _ => None,
    }
}

fn expand(state: &mut State) -> Option<Effect> {
    let text = current_text(state)?;
    if text.is_empty() {
        return None;
    }
    let op = match state.page {
        Page::Browser => state.browser.as_ref()?.editor.last_op.clone(),
        _ => current_op(state),
    };
    let title = match state.page {
        Page::Browser => {
            let browser = state.browser.as_ref()?;
            format!(
                "{}.{}",
                browser.context.interface,
                browser.member_name(browser.selected())
            )
        }
        Page::Watch => state.watch()?.target.name().into(),
        _ => return None,
    };
    state.expanded = Some(Expanded {
        origin: state.page,
        title,
        text,
        op,
        scroll: 0,
        hscroll: 0,
    });
    state.page = Page::Expanded;
    None
}

fn start_watch(state: &mut State) -> Option<Effect> {
    let browser = state.browser.as_ref()?;
    let context = browser.context.clone();
    let name = browser.member_name(browser.selected()).to_string();
    if name.is_empty() {
        return None;
    }
    let target = match browser.category {
        Category::Methods => ListenTarget::Method(name),
        Category::Properties => ListenTarget::Property(name),
        Category::Signals => ListenTarget::Signal(name),
    };
    let rule = match listen_rule(&context, &target) {
        Ok(rule) => rule.to_string(),
        Err(error) => {
            set_editor_error(state, error);
            return None;
        }
    };
    let id = state.request_id();
    state.watch = Some(Watch {
        id,
        context: context.clone(),
        target: target.clone(),
        rule,
        cancel: None,
        starting: true,
        stopped: false,
        error: None,
        events: Default::default(),
        total: 0,
        paused: None,
        scroll: 0,
        hscroll: 0,
        follow: true,
    });
    state.page = Page::Watch;
    Some(Effect::Listen(id, context, target))
}

fn watch_key(state: &mut State, key: KeyCode) -> Option<Effect> {
    match key {
        KeyCode::Enter => return expand(state),
        KeyCode::Char('p') => watch_action(state, 0),
        KeyCode::Char('s') => watch_action(state, 1),
        KeyCode::Char('x') => watch_action(state, 2),
        KeyCode::End => {
            if let Some(watch) = state.watch_mut() {
                watch.follow = true;
            }
        }
        _ => {
            if let Some(watch) = state.watch_mut() {
                if matches!(
                    key,
                    KeyCode::Up
                        | KeyCode::Down
                        | KeyCode::PageUp
                        | KeyCode::PageDown
                        | KeyCode::Home
                        | KeyCode::Char('j' | 'k')
                ) {
                    watch.follow = false;
                }
                scroll_text(&watch.text(), &mut watch.scroll, &mut watch.hscroll, key);
            }
        }
    }
    None
}

fn watch_action(state: &mut State, index: usize) {
    if index == 3 {
        back(state);
        return;
    }
    let Some(watch) = state.watch_mut() else {
        return;
    };
    match index {
        0 => {
            watch.paused = if watch.paused.is_some() {
                None
            } else {
                Some(watch.text())
            };
            watch.follow = watch.paused.is_none();
        }
        1 => {
            watch.cancel = None;
            watch.stopped = true;
            watch.starting = false;
        }
        2 => {
            watch.events.clear();
            watch.paused = watch.paused.as_ref().map(|_| String::new());
            watch.scroll = 0;
        }
        _ => {}
    }
}

pub fn scroll_text(text: &str, scroll: &mut usize, horizontal: &mut usize, key: KeyCode) {
    let last = text.lines().count().saturating_sub(1);
    match key {
        KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => *scroll = (*scroll + 1).min(last),
        KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
        KeyCode::PageDown => *scroll = (*scroll + 10).min(last),
        KeyCode::Home => {
            *scroll = 0;
            *horizontal = 0;
        }
        KeyCode::End => *scroll = last,
        KeyCode::Left | KeyCode::Char('h') => *horizontal = horizontal.saturating_sub(4),
        KeyCode::Right | KeyCode::Char('l') => {
            let longest = text
                .lines()
                .map(|line| ratatui::text::Line::raw(line).width())
                .max()
                .unwrap_or(0);
            *horizontal = (*horizontal + 4).min(longest.saturating_sub(1));
        }
        _ => {}
    }
}

fn move_index(index: &mut usize, count: usize, delta: i32) {
    *index = if delta < 0 {
        index.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        index
            .saturating_add(delta as usize)
            .min(count.saturating_sub(1))
    };
}

fn clear_filter(state: &mut State) {
    let query = state.query().to_string();
    if state.page == Page::Browser
        && let Some(browser) = &mut state.browser
        && browser.focus == Focus::Navigation
        && let Some(row) = browser
            .navigation(&query)
            .get(browser.nav_selected)
            .copied()
    {
        browser.nav_selected = browser
            .navigation("")
            .iter()
            .position(|candidate| *candidate == row)
            .unwrap_or(0);
    }
    state.filter = None;
}

fn filter_key(state: &mut State, key: KeyEvent) -> Option<Effect> {
    match key.code {
        KeyCode::Esc => {
            clear_filter(state);
            clamp_filter(state);
        }
        KeyCode::Enter => {
            let empty = match state.page {
                Page::Services => filtered_services(state).is_empty(),
                Page::Browser => state.browser.as_ref().is_none_or(|browser| {
                    if browser.focus == Focus::Navigation {
                        browser.navigation(state.query()).is_empty()
                    } else {
                        browser.visible_members(state.query()).is_empty()
                    }
                }),
                _ => false,
            };
            if empty {
                return None;
            }
            let effect = match state.page {
                Page::Services => open_service(state),
                Page::Browser
                    if state
                        .browser
                        .as_ref()
                        .is_some_and(|browser| browser.focus == Focus::Navigation) =>
                {
                    open_navigation(state)
                }
                Page::Browser => {
                    if let Some(browser) = &mut state.browser {
                        browser.focus = if browser.editor.fields.is_empty() {
                            Focus::Action(0)
                        } else {
                            Focus::Field(0)
                        };
                    }
                    None
                }
                _ => None,
            };
            state.filter = None;
            return effect;
        }
        KeyCode::Up | KeyCode::Down => {
            move_filtered(state, if key.code == KeyCode::Down { 1 } else { -1 })
        }
        _ => {
            if let Some(input) = &mut state.filter {
                input.handle_event(&Event::Key(key));
            }
            clamp_filter(state);
        }
    }
    None
}

fn filtered_services(state: &State) -> Vec<usize> {
    let query = state.query().to_ascii_lowercase();
    state
        .services
        .iter()
        .enumerate()
        .filter(|(_, service)| service.name.to_ascii_lowercase().contains(&query))
        .map(|(index, _)| index)
        .collect()
}

fn clamp_filter(state: &mut State) {
    let query = state.query().to_string();
    match state.page {
        Page::Services => {
            let view = filtered_services(state);
            if !view.contains(&state.service_selected) {
                state.service_selected = view.first().copied().unwrap_or(0);
            }
        }
        Page::Browser => {
            if let Some(browser) = &mut state.browser {
                if browser.focus == Focus::Navigation {
                    browser.nav_selected = browser
                        .nav_selected
                        .min(browser.navigation(&query).len().saturating_sub(1));
                } else {
                    let view = browser.visible_members(&query);
                    if !view.contains(&browser.selected()) {
                        browser.selected[browser.category as usize] =
                            view.first().copied().unwrap_or(0);
                        browser.select_member();
                    }
                }
            }
        }
        _ => {}
    }
}

fn move_filtered(state: &mut State, delta: i32) {
    let query = state.query().to_string();
    if state.page == Page::Services {
        let view = filtered_services(state);
        let mut index = view
            .iter()
            .position(|index| *index == state.service_selected)
            .unwrap_or(0);
        move_index(&mut index, view.len(), delta);
        if let Some(selected) = view.get(index) {
            state.service_selected = *selected;
        }
    } else if let Some(browser) = &mut state.browser {
        if browser.focus == Focus::Navigation {
            let count = browser.navigation(&query).len();
            move_index(&mut browser.nav_selected, count, delta);
        } else {
            let view = browser.visible_members(&query);
            let mut index = view
                .iter()
                .position(|index| *index == browser.selected())
                .unwrap_or(0);
            move_index(&mut index, view.len(), delta);
            if let Some(selected) = view.get(index)
                && *selected != browser.selected()
            {
                browser.selected[browser.category as usize] = *selected;
                browser.select_member();
            }
        }
    }
}

fn open_copy(state: &mut State, op: CopyOp) {
    state.clipboard_request = None;
    let commands = Tool::ALL
        .into_iter()
        .map(|tool| (tool, generate(&op, &state.bus, tool)))
        .collect();
    state.popup = Some(CopyAsPopup {
        commands,
        selected: 0,
        status: None,
        scroll: 0,
    });
}

fn copy_text(state: &mut State, text: String) -> Effect {
    let id = state.request_id();
    state.clipboard_request = Some(id);
    Effect::Clipboard(id, text)
}

fn popup_key(state: &mut State, key: KeyCode) -> Option<Effect> {
    if key == KeyCode::Esc {
        state.popup = None;
        return None;
    }
    let popup = state.popup.as_mut()?;
    match key {
        KeyCode::Left | KeyCode::Up | KeyCode::Char('h' | 'k') => {
            move_index(&mut popup.selected, popup.commands.len(), -1);
            popup.scroll = 0;
            popup.status = None;
        }
        KeyCode::Right | KeyCode::Down | KeyCode::Tab | KeyCode::Char('l' | 'j') => {
            popup.selected = (popup.selected + 1) % popup.commands.len();
            popup.scroll = 0;
            popup.status = None;
        }
        KeyCode::PageUp => popup.scroll = popup.scroll.saturating_sub(10),
        KeyCode::PageDown => {
            let length = popup
                .commands
                .get(popup.selected)
                .and_then(|(_, cmd)| cmd.as_ref())
                .map_or(0, String::len);
            popup.scroll = (popup.scroll + 10).min(length);
        }
        KeyCode::Enter => {
            if let Some(command) = popup
                .commands
                .get(popup.selected)
                .and_then(|(_, command)| command.clone())
            {
                popup.status = Some("Copying...".into());
                return Some(copy_text(state, command));
            }
        }
        _ => {}
    }
    None
}

fn mouse(state: &mut State, event: MouseEvent) -> Option<Effect> {
    state.key_prefix = None;
    if state.help_open {
        return None;
    }
    if matches!(
        event.kind,
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
    ) {
        if state.popup.is_some() {
            return None;
        }
        let key = if event.kind == MouseEventKind::ScrollDown {
            KeyCode::Down
        } else {
            KeyCode::Up
        };
        return if state.filter.is_some() {
            move_filtered(state, if key == KeyCode::Down { 1 } else { -1 });
            None
        } else {
            key_event(state, KeyEvent::new(key, KeyModifiers::NONE))
        };
    }
    if event.kind != MouseEventKind::Down(MouseButton::Left) {
        return None;
    }
    let target = state
        .click_targets
        .iter()
        .rev()
        .find(|(area, _)| area.contains((event.column, event.row).into()))
        .map(|(_, target)| target.clone())?;
    if state.popup.is_some() && !matches!(target, ClickTarget::PopupTool(_)) {
        return None;
    }
    if !matches!(
        target,
        ClickTarget::Tab(_) | ClickTarget::CloseTab(_) | ClickTarget::NewTab
    ) && let Some(browser) = state.browser.as_mut()
    {
        browser.editor.editing = false;
    }
    match target {
        ClickTarget::Service(index) => {
            let already = state.service_selected == index;
            state.service_selected = index;
            if already {
                return open_service(state);
            }
        }
        ClickTarget::Navigation(index) => {
            if state
                .browser
                .as_ref()
                .is_some_and(|browser| browser.focus != Focus::Navigation)
            {
                clear_filter(state);
            }
            let browser = state.browser.as_mut()?;
            let already = browser.focus == Focus::Navigation && browser.nav_selected == index;
            browser.focus = Focus::Navigation;
            browser.nav_selected = index;
            if already {
                return open_navigation(state);
            }
        }
        ClickTarget::Category(category) => {
            clear_filter(state);
            let browser = state.browser.as_mut()?;
            if browser.category != category {
                browser.category = category;
                browser.select_member();
            }
            browser.focus = Focus::Members;
        }
        ClickTarget::Member(index) => {
            if state.browser.as_ref().is_some_and(|browser| {
                browser.focus != Focus::Members || browser.selected() == index
            }) {
                clear_filter(state);
            }
            let browser = state.browser.as_mut()?;
            let already = browser.focus == Focus::Members && browser.selected() == index;
            if browser.selected() != index {
                browser.selected[browser.category as usize] = index;
                browser.select_member();
            }
            browser.focus = if already {
                if browser.editor.fields.is_empty() {
                    Focus::Action(0)
                } else {
                    Focus::Field(0)
                }
            } else {
                Focus::Members
            };
        }
        ClickTarget::Field(index) => {
            clear_filter(state);
            state.browser.as_mut()?.focus = Focus::Field(index);
        }
        ClickTarget::Action(index) => {
            clear_filter(state);
            state.browser.as_mut()?.focus = Focus::Action(index);
            return activate(state, index);
        }
        ClickTarget::Result => {
            clear_filter(state);
            state.browser.as_mut()?.focus = Focus::Result;
        }
        ClickTarget::Tab(index) => state.select_tab(index),
        ClickTarget::CloseTab(index) => state.close_tab_at(index),
        ClickTarget::NewTab => state.new_tab(),
        ClickTarget::WatchAction(index) => watch_action(state, index),
        ClickTarget::PopupTool(index) => {
            let popup = state.popup.as_mut()?;
            if popup.selected == index {
                return popup_key(state, KeyCode::Enter);
            }
            popup.selected = index;
            popup.scroll = 0;
            popup.status = None;
        }
    }
    None
}

fn members_of(node: &Node<'_>, interface: &str) -> Members {
    let Some(interface) = node
        .interfaces()
        .iter()
        .find(|iface| iface.name().as_ref() == interface)
    else {
        return Members::default();
    };
    let methods = interface
        .methods()
        .iter()
        .map(|method| {
            let args: Vec<_> = method
                .args()
                .iter()
                .filter(|arg| arg.direction() != Some(ArgDirection::Out))
                .map(|arg| (arg.name().unwrap_or("").to_string(), sig_str(arg.ty())))
                .collect();
            let signature = args
                .iter()
                .map(|(_, signature)| signature.as_str())
                .collect();
            let returns = method
                .args()
                .iter()
                .filter(|arg| arg.direction() == Some(ArgDirection::Out))
                .map(|arg| sig_str(arg.ty()))
                .collect();
            Method {
                name: method.name().to_string(),
                signature,
                returns,
                args,
            }
        })
        .collect();
    let properties = interface
        .properties()
        .iter()
        .map(|property| Property {
            name: property.name().to_string(),
            signature: sig_str(property.ty()),
            access: access_str(property.access()).into(),
        })
        .collect();
    let signals = interface
        .signals()
        .iter()
        .map(|signal| Signal {
            name: signal.name().to_string(),
            signature: signal.args().iter().map(|arg| sig_str(arg.ty())).collect(),
        })
        .collect();
    Members {
        methods,
        properties,
        signals,
    }
}

/// Signal/property rules constrain sender as well as path and interface.
/// PropertiesChanged additionally constrains arg0 to the selected interface.
/// Method calls constrain destination (the sender is the caller).
pub fn listen_rule(context: &Context, target: &ListenTarget) -> Result<zbus::MatchRule<'static>> {
    let builder = zbus::MatchRule::builder().path(context.object.as_str())?;
    let rule = match target {
        ListenTarget::Signal(member) => builder
            .msg_type(zbus::message::Type::Signal)
            .sender(context.service.as_str())?
            .interface(context.interface.as_str())?
            .member(member.as_str())?
            .build(),
        ListenTarget::Property(_) => builder
            .msg_type(zbus::message::Type::Signal)
            .sender(context.service.as_str())?
            .interface("org.freedesktop.DBus.Properties")?
            .member("PropertiesChanged")?
            .arg(0, context.interface.as_str())?
            .build(),
        ListenTarget::Method(member) => {
            // The async subscription setup resolves well-known destinations
            // to unique names, which are the only names zbus accepts here.
            let builder = if context.service.starts_with(':') {
                builder.destination(context.service.as_str())?
            } else {
                builder
            };
            builder
                .msg_type(zbus::message::Type::MethodCall)
                .interface(context.interface.as_str())?
                .member(member.as_str())?
                .build()
        }
    };
    Ok(rule.into_owned())
}
