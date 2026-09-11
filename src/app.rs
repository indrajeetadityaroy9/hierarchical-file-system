use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::DefaultTerminal;

use crate::ui;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub struct App {
    input: String,
    cursor: usize,
    should_quit: bool,
}

impl Default for App {
    fn default() -> Self {
        let input = String::from(r"\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}");
        let cursor = input.len();
        Self {
            input,
            cursor,
            should_quit: false,
        }
    }
}

impl App {
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| ui::render(frame, &self))?;

            if event::poll(EVENT_POLL_INTERVAL)?
                && let Event::Key(key) = event::read()?
            {
                self.handle_key(key);
            }
        }
        Ok(())
    }

    pub(crate) fn input(&self) -> &str {
        &self.input
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }

        match key.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.input.insert(self.cursor, character);
                self.cursor += character.len_utf8();
            }
            KeyCode::Backspace => self.remove_previous_character(),
            KeyCode::Delete => self.remove_next_character(),
            KeyCode::Left => self.cursor = self.previous_boundary(),
            KeyCode::Right => self.cursor = self.next_boundary(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.len(),
            _ => {}
        }
    }

    fn remove_previous_character(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let previous = self.previous_boundary();
        self.input.drain(previous..self.cursor);
        self.cursor = previous;
    }

    fn remove_next_character(&mut self) {
        if self.cursor == self.input.len() {
            return;
        }
        let next = self.next_boundary();
        self.input.drain(self.cursor..next);
    }

    fn previous_boundary(&self) -> usize {
        self.input[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next_boundary(&self) -> usize {
        self.input[self.cursor..]
            .char_indices()
            .nth(1)
            .map_or(self.input.len(), |(index, _)| self.cursor + index)
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::App;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn edits_at_unicode_character_boundaries() {
        let mut app = App {
            input: String::from("xα"),
            cursor: "xα".len(),
            should_quit: false,
        };

        app.handle_key(key(KeyCode::Left));
        app.handle_key(key(KeyCode::Backspace));
        app.handle_key(key(KeyCode::Char('β')));

        assert_eq!(app.input, "βα");
        assert_eq!(app.cursor, 'β'.len_utf8());
    }

    #[test]
    fn escape_requests_shutdown() {
        let mut app = App::default();
        app.handle_key(key(KeyCode::Esc));
        assert!(app.should_quit);
    }
}
