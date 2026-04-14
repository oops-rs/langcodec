use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use super::app::{App, FilterMode, InputMode, StatusTone};

/// How many rows the selected language is allowed to wrap into.
const SELECTED_MAX_ROWS: u16 = 5;

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Outer layout: content rows + status bar (3 lines)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    // Content: left key panel (split_ratio%) + right translations panel
    let content = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(app.split_ratio as u16),
            Constraint::Percentage(100 - app.split_ratio as u16),
        ])
        .split(outer[0]);

    render_key_list(frame, app, content[0]);
    render_translations(frame, app, content[1]);
    render_status_bar(frame, app, outer[1]);

    // Help overlay drawn last so it appears on top of everything
    if app.show_help || app.input_mode == InputMode::Help {
        render_help_overlay(frame, area);
    }
}

// ── Key list ──────────────────────────────────────────────────────────────────

fn render_key_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let total = app.all_keys.len();
    let filtered = app.filtered_keys.len();
    let selected_pos = app.key_list_state.selected().map(|i| i + 1).unwrap_or(0);

    let in_search = matches!(app.input_mode, InputMode::Search);

    // Border is Cyan when this panel is "active" (i.e. in search mode)
    let border_color = if in_search {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    // Filter badge
    let filter_badge = if app.filter_mode == FilterMode::Missing {
        " [missing]"
    } else {
        ""
    };

    // Title: show position/total; append search query when active
    let title = if !app.search_query.is_empty() || in_search {
        let cursor = if in_search { "█" } else { "" };
        format!(
            " Keys ({filtered}/{total}){filter_badge} /{}{cursor}/ ",
            app.search_query
        )
    } else {
        format!(" Keys ({selected_pos}/{total}){filter_badge} ")
    };

    // Build list items with missing-translation indicators
    let items: Vec<ListItem> = app
        .filtered_keys
        .iter()
        .map(|k| {
            let has_missing = app.has_missing(k);
            if has_missing {
                ListItem::new(Line::from(vec![
                    Span::styled("! ", Style::default().fg(Color::Yellow)),
                    Span::styled(k.as_str(), Style::default().fg(Color::Yellow)),
                ]))
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::raw(k.as_str()),
                ]))
            }
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color))
                .title(title),
        )
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, area, &mut app.key_list_state);
}

// ── Translations panel ────────────────────────────────────────────────────────

