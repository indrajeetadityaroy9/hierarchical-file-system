use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use txm::ratatui::Math;
use unicode_width::UnicodeWidthStr;

use crate::App;

pub(crate) fn render(frame: &mut Frame, app: &App) {
    let [header_area, input_area, preview_area, help_area] =
        frame.area().layout(&Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(1),
        ]));

    render_header(frame, header_area);
    render_input(frame, app, input_area);
    render_preview(frame, app, preview_area);
    render_help(frame, help_area);
}

fn render_header(frame: &mut Frame, area: Rect) {
    let title = Line::from(vec![
        Span::styled(
            "mathnote",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  live terminal mathematics notebook"),
    ]);
    frame.render_widget(Paragraph::new(title), area);
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" LaTeX input ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let cursor_width = UnicodeWidthStr::width(&app.input()[..app.cursor()]) as u16;
    let scroll = cursor_width.saturating_sub(inner.width.saturating_sub(1));
    frame.render_widget(Paragraph::new(app.input()).scroll((0, scroll)), inner);
    frame.set_cursor_position(Position::new(
        inner.x + cursor_width.saturating_sub(scroll),
        inner.y,
    ));
}

fn render_preview(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Terminal math preview ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    match Math::new(app.input()) {
        Ok(math) => (&math).render(inner, frame.buffer_mut()),
        Err(error) => frame.render_widget(
            Paragraph::new(format!("Cannot render: {error}"))
                .style(Style::default().fg(Color::Red)),
            inner,
        ),
    }
}

fn render_help(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new("Type LaTeX to update the preview  •  Esc or Ctrl-C to quit")
            .style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::render;
    use crate::App;

    #[test]
    fn renders_application_and_math_preview() {
        let mut terminal = Terminal::new(TestBackend::new(80, 18)).expect("test terminal");
        let app = App::default();

        terminal.draw(|frame| render(frame, &app)).expect("draw");

        let output: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(output.contains("mathnote"));
        assert!(output.contains("LaTeX input"));
        assert!(output.contains("Terminal math preview"));
        assert!(output.contains('±'));
    }
}
