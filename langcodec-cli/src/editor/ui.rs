use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use super::app::{App, InputMode, StatusTone};

/// How many rows the selected language is allowed to wrap into.
const SELECTED_MAX_ROWS: u16 = 5;

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Outer layout: content rows + status bar (3 lines)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    // Content: left key panel (38%) + right translations panel (62%)
    let content = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(outer[0]);

    render_key_list(frame, app, content[0]);
    render_translations(frame, app, content[1]);
    render_status_bar(frame, app, outer[1]);
}

fn render_key_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let total = app.all_keys.len();
    let filtered = app.filtered_keys.len();

    let title = if app.search_query.is_empty() {
        format!(" Keys ({total}) ")
    } else {
        format!(" Keys ({filtered}/{total}) ")
    };

    let items: Vec<ListItem> = app
        .filtered_keys
        .iter()
        .map(|k| ListItem::new(k.as_str()))
        .collect();

    let border_style = if matches!(app.input_mode, InputMode::Search) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let block_title =
        if !app.search_query.is_empty() || matches!(app.input_mode, InputMode::Search) {
            let cursor = if matches!(app.input_mode, InputMode::Search) {
                "█"
            } else {
                ""
            };
            format!("{} /{}{}/", title.trim_end(), app.search_query, cursor)
        } else {
            title
        };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(block_title),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.key_list_state);
}

fn render_translations(frame: &mut Frame, app: &App, area: Rect) {
    // Clear the whole area to prevent stale cells from complex-script characters
    // whose terminal display width differs from what unicode-width reports.
    frame.render_widget(Clear, area);

    let in_edit = matches!(app.input_mode, InputMode::Edit);
    let border_style = if in_edit {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Translations ");

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
                Span::styled(key.as_str(), Style::default().add_modifier(Modifier::BOLD)),
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
    //     right edge. This is immune to unicode-width inaccuracies and embedded
    //     newlines because the Rect physically limits the row to one line.
    //   • Selected       → height up to SELECTED_MAX_ROWS, WITH .wrap() so the
    //     full translation is readable.
    for (i, lang) in app.languages.iter().enumerate() {
        if y >= inner.y + inner.height {
            break;
        }

        let is_selected = i == selected_lang_idx;
        let value = app
            .get_translation(&key, lang)
            .unwrap_or_else(|| "—".to_string());

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
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            format!(" E   {:<9}", lang),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            app.edit_buffer.as_str(),
                            Style::default().bg(Color::DarkGray).fg(Color::White),
                        ),
                        Span::styled("█", Style::default().fg(Color::Yellow)),
                    ]))
                    .wrap(Wrap { trim: false }),
                    row_rect,
                );
            } else {
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            format!(" >   {:<9}", lang),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(value),
                    ]))
                    .wrap(Wrap { trim: false }),
                    row_rect,
                );
            }
        } else {
            // Exactly 1 row, no wrap → text is clipped, never bleeds into the
            // next language row regardless of script or embedded newlines.
            let row_rect = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            y += 1;

            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!("     {:<9}", lang),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(value),
                ])),
                row_rect,
            );
        }
    }
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let dirty_marker = if app.dirty { " ●" } else { "" };

    let (content, style) = match &app.input_mode {
        InputMode::Normal => {
            let hint =
                " [/] Search  [Tab] Lang↓  [Shift+Tab] Lang↑  [e] Edit  [s] Save  [q] Quit  [g/G] Top/Bot";
            if let Some((msg, tone)) = &app.status_message {
                let color = match tone {
                    StatusTone::Success => Color::Green,
                    StatusTone::Error => Color::Red,
                };
                let content =
                    format!("{}{} — {}", dirty_marker, truncate_path(&app.file_path, 40), msg);
                frame.render_widget(
                    Paragraph::new(content)
                        .style(Style::default().fg(color))
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .border_style(Style::default().fg(color)),
                        ),
                    area,
                );
                return;
            }
            (
                format!("{}{}{}", dirty_marker, truncate_path(&app.file_path, 40), hint),
                Style::default().fg(Color::DarkGray),
            )
        }

        InputMode::Search => (
            format!(
                " [↑/↓] Navigate  [Enter] Confirm  [Esc] Clear search  — {} results",
                app.filtered_keys.len()
            ),
            Style::default().fg(Color::Yellow),
        ),

        InputMode::Edit => (
            " [Enter] Confirm  [Esc] Cancel  [Backspace] Delete char  (type to edit)".to_string(),
            Style::default().fg(Color::Green),
        ),

        InputMode::ConfirmQuit => (
            " Unsaved changes! [y] Save & Quit  [n] Quit without saving  [Esc/c] Cancel"
                .to_string(),
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
    };

    frame.render_widget(
        Paragraph::new(content)
            .style(style)
            .block(Block::default().borders(Borders::ALL)),
        area,
    );
}

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