fn render_translations(frame: &mut Frame, app: &App, area: Rect) {
    // Clear the whole area first to prevent stale cells from complex-script
    // characters whose terminal display width differs from unicode-width reports.
    frame.render_widget(Clear, area);

    let in_edit = matches!(app.input_mode, InputMode::Edit);

    // Translations panel is "active" (Cyan border) during edit mode
    let border_color = if in_edit {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    // Panel title: show scroll hint when translation is scrolled
    let panel_title = if app.translation_scroll > 0 {
        " Translations  ↑↓ scroll "
    } else {
        " Translations "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(panel_title);

    // Get the inner content area (excludes the 1-cell border on each side).
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let Some(key) = app.selected_key().map(|s| s.to_string()) else {
        frame.render_widget(
            Paragraph::new(Span::styled(
                " No key selected",
                Style::default().fg(Color::DarkGray),
            )),
            inner,
        );
        return;
    };

    let selected_lang_idx = app.selected_lang_index;
    let mut y = inner.y;

    // ── Header: key name ────────────────────────────────────────────────────
    if y < inner.y + inner.height {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Key  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    key.as_str(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ])),
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            },
        );
        y += 1;
    }

    // Blank separator row
    if y < inner.y + inner.height {
        y += 1;
    }

    // ── Language rows ────────────────────────────────────────────────────────
    // Strategy:
    //   • Non-selected  → height 1, NO .wrap()  → long text is CLIPPED at the
    //     right edge. Immune to unicode-width inaccuracies and embedded newlines.
    //   • Selected       → height up to SELECTED_MAX_ROWS, WITH .wrap() and
    //     translation_scroll applied so the full translation is readable.
    for (i, lang) in app.languages.iter().enumerate() {
        if y >= inner.y + inner.height {
            break;
        }

        let is_selected = i == selected_lang_idx;
        let raw_value = app.get_translation(&key, lang);
        let is_missing = raw_value.is_none();

        if is_selected {
            let avail_rows = (inner.y + inner.height).saturating_sub(y);
            let row_height = SELECTED_MAX_ROWS.min(avail_rows);
            let row_rect = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: row_height,
            };
            y += row_height;

            if in_edit {
                // Render edit buffer with a block-cursor at cursor_pos
                let chars: Vec<char> = app.edit_buffer.chars().collect();
                let before: String = chars[..app.cursor_pos].iter().collect();
                let cursor_ch: String = chars
                    .get(app.cursor_pos)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| " ".to_string());
                // after = everything after the cursor character
                let after_start =
                    app.cursor_pos + 1_usize.min(chars.len().saturating_sub(app.cursor_pos));
                let after: String = chars[after_start..].iter().collect();

                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            format!(" E   {:<9}", lang),
                            Style::default()
                                .fg(Color::Yellow)
                                .bg(Color::DarkGray)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            before,
                            Style::default().fg(Color::Yellow).bg(Color::DarkGray),
                        ),
                        Span::styled(
                            cursor_ch,
                            Style::default()
                                .fg(Color::DarkGray)
                                .bg(Color::Yellow)
                                .add_modifier(Modifier::REVERSED),
                        ),
                        Span::styled(
                            after,
                            Style::default().fg(Color::Yellow).bg(Color::DarkGray),
                        ),
                    ]))
                    .wrap(Wrap { trim: false })
                    .scroll((app.translation_scroll, 0)),
                    row_rect,
                );
            } else {
                let value_span = if is_missing {
                    Span::styled("—", Style::default().fg(Color::DarkGray))
                } else {
                    Span::raw(raw_value.unwrap())
                };

                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            format!(" ▶   {:<9}", lang),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        value_span,
                    ]))
                    .wrap(Wrap { trim: false })
                    .scroll((app.translation_scroll, 0)),
                    row_rect,
                );
            }
        } else {
            // Exactly 1 row, no wrap → text is clipped, never bleeds
            let row_rect = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            y += 1;

            let value_span = if is_missing {
                Span::styled("—", Style::default().fg(Color::DarkGray))
            } else {
                Span::raw(raw_value.unwrap())
            };

            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!("     {:<9}", lang),
                        Style::default().fg(Color::DarkGray),
                    ),
                    value_span,
                ])),
                row_rect,
            );
        }
    }
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    match &app.input_mode {
        InputMode::Normal => {
            // Show transient status messages (save OK, undo, errors, etc.)
            if let Some((msg, tone)) = &app.status_message {
                let color = match tone {
                    StatusTone::Success => Color::Green,
                    StatusTone::Error => Color::Red,
                };
                let dirty_part = if app.dirty { " ● " } else { " " };
                let content = format!(
                    "{}{} — {}",
                    dirty_part,
                    truncate_path(&app.file_path, 40),
                    msg
                );
                frame.render_widget(
                    Paragraph::new(content)
                        .style(Style::default().fg(color))
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .border_type(BorderType::Rounded)
                                .border_style(Style::default().fg(color)),
                        ),
                    area,
                );
                return;
            }

            // Default normal-mode status bar
            let hint = "[/] Search  [Tab] Lang  [e] Edit  [s] Save  [D] Del  [F] Filter  [?] Help  [q] Quit";
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        if app.dirty { " ● " } else { " " },
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(
                        truncate_path(&app.file_path, 40),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("  {hint}"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::DarkGray)),
                ),
                area,
            );
        }

        InputMode::Search => {
            let content = format!(
                " Search: {}█   {} results   [Enter] confirm  [Esc] clear",
                app.search_query,
                app.filtered_keys.len()
            );
            frame.render_widget(
                Paragraph::new(content)
                    .style(Style::default().fg(Color::Cyan))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(Style::default().fg(Color::Cyan)),
                    ),
                area,
            );
        }

        InputMode::Edit => {
            frame.render_widget(
                Paragraph::new(
                    " [←→] cursor   [Ctrl+Y] copy source   [Enter] confirm   [Esc] cancel",
                )
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::Yellow)),
                ),
                area,
            );
        }

        InputMode::ConfirmQuit => {
            frame.render_widget(
                Paragraph::new(
                    " Unsaved changes — [y] Save & quit   [n] Quit   [Esc] Cancel",
                )
                .style(
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                )
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::BOLD),
                        ),
                ),
                area,
            );
        }

        InputMode::ConfirmDelete => {
            let key_name = app.selected_key().unwrap_or("<unknown>");
            let msg = format!(
                " Delete key '{key_name}'?   [y] Yes   [n] No   [Esc] Cancel"
            );
            frame.render_widget(
                Paragraph::new(msg)
                    .style(
                        Style::default()
                            .fg(Color::Red)
                            .add_modifier(Modifier::BOLD),
                    )
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(
                                Style::default()
                                    .fg(Color::Red)
                                    .add_modifier(Modifier::BOLD),
                            ),
                    ),
                area,
            );
        }

        InputMode::Help => {
            frame.render_widget(
                Paragraph::new(" Press any key to close help")
                    .style(Style::default().fg(Color::DarkGray))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(Style::default().fg(Color::DarkGray)),
                    ),
                area,
            );
        }
    }
}

// ── Help overlay ──────────────────────────────────────────────────────────────

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    // Center: ~50% wide, ~70% tall
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(15),
            Constraint::Percentage(70),
            Constraint::Percentage(15),
        ])
        .split(area);

    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(50),
            Constraint::Percentage(25),
        ])
        .split(vert[1]);

    let popup_area = horiz[1];

    // Clear the background before drawing the popup
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            " Help ",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let section = |label: &'static str| {
        Line::from(Span::styled(
            label,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let row = |text: &'static str| Line::from(Span::raw(text));

    let help_lines = vec![
        section(" Navigation"),
        row("  j/↓  k/↑       Move between keys"),
        row("  g / G           Jump to top / bottom"),
        row("  Ctrl+d/u        Page down / up"),
        row("  ] / [           Next / prev missing"),
        row(""),
        section(" Language"),
        row("  Tab/Shift+Tab   Cycle language"),
        row(""),
        section(" Edit"),
        row("  e / Enter       Edit translation"),
        row("  ←/→  Home/End   Move cursor"),
        row("  Ctrl+Y          Copy source language"),
        row("  Enter           Confirm  |  Esc  Cancel"),
        row(""),
        section(" Actions"),
        row("  s               Save"),
        row("  u               Undo last edit"),
        row("  D               Delete key (confirm)"),
        row("  F               Toggle filter (all/missing)"),
        row("  < / >           Resize panels"),
        row("  /               Search"),
        row("  ?               Toggle this help"),
        row("  q               Quit"),
        row(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    frame.render_widget(
        Paragraph::new(help_lines)
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }
    let short = &path[path.len() - max_len..];
    if let Some(pos) = short.find('/') {
        format!("…{}", &short[pos..])
    } else {
        format!("…{short}")
    }
}
