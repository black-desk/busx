// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Responsive browser and inline inspector. Narrow terminals show navigation
//! and content separately; inputs reuse tui-input's visual cursor/scroll model.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::dbus::conn::Bus;
use crate::tui::state::*;

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

fn active() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}
fn selected() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

pub fn render(
    frame: &mut Frame,
    state: &State,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut [usize; 3],
) {
    targets.clear();
    let area = frame.area();
    if area.width < 32 || area.height < 10 {
        frame.render_widget(
            Paragraph::new("Terminal too small\nNeed at least 32 x 10\nCtrl-C to quit"),
            area,
        );
        return;
    }
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(if state.page == Page::Services { 1 } else { 2 }),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let bus = match state.bus {
        Bus::Session => "Session bus",
        Bus::System => "System bus",
        Bus::Other(_) => "Custom bus",
    };
    tab_bar(frame, line(header, 0), state, bus, targets);
    let context = match state.page {
        Page::Services => String::new(),
        Page::Browser => state
            .browser
            .as_ref()
            .map(|browser| {
                if browser.focus == Focus::Navigation
                    && let Some(row) = browser.navigation(state.query()).get(browser.nav_selected)
                {
                    let name = match *row {
                        NavRow::Object(index) => &browser.objects[index].path,
                        NavRow::Interface(object, interface) => {
                            &browser.objects[object].interfaces[interface]
                        }
                    };
                    return truncate(name, header.width as usize);
                }
                breadcrumb(&browser.context, header.width as usize)
            })
            .unwrap_or_default(),
        Page::Watch => state
            .watch()
            .map(|watch| breadcrumb(&watch.context, header.width as usize))
            .unwrap_or_default(),
        Page::Expanded => state
            .expanded
            .as_ref()
            .map(|expanded| truncate(&expanded.title, header.width as usize))
            .unwrap_or_default(),
    };
    if state.page != Page::Services {
        frame.render_widget(
            Paragraph::new(context).style(Style::default().fg(MUTED)),
            line(header, 1),
        );
    }
    let (content, filter) = if state.filter.is_some() {
        let [content, filter] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(body);
        (content, Some(filter))
    } else {
        (body, None)
    };
    scroll[2] = 0;
    match state.page {
        Page::Services => services(frame, content, state, targets, &mut scroll[0]),
        Page::Browser => {
            if let Some(browser) = &state.browser {
                browser_view(frame, content, browser, state.query(), targets, scroll);
            }
        }
        Page::Watch => watch_view(frame, content, state, targets, scroll),
        Page::Expanded => {
            if let Some(expanded) = &state.expanded {
                let block = Block::bordered().title("Result · y copy · c copy command");
                let inner = block.inner(content);
                frame.render_widget(block, content);
                scroll[2] = result_text(
                    frame,
                    inner,
                    &expanded.text,
                    expanded.scroll,
                    expanded.hscroll,
                    false,
                );
            }
        }
    }
    if let (Some(area), Some(input)) = (filter, &state.filter) {
        let [label, field] =
            Layout::horizontal([Constraint::Length(9), Constraint::Min(1)]).areas(area);
        frame.render_widget(Paragraph::new(" Filter /"), label);
        input_widget(frame, field, input, true, true);
    }
    let editing = state.filter.is_some()
        || (state.page == Page::Browser
            && state.browser.as_ref().is_some_and(|browser| {
                browser.editor.editing && matches!(browser.focus, Focus::Field(_))
            }));
    let hint = if state.filter.is_some() {
        "Type to filter · Enter open · Esc leave filter"
    } else if editing {
        "INSERT · Tab next · Enter done · Esc normal"
    } else {
        match state.page {
            Page::Services => "j/k select · Enter open · / filter",
            Page::Browser => match state.browser.as_ref().map(|browser| browser.focus) {
                Some(Focus::Navigation) => "j/k select · h/l fold · Enter open · / filter",
                Some(Focus::Action(_)) => "h/l action · k inputs · j result · Enter run",
                Some(Focus::Result) => "hjkl scroll · Enter expand · y copy",
                Some(Focus::Field(_)) => "NORMAL · i edit · j/k focus · Enter actions",
                _ => "j/k select · h/l category · Enter details · / filter",
            },
            Page::Watch => "hjkl scroll · p pause · s stop · x clear · Enter expand",
            Page::Expanded => "hjkl scroll · PgUp/PgDn page · y copy · Esc back",
        }
    };
    let pending = state.key_prefix.map(|prefix| match prefix {
        KeyPrefix::Count(count) => format!("{count}"),
        KeyPrefix::G(Some(count)) => format!("{count}g"),
        KeyPrefix::G(None) => "g".into(),
    });
    let status = state
        .status
        .as_deref()
        .or(pending.as_deref())
        .unwrap_or(hint);
    let help = if editing { "Esc then ? help" } else { "? help" };
    let help_width = help.len() as u16;
    let [message, help_area] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(help_width + 2)]).areas(footer);
    frame.render_widget(
        Paragraph::new(truncate(status, message.width as usize)).style(Style::default().fg(MUTED)),
        message,
    );
    frame.render_widget(
        Paragraph::new(format!("· {help}")).style(Style::default().fg(MUTED)),
        help_area,
    );
    if let Some(popup) = &state.popup {
        targets.clear();
        frame.render_widget(Clear, area);
        copy_popup(frame, area, popup, targets);
    }
    if state.help_open {
        targets.clear();
        frame.render_widget(Clear, area);
        let help = "busx · keyboard\n\nt            new browsing tab\nq            close current tab (last tab exits)\ngt / gT      next / previous tab\n{number}gt   jump to tab number\nTab / S-Tab  next / previous pane or form control\ngg / G       first / last item or result line\nj / k        down / up\nh / l        member categories · fold objects · actions\nEnter        inspect · activate focused action\n/            filter services, navigation, or members\nr            refresh service/tree/property values\nn            focus navigation (also on narrow screens)\nc            copy command (choose a tool)\ny            copy result or watch text\nEsc          leave input · back to navigation · back\nCtrl-C       close current tab, including from inputs\n\nNavigation: /App47 is an object; @ marks an interface.\nInterface names retain their full namespace.\nLong names are clipped; the selected full name is above.\nArrow keys remain aliases for hjkl.\n\nInputs: i enters INSERT; Esc returns to NORMAL.\nj/k selects fields in NORMAL; Tab moves focus.\nEnter finishes editing and focuses actions without running.\nIn INSERT, q, t and other characters enter text normally.\nFilters accept text immediately after /.\nInput syntax follows busctl (see man busctl).\nBasic values: enter verbatim, including spaces.\nArrays/dicts: count, then elements/key-value pairs.\nExample: as takes 2 hello world; empty arrays take 0.\nStructs: fields in order; variants: signature then value.\nQuote container tokens with spaces, e.g. 1 \"hello world\".\nCall and Write run only when their action is activated.\n\nWatch uses the current tab for one subscription.\np pauses display, s stops but keeps retained messages,\nx clears retained events, End or G follows latest.\nEsc returns to browsing and stops this subscription.\nq closes the tab and stops its subscription.\nUse t to browse elsewhere while this tab listens.\nOnly the latest 1000 events are retained per watch.\n\nResult: hjkl scroll, PgUp/PgDn page, gg/G first/last.\nEsc returns from an expanded result to its source page.\n\nEsc or ? closes help";
        frame.render_widget(
            Paragraph::new(help)
                .wrap(Wrap { trim: false })
                .scroll((state.help_scroll as u16, 0))
                .block(Block::bordered().title("Help · arrows scroll · Esc close")),
            area,
        );
    }
}

