use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::app::{App, InputMode};

pub enum HandlerResult {
    Continue,
    Quit,
}

pub fn handle_event(app: &mut App, event: Event) -> HandlerResult {
    if let Event::Key(key) = event {
        if key.kind != KeyEventKind::Press {
            return HandlerResult::Continue;
        }
        return handle_key(app, key);
    }
    HandlerResult::Continue
}

fn handle_key(app: &mut App, key: KeyEvent) -> HandlerResult {
    match app.input_mode {
        InputMode::Normal => handle_normal(app, key),
        InputMode::Search => handle_search(app, key),
        InputMode::Edit => handle_edit(app, key),
        InputMode::ConfirmQuit => handle_confirm_quit(app, key),
        InputMode::Help => handle_help(app, key),
        InputMode::ConfirmDelete => handle_confirm_delete(app, key),
    }
}

fn handle_normal(app: &mut App, key: KeyEvent) -> HandlerResult {
    match key.code {
        // Translation panel scroll — Alt+Down / Alt+Up or Shift+J / Shift+K
        // These must come before the plain Down/Up/j/k arms to avoid being swallowed.
        KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
            app.translation_scroll_down();
        }
        KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
            app.translation_scroll_up();
        }
        KeyCode::Char('J') => {
            app.translation_scroll_down();
        }
        KeyCode::Char('K') => {
            app.translation_scroll_up();
        }

        // Page navigation — must come before plain j/k to avoid swallowing Ctrl+d/u
        KeyCode::PageDown | KeyCode::Char('d')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.key_next_page(10);
        }
        KeyCode::PageUp | KeyCode::Char('u')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.key_prev_page(10);
        }

        // Key list navigation
        KeyCode::Down | KeyCode::Char('j') => {
            app.key_next();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.key_prev();
        }
        KeyCode::Char('g') => {
            app.key_jump_top();
        }
        KeyCode::Char('G') => {
            app.key_jump_bottom();
        }

        // Missing-translation navigation
        KeyCode::Char(']') => {
            app.next_missing();
        }
        KeyCode::Char('[') => {
            app.prev_missing();
        }

        // Language selection in translations panel
        KeyCode::Tab => {
            app.lang_next();
        }
        KeyCode::BackTab => {
            app.lang_prev();
        }

        // Panel resize
        KeyCode::Char('<') => {
            app.split_narrower();
        }
        KeyCode::Char('>') => {
            app.split_wider();
        }

        // Enter search mode
        KeyCode::Char('/') => {
            app.input_mode = InputMode::Search;
            app.status_message = None;
        }

        // Enter edit mode for selected lang
        KeyCode::Char('e') | KeyCode::Enter => {
            app.enter_edit_mode();
        }

        // Undo
        KeyCode::Char('u') => {
            app.undo();
        }

        // Cycle filter mode
        KeyCode::Char('F') => {
            app.cycle_filter_mode();
        }

        // Delete selected key (with confirmation)
        KeyCode::Char('D') => {
            app.confirm_delete = true;
            app.input_mode = InputMode::ConfirmDelete;
        }

        // Help overlay
        KeyCode::Char('?') => {
            app.show_help = true;
            app.input_mode = InputMode::Help;
        }

        // Save
        KeyCode::Char('s') => {
            if let Err(e) = app.save() {
                app.status_message = Some((e, super::app::StatusTone::Error));
            }
        }

        // Quit
        KeyCode::Char('q') => {
            if app.dirty {
                app.input_mode = InputMode::ConfirmQuit;
            } else {
                return HandlerResult::Quit;
            }
        }

        // Escape clears status message
        KeyCode::Esc => {
            app.status_message = None;
        }

        _ => {}
    }
    HandlerResult::Continue
}

fn handle_search(app: &mut App, key: KeyEvent) -> HandlerResult {
    match key.code {
        KeyCode::Esc => {
            // Clear search and return to normal
            app.search_query.clear();
            app.apply_filter();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Enter => {
            // Confirm search, stay on filtered results
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            app.apply_filter();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.key_next();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.key_prev();
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.apply_filter();
        }
        _ => {}
    }
    HandlerResult::Continue
}

fn handle_edit(app: &mut App, key: KeyEvent) -> HandlerResult {
    match key.code {
        KeyCode::Left => {
            app.cursor_move_left();
        }
        KeyCode::Right => {
            app.cursor_move_right();
        }
        KeyCode::Home => {
            app.cursor_home();
        }
        KeyCode::End => {
            app.cursor_end();
        }
        KeyCode::Backspace => {
            app.edit_backspace();
        }
        KeyCode::Delete => {
            app.edit_delete_forward();
        }
        KeyCode::Enter => {
            app.commit_edit();
        }
        KeyCode::Esc => {
            app.cancel_edit();
        }
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            match c {
                'a' | 'A' => app.cursor_home(),
                'e' | 'E' => app.cursor_end(),
                'y' | 'Y' => app.copy_from_source_lang(),
                'c' | 'C' | 'd' | 'D' => app.cancel_edit(),
                _ => {}
            }
        }
        KeyCode::Char(c) => {
            app.edit_insert(c);
        }
        _ => {}
    }
    HandlerResult::Continue
}

fn handle_help(app: &mut App, _key: KeyEvent) -> HandlerResult {
    app.show_help = false;
    app.input_mode = InputMode::Normal;
    HandlerResult::Continue
}

fn handle_confirm_delete(app: &mut App, key: KeyEvent) -> HandlerResult {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            app.confirm_delete_key();
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.confirm_delete = false;
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    HandlerResult::Continue
}

fn handle_confirm_quit(app: &mut App, key: KeyEvent) -> HandlerResult {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Err(e) = app.save() {
                app.status_message = Some((e, super::app::StatusTone::Error));
                app.input_mode = InputMode::Normal;
            } else {
                return HandlerResult::Quit;
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            return HandlerResult::Quit;
        }
        KeyCode::Esc | KeyCode::Char('c') | KeyCode::Char('C') => {
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
    HandlerResult::Continue
}
