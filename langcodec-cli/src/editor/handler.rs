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
    }
}

fn handle_normal(app: &mut App, key: KeyEvent) -> HandlerResult {
    match key.code {
        // Key list navigation
        KeyCode::Down | KeyCode::Char('j') => {
            app.key_next();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.key_prev();
        }
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
        KeyCode::Char('g') => {
            app.key_jump_top();
        }
        KeyCode::Char('G') => {
            app.key_jump_bottom();
        }

        // Language selection in translations panel
        KeyCode::Tab => {
            app.lang_next();
        }
        KeyCode::BackTab => {
            app.lang_prev();
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
        KeyCode::Enter => {
            app.commit_edit();
        }
        KeyCode::Esc => {
            app.cancel_edit();
        }
        KeyCode::Backspace => {
            app.edit_buffer.pop();
        }
        KeyCode::Char(c) => {
            // Honour Ctrl+C/Ctrl+D as cancel
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                app.cancel_edit();
            } else {
                app.edit_buffer.push(c);
            }
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