fn services(
    frame: &mut Frame,
    area: Rect,
    state: &State,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut usize,
) {
    let title = if state.services_loading {
        "Services · loading...".to_string()
    } else {
        format!("Services ({})", state.services.len())
    };
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if let Some(error) = &state.services_error {
        frame.render_widget(
            Paragraph::new(error_text(error))
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(Color::Red)),
            inner,
        );
        return;
    }
    if inner.height == 0 {
        return;
    }
    let name_width = (inner.width as usize).saturating_sub(26);
    let wide = inner.width >= 50;
    let header = if wide {
        format!(
            "  {}  {:>7}  {:>13}",
            pad("Name", name_width.saturating_sub(2)),
            "PID",
            "Process"
        )
    } else {
        "  Name".into()
    };
    frame.render_widget(
        Paragraph::new(header).style(Style::default().fg(MUTED)),
        line(inner, 0),
    );
    let query = state.query().to_ascii_lowercase();
    let rows: Vec<_> = state
        .services
        .iter()
        .enumerate()
        .filter(|(_, service)| service.name.to_ascii_lowercase().contains(&query))
        .map(|(index, service)| {
            let text = if wide {
                format!(
                    "{}  {:>7}  {:>13}",
                    pad(&service.name, name_width.saturating_sub(2)),
                    service
                        .pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "-".into()),
                    truncate(service.process.as_deref().unwrap_or("-"), 13)
                )
            } else {
                service.name.clone()
            };
            (index, text)
        })
        .collect();
    list(
        frame,
        below(inner, 1),
        rows,
        state.service_selected,
        true,
        scroll,
        targets,
        ClickTarget::Service,
    );
}

fn browser_view(
    frame: &mut Frame,
    area: Rect,
    browser: &Browser,
    query: &str,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut [usize; 3],
) {
    if area.width >= 80 {
        let [nav, content] =
            Layout::horizontal([Constraint::Length(25), Constraint::Min(1)]).areas(area);
        navigation(
            frame,
            nav,
            browser,
            if browser.focus == Focus::Navigation {
                query
            } else {
                ""
            },
            targets,
            &mut scroll[0],
        );
        workspace(
            frame,
            content,
            browser,
            if browser.focus == Focus::Members {
                query
            } else {
                ""
            },
            targets,
            scroll,
        );
    } else if browser.focus == Focus::Navigation {
        navigation(frame, area, browser, query, targets, &mut scroll[0]);
    } else {
        workspace(frame, area, browser, query, targets, scroll);
    }
}

fn navigation(
    frame: &mut Frame,
    area: Rect,
    browser: &Browser,
    query: &str,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut usize,
) {
    let block = Block::bordered()
        .title(if browser.tree_loading {
            "Navigation · loading..."
        } else {
            "Navigation"
        })
        .border_style(if browser.focus == Focus::Navigation {
            active()
        } else {
            Style::default()
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [items, legend] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    frame.render_widget(
        Paragraph::new("/ object · @ interface").style(Style::default().fg(MUTED)),
        legend,
    );
    let rows = browser
        .navigation(query)
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let text = match *row {
                NavRow::Object(object) => {
                    let object = &browser.objects[object];
                    let indent = "  ".repeat(object.depth.min(3));
                    let name = object.path.rsplit('/').next().unwrap_or("");
                    format!(
                        "{indent}{} /{name}",
                        if object.expanded { "▾" } else { "▸" }
                    )
                }
                NavRow::Interface(object, interface) => {
                    let object = &browser.objects[object];
                    let indent = "  ".repeat((object.depth + 1).min(4));
                    let name = &object.interfaces[interface];
                    let available = (items.width as usize).saturating_sub(indent.len() + 4);
                    format!("{indent}@ {}", truncate(name, available))
                }
            };
            (index, text)
        })
        .collect();
    list(
        frame,
        items,
        rows,
        browser.nav_selected,
        browser.focus == Focus::Navigation,
        scroll,
        targets,
        ClickTarget::Navigation,
    );
}

fn workspace(
    frame: &mut Frame,
    area: Rect,
    browser: &Browser,
    query: &str,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut [usize; 3],
) {
    if let Some(error) = &browser.error {
        frame.render_widget(
            Paragraph::new(error_text(error))
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title("Unavailable · r refresh")),
            area,
        );
        return;
    }
    if browser.context.interface.is_empty() {
        let row = browser.navigation("").get(browser.nav_selected).copied();
        let text = match row {
            Some(NavRow::Object(index)) => format!("{}\n\n{} interface(s)\n\nExpand the object and select an interface.\nEnter opens its members.", browser.objects[index].path, browser.objects[index].interfaces.len()),
            Some(NavRow::Interface(object, interface)) => format!("{}\n\n{}\n\nEnter opens properties, methods and signals.", browser.objects[object].path, browser.objects[object].interfaces[interface]),
            None if browser.tree_loading => "Loading service objects...".into(),
            _ => "No visible objects or interfaces.\n\nUse --show-standard-interfaces to include\nstandard D-Bus interfaces.".into(),
        };
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title("Object")),
            area,
        );
        return;
    }
    if browser.interface_loading {
        frame.render_widget(
            Paragraph::new("Loading interface...").block(Block::bordered()),
            area,
        );
        return;
    }
    let top_height = if area.height >= 15 {
        (area.height * 2 / 5).clamp(6, 10)
    } else {
        5
    };
    let [members, detail] =
        Layout::vertical([Constraint::Length(top_height), Constraint::Min(1)]).areas(area);
    member_list(frame, members, browser, query, targets, &mut scroll[1]);
    if !query.is_empty()
        && browser.focus == Focus::Members
        && browser.visible_members(query).is_empty()
    {
        frame.render_widget(
            Paragraph::new("No matching members").block(Block::bordered().title("Details")),
            detail,
        );
    } else {
        inspector(frame, detail, browser, targets, &mut scroll[2]);
    }
}

fn member_list(
    frame: &mut Frame,
    area: Rect,
    browser: &Browser,
    query: &str,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut usize,
) {
    let block = Block::bordered().border_style(if browser.focus == Focus::Members {
        active()
    } else {
        Style::default()
    });
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return;
    }
    let mut x = inner.x;
    for category in Category::ALL {
        let count = match category {
            Category::Properties => browser.members.properties.len(),
            Category::Methods => browser.members.methods.len(),
            Category::Signals => browser.members.signals.len(),
        };
        let short = inner.width < 44;
        let name = if short {
            match category {
                Category::Properties => "Props",
                Category::Methods => "Methods",
                Category::Signals => "Signals",
            }
        } else {
            category.name()
        };
        let label = format!(" {name} {count} ");
        let width = (label.len() as u16).min(inner.right().saturating_sub(x));
        let rect = Rect::new(x, inner.y, width, 1);
        frame.render_widget(
            Paragraph::new(label).style(if browser.category == category {
                selected()
            } else {
                Style::default().fg(MUTED)
            }),
            rect,
        );
        targets.push((rect, ClickTarget::Category(category)));
        x += width;
    }
    let content = below(inner, 1);
    let available = (content.width as usize).saturating_sub(2);
    let name_width = (available / 3).clamp(8, 22);
    let header = match browser.category {
        Category::Properties => format!(
            "  {} {} Mode Value",
            pad("Name", name_width),
            pad("Type", 6)
        ),
        Category::Methods => format!("  {} {} Out", pad("Method", name_width), pad("In", 8)),
        Category::Signals => format!("  {} Arguments", pad("Signal", name_width)),
    };
    frame.render_widget(
        Paragraph::new(header).style(Style::default().fg(MUTED)),
        line(content, 0),
    );
    let rows = browser
        .visible_members(query)
        .into_iter()
        .map(|index| {
            let text = match browser.category {
                Category::Properties => {
                    let property = &browser.members.properties[index];
                    let value = if !property.readable() {
                        "(write-only)"
                    } else {
                        browser
                            .value(&property.name)
                            .unwrap_or(if browser.properties_loading {
                                "..."
                            } else {
                                "-"
                            })
                    };
                    format!(
                        "{} {} {:<4} {}",
                        pad(&property.name, name_width),
                        pad(&property.signature, 6),
                        property.mode(),
                        value
                    )
                }
                Category::Methods => {
                    let method = &browser.members.methods[index];
                    format!(
                        "{} {} {}",
                        pad(&method.name, name_width),
                        pad(
                            if method.signature.is_empty() {
                                "-"
                            } else {
                                &method.signature
                            },
                            8
                        ),
                        if method.returns.is_empty() {
                            "-"
                        } else {
                            &method.returns
                        }
                    )
                }
                Category::Signals => {
                    let signal = &browser.members.signals[index];
                    format!("{} {}", pad(&signal.name, name_width), signal.signature)
                }
            };
            (index, text)
        })
        .collect();
    list(
        frame,
        below(content, 1),
        rows,
        browser.selected(),
        browser.focus == Focus::Members,
        scroll,
        targets,
        ClickTarget::Member,
    );
}

fn inspector(
    frame: &mut Frame,
    area: Rect,
    browser: &Browser,
    targets: &mut Vec<(Rect, ClickTarget)>,
    result_scroll: &mut usize,
) {
    let name = browser.member_name(browser.selected());
    let title = if name.is_empty() { "Details" } else { name };
    let block = Block::bordered().title(title).border_style(
        if !matches!(browser.focus, Focus::Navigation | Focus::Members) {
            active()
        } else {
            Style::default()
        },
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return;
    }
    if name.is_empty() {
        frame.render_widget(
            Paragraph::new(format!(
                "No {} on this interface",
                browser.category.name().to_ascii_lowercase()
            )),
            inner,
        );
        return;
    }
    let mut y = 0;
    match browser.category {
        Category::Properties => {
            let property = &browser.members.properties[browser.selected()];
            frame.render_widget(
                Paragraph::new(format!("{} · {}", property.signature, property.mode()))
                    .style(Style::default().fg(MUTED)),
                line(inner, y),
            );
            y += 1;
            if property.readable() {
                frame.render_widget(
                    Paragraph::new(format!(
                        "Current  {}",
                        browser
                            .value(name)
                            .unwrap_or(if browser.properties_loading {
                                "loading..."
                            } else {
                                "unavailable"
                            })
                    )),
                    line(inner, y),
                );
                y += 1;
            }
        }
        Category::Methods => {
            let method = &browser.members.methods[browser.selected()];
            frame.render_widget(
                Paragraph::new(format!(
                    "In: {} · Out: {}",
                    if method.signature.is_empty() {
                        "none"
                    } else {
                        &method.signature
                    },
                    if method.returns.is_empty() {
                        "none"
                    } else {
                        &method.returns
                    }
                ))
                .style(Style::default().fg(MUTED)),
                line(inner, y),
            );
            y += 1;
        }
        Category::Signals => {
            let signal = &browser.members.signals[browser.selected()];
            frame.render_widget(
                Paragraph::new(format!(
                    "Arguments: {}",
                    if signal.signature.is_empty() {
                        "none"
                    } else {
                        &signal.signature
                    }
                ))
                .style(Style::default().fg(MUTED)),
                line(inner, y),
            );
            y += 1;
            frame.render_widget(
                Paragraph::new("Watch listens in this tab; t opens another.")
                    .wrap(Wrap { trim: false }),
                line(inner, y),
            );
            y += 1;
        }
    }
    let fields_space = inner.height.saturating_sub(y + 4).max(1) as usize;
    let label_width = browser
        .editor
        .fields
        .iter()
        .map(|field| Line::raw(field.label.as_str()).width() + 2)
        .max()
        .unwrap_or(0)
        .min((inner.width / 2) as usize) as u16;
    let start = if let Focus::Field(index) = browser.focus {
        index.saturating_sub(fields_space - 1)
    } else {
        0
    };
    for (index, field) in browser
        .editor
        .fields
        .iter()
        .enumerate()
        .skip(start)
        .take(fields_space)
    {
        if y >= inner.height {
            break;
        }
        let row = line(inner, y);
        let [label, input] =
            Layout::horizontal([Constraint::Length(label_width), Constraint::Min(1)]).areas(row);
        frame.render_widget(
            Paragraph::new(truncate(
                &field.label,
                label.width.saturating_sub(1) as usize,
            )),
            label,
        );
        input_widget(
            frame,
            input,
            &field.input,
            browser.focus == Focus::Field(index),
            browser.editor.editing && browser.focus == Focus::Field(index),
        );
        targets.push((row, ClickTarget::Field(index)));
        y += 1;
    }
    let containers = match browser.category {
        Category::Methods => {
            browser
                .members
                .methods
                .get(browser.selected())
                .is_some_and(|method| {
                    method
                        .args
                        .iter()
                        .any(|(_, sig)| sig.len() > 1 || sig == "v")
                })
        }
        Category::Properties => browser
            .members
            .properties
            .get(browser.selected())
            .is_some_and(|property| property.signature.len() > 1 || property.signature == "v"),
        Category::Signals => false,
    };
    if containers && !browser.editor.fields.is_empty() && y < inner.height {
        frame.render_widget(
            Paragraph::new("Input syntax: Esc, then ? for help").style(Style::default().fg(MUTED)),
            line(inner, y),
        );
        y += 1;
    }
    let actions = browser.actions();
    let mut x = inner.x;
    for (index, action) in actions.iter().enumerate() {
        let label = format!(" {} ", action.label());
        let width = (label.len() as u16).min(inner.width);
        if x + width > inner.right() {
            x = inner.x;
            y += 1;
        }
        if y >= inner.height {
            break;
        }
        let rect = Rect::new(x, inner.y + y, width, 1);
        let style = if browser.focus == Focus::Action(index) {
            selected().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(ACCENT)
        };
        frame.render_widget(Paragraph::new(label).style(style), rect);
        targets.push((rect, ClickTarget::Action(index)));
        x += width + 1;
    }
    y += 1;
    if y >= inner.height {
        return;
    }
    let result_area = below(inner, y);
    targets.push((result_area, ClickTarget::Result));
    if browser.editor.pending.is_some() {
        frame.render_widget(
            Paragraph::new("Working...").style(Style::default().fg(Color::Yellow)),
            result_area,
        );
    } else if let Some(outcome) = &browser.editor.outcome {
        let error = outcome.error.is_some();
        frame.render_widget(
            Paragraph::new(if error { "ERROR" } else { "SUCCESS" })
                .style(Style::default().fg(if error { Color::Red } else { Color::Green })),
            line(result_area, 0),
        );
        *result_scroll = result_text(
            frame,
            below(result_area, 1),
            &outcome.text(),
            browser.editor.scroll,
            browser.editor.hscroll,
            error,
        );
    } else if let Some(error) = &browser.properties_error {
        frame.render_widget(
            Paragraph::new(error_text(error))
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: false }),
            result_area,
        );
    }
}

fn watch_view(
    frame: &mut Frame,
    area: Rect,
    state: &State,
    targets: &mut Vec<(Rect, ClickTarget)>,
    scroll: &mut [usize; 3],
) {
    let Some(watch) = state.watch() else {
        return;
    };
    let block = Block::bordered()
        .title(format!(
            "{} · {} · {} events",
            watch.target.name(),
            watch.status(),
            watch.total
        ))
        .border_style(active());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [events, status, buttons] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    if let Some(error) = &watch.error {
        frame.render_widget(
            Paragraph::new(error_text(error))
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(Color::Red)),
            events,
        );
    } else {
        let text = watch.text();
        if text.is_empty() {
            frame.render_widget(
                Paragraph::new(if watch.starting {
                    "Starting subscription..."
                } else if watch.stopped {
                    "Subscription stopped."
                } else {
                    "Waiting for events..."
                }),
                events,
            );
        } else {
            let offset = if watch.follow && watch.paused.is_none() {
                text.lines().count().saturating_sub(events.height as usize)
            } else {
                watch.scroll
            };
            scroll[2] = result_text(frame, events, &text, offset, watch.hscroll, false);
        }
    }
    let mode = if watch.paused.is_some() {
        "PAUSED DISPLAY"
    } else if watch.follow {
        "Following latest"
    } else {
        "End: follow latest"
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{mode} · retained {}/{}",
            watch.events.len(),
            EVENT_LIMIT
        ))
        .style(Style::default().fg(MUTED)),
        status,
    );
    let mut x = buttons.x;
    for (index, label) in [
        if watch.paused.is_some() {
            "Resume"
        } else {
            "Pause"
        },
        "Stop",
        "Clear",
        "Back",
    ]
    .iter()
    .enumerate()
    {
        let label = format!(" {label} ");
        let width = (label.len() as u16).min(buttons.right().saturating_sub(x));
        let rect = Rect::new(x, buttons.y, width, buttons.height);
        frame.render_widget(Paragraph::new(label).style(active()), rect);
        targets.push((rect, ClickTarget::WatchAction(index)));
        x += width + 1;
    }
}

/// Keep the active tab visible even when there are more tabs than terminal
/// columns. Tab numbers remain absolute when the strip scrolls horizontally.
fn tab_bar(
    frame: &mut Frame,
    area: Rect,
    state: &State,
    bus: &str,
    targets: &mut Vec<(Rect, ClickTarget)>,
) {
    let prefix = if area.width >= 60 {
        format!("busx · {bus}  ")
    } else {
        "busx ".into()
    };
    let prefix_width = Line::raw(prefix.as_str()).width() as u16;
    frame.render_widget(
        Paragraph::new(prefix).style(active()),
        Rect::new(area.x, area.y, prefix_width, 1),
    );
    let new_tab = Rect::new(area.right().saturating_sub(3), area.y, 3, 1);
    frame.render_widget(Paragraph::new(" + ").style(active()), new_tab);
    targets.push((new_tab, ClickTarget::NewTab));
    let available = new_tab.x.saturating_sub(area.x + prefix_width + 2) as usize;
    let visible = (available / 10).max(1).min(state.tabs.len());
    let first = state
        .active_tab
        .saturating_sub(visible / 2)
        .min(state.tabs.len() - visible);
    let width = (available / visible).min(28) as u16;
    let mut x = area.x + prefix_width;
    if first > 0 {
        frame.render_widget(Paragraph::new("‹"), Rect::new(x, area.y, 1, 1));
    }
    x += 1;
    for (index, tab) in state.tabs.iter().enumerate().skip(first).take(visible) {
        let rect = Rect::new(x, area.y, width, 1);
        let title_width = width.saturating_sub(3);
        let title = truncate(
            &format!(" {} {} ", index + 1, tab.title()),
            title_width as usize,
        );
        let style = if index == state.active_tab {
            selected()
        } else {
            Style::default().fg(MUTED)
        };
        frame.render_widget(Paragraph::new("").style(style), rect);
        let title_rect = Rect::new(x, area.y, title_width, 1);
        frame.render_widget(Paragraph::new(title).style(style), title_rect);
        targets.push((title_rect, ClickTarget::Tab(index)));
        let close_rect = Rect::new(x + title_width, area.y, width.min(3), 1);
        frame.render_widget(Paragraph::new(" × ").style(style), close_rect);
        targets.push((close_rect, ClickTarget::CloseTab(index)));
        x += width;
    }
    if first + visible < state.tabs.len() && x < new_tab.x {
        frame.render_widget(Paragraph::new("›"), Rect::new(x, area.y, 1, 1));
    }
}

fn copy_popup(
    frame: &mut Frame,
    area: Rect,
    popup: &CopyAsPopup,
    targets: &mut Vec<(Rect, ClickTarget)>,
) {
    let block = Block::bordered().title("Copy command · Esc close");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [tools, preview, status, hint] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .areas(inner);
    let mut x = tools.x;
    let mut y = tools.y;
    for (index, (tool, command)) in popup.commands.iter().enumerate() {
        let label = format!(" {} ", tool.name());
        let width = label.len() as u16;
        if x + width > tools.right() {
            x = tools.x;
            y += 1;
        }
        if y >= tools.bottom() {
            break;
        }
        let rect = Rect::new(x, y, width.min(tools.right().saturating_sub(x)), 1);
        frame.render_widget(
            Paragraph::new(label).style(if popup.selected == index {
                selected()
            } else if command.is_none() {
                Style::default().fg(MUTED)
            } else {
                Style::default()
            }),
            rect,
        );
        targets.push((rect, ClickTarget::PopupTool(index)));
        x += width + 1;
    }
    let command = popup
        .commands
        .get(popup.selected)
        .and_then(|(_, command)| command.as_deref())
        .unwrap_or("This tool cannot express the selected operation.");
    frame.render_widget(
        Paragraph::new(command)
            .wrap(Wrap { trim: false })
            .scroll((popup.scroll.min(u16::MAX as usize) as u16, 0))
            .block(Block::bordered().title("Preview")),
        preview,
    );
    frame.render_widget(
        Paragraph::new(
            popup
                .status
                .as_deref()
                .unwrap_or("Preview wraps; copied text preserves the command."),
        ),
        status,
    );
    frame.render_widget(
        Paragraph::new("Left/Right tool · Enter copy · PgUp/PgDn scroll"),
        hint,
    );
}

#[allow(clippy::too_many_arguments)]
fn list(
    frame: &mut Frame,
    area: Rect,
    rows: Vec<(usize, String)>,
    selected_index: usize,
    focused: bool,
    offset: &mut usize,
    targets: &mut Vec<(Rect, ClickTarget)>,
    target: impl Fn(usize) -> ClickTarget,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No matches").style(Style::default().fg(MUTED)),
            area,
        );
        *offset = 0;
        return;
    }
    let position = rows.iter().position(|(index, _)| *index == selected_index);
    let items: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(position_index, (_, text))| {
            ListItem::new(format!(
                "{} {}",
                if position == Some(position_index) {
                    ">"
                } else {
                    " "
                },
                truncate(text, area.width.saturating_sub(2) as usize)
            ))
        })
        .collect();
    let mut list_state =
        ListState::default().with_offset((*offset).min(rows.len().saturating_sub(1)));
    list_state.select(position);
    frame.render_stateful_widget(
        List::new(items).highlight_style(if focused {
            selected()
        } else {
            Style::default().fg(ACCENT)
        }),
        area,
        &mut list_state,
    );
    *offset = list_state.offset();
    for (visible, (index, _)) in rows
        .iter()
        .skip(*offset)
        .take(area.height as usize)
        .enumerate()
    {
        targets.push((
            Rect::new(area.x, area.y + visible as u16, area.width, 1),
            target(*index),
        ));
    }
}

fn input_widget(
    frame: &mut Frame,
    area: Rect,
    input: &tui_input::Input,
    focused: bool,
    editing: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let offset = input.visual_scroll(area.width.saturating_sub(1) as usize);
    frame.render_widget(
        Paragraph::new(input.value())
            .style(if focused {
                selected()
            } else {
                Style::default().bg(Color::DarkGray)
            })
            .scroll((0, offset.min(u16::MAX as usize) as u16)),
        area,
    );
    if editing {
        let cursor = input
            .visual_cursor()
            .saturating_sub(offset)
            .min(area.width.saturating_sub(1) as usize);
        frame.set_cursor_position((area.x + cursor as u16, area.y));
    }
}

fn result_text(
    frame: &mut Frame,
    area: Rect,
    text: &str,
    scroll: usize,
    horizontal: usize,
    wrap: bool,
) -> usize {
    let scroll = scroll.min(text.lines().count().saturating_sub(area.height as usize));
    let paragraph = Paragraph::new(text).scroll((
        scroll.min(u16::MAX as usize) as u16,
        horizontal.min(u16::MAX as usize) as u16,
    ));
    frame.render_widget(
        if wrap {
            paragraph.wrap(Wrap { trim: false })
        } else {
            paragraph
        },
        area,
    );
    scroll
}

fn breadcrumb(context: &Context, width: usize) -> String {
    if context.object.is_empty() {
        return truncate(&context.service, width);
    }
    if context.interface.is_empty() {
        return truncate(&format!("{} › {}", context.service, context.object), width);
    }
    let full = format!(
        "{} › {} › {}",
        context.service, context.object, context.interface
    );
    if Line::raw(full.as_str()).width() <= width {
        return full;
    }
    let service = truncate(&context.service, width / 3);
    let interface = truncate(&context.interface, width / 4);
    let path_width = width.saturating_sub(
        Line::raw(service.as_str()).width() + Line::raw(interface.as_str()).width() + 6,
    );
    format!(
        "{service} › {} › {interface}",
        tail(&context.object, path_width)
    )
}

fn line(area: Rect, row: u16) -> Rect {
    Rect::new(
        area.x,
        area.y + row.min(area.height),
        area.width,
        u16::from(row < area.height),
    )
}

fn below(area: Rect, rows: u16) -> Rect {
    Rect::new(
        area.x,
        area.y + rows.min(area.height),
        area.width,
        area.height.saturating_sub(rows),
    )
}

/// Truncate by terminal-cell width, including CJK values and combining marks.
fn truncate(text: &str, width: usize) -> String {
    if Line::raw(text).width() <= width {
        return text.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 0;
    let mut out = String::new();
    for ch in text.chars() {
        let size = Span::raw(ch.to_string()).width();
        if used + size > width - 1 {
            break;
        }
        out.push(ch);
        used += size;
    }
    out.push('…');
    out
}

fn pad(text: &str, width: usize) -> String {
    let text = truncate(text, width);
    let padding = width.saturating_sub(Line::raw(text.as_str()).width());
    format!("{text}{}", " ".repeat(padding))
}

fn tail(text: &str, width: usize) -> String {
    if Line::raw(text).width() <= width {
        return text.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 1;
    let mut out = vec![];
    for ch in text.chars().rev() {
        let size = Span::raw(ch.to_string()).width();
        if used + size > width {
            break;
        }
        out.push(ch);
        used += size;
    }
    format!("…{}", out.into_iter().rev().collect::<String>())
}
