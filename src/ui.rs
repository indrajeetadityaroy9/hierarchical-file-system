use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, StatefulWidget, Wrap};
use ratatui_image::StatefulImage;

use crate::App;
use crate::app::PaneFocus;
use crate::layout;
use crate::theme::Theme;

pub(crate) fn render(frame: &mut Frame, app: &mut App) {
    let theme = Theme::default();
    frame.render_widget(
        Block::new().style(Style::default().bg(theme.background)),
        frame.area(),
    );

    let [content_area, status_area] = frame.area().layout(&Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
    ]));
    let panes = layout::split(content_area, app.focus());

    let source_inner = render_source(frame, app, panes.source, &theme);
    let latex_inner = render_generated_latex(frame, app, panes.latex, &theme);
    let preview_inner = render_preview(frame, app, panes.preview, &theme);
    app.configure_layout(
        source_inner.into(),
        latex_inner.into(),
        preview_inner.into(),
    );
    render_status(frame, app, status_area, &theme);

    if app.show_help() {
        render_help(frame, &theme);
    }
}

fn render_source(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Natural note ")
        .border_style(pane_border(theme, app.focus() == PaneFocus::Source));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return inner;
    }

    let diagnostic_line = app.diagnostic_line();
    let (scroll_y, scroll_x) = app.source_scroll();
    let first_line = usize::from(scroll_y);
    let end_line = first_line
        .saturating_add(usize::from(inner.height))
        .min(app.source_line_count());
    let lines: Vec<Line<'static>> = (first_line..end_line)
        .map(|line_index| {
            let style = if diagnostic_line == Some(line_index) {
                Style::default().fg(Color::White).bg(theme.error)
            } else {
                Style::default().fg(theme.foreground)
            };
            Line::styled(app.source_line(line_index), style)
        })
        .collect();
    frame.render_widget(
        Paragraph::new(Text::from(lines)).scroll((0, scroll_x)),
        inner,
    );

    if app.focus() == PaneFocus::Source {
        let (cursor_x, cursor_y) = app.cursor_screen_position();
        if cursor_x < inner.width && cursor_y < inner.height {
            frame.set_cursor_position(Position::new(inner.x + cursor_x, inner.y + cursor_y));
        }
    }
    inner
}

fn render_generated_latex(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Generated LaTeX body ")
        .border_style(pane_border(theme, app.focus() == PaneFocus::Latex));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return inner;
    }

    let body = app.generated_body();
    let paragraph = if body.trim().is_empty() {
        Paragraph::new(Line::styled(
            "The generated document body will appear here.",
            Style::default().fg(theme.muted),
        ))
    } else {
        Paragraph::new(body)
    };
    frame.render_widget(
        paragraph
            .style(Style::default().fg(theme.foreground))
            .wrap(Wrap { trim: false })
            .scroll((app.latex_scroll(), 0)),
        inner,
    );
    inner
}

fn render_preview(frame: &mut Frame, app: &mut App, area: Rect, theme: &Theme) -> Rect {
    let title = format!(
        " LaTeX document · {} · {} ",
        app.page_label(),
        app.protocol_label()
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(pane_border(theme, app.focus() == PaneFocus::Preview));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return inner;
    }

    frame.render_widget(Clear, inner);
    frame.render_widget(Block::new().style(Style::default().bg(Color::White)), inner);
    if app.has_preview() {
        StatefulImage::new().render(inner, frame.buffer_mut(), app.image_state_mut());
    } else {
        frame.render_widget(
            Paragraph::new(app.preview_placeholder())
                .style(Style::default().fg(theme.muted).bg(Color::White))
                .alignment(Alignment::Center),
            inner,
        );
    }
    inner
}

fn render_status(frame: &mut Frame, app: &App, area: Rect, theme: &Theme) {
    frame.render_widget(
        Block::new().style(Style::default().bg(theme.status_background)),
        area,
    );

    let hints = if area.width >= 72 {
        "F1 help  F6 panes  Esc quit"
    } else if area.width >= 50 {
        "F1 help  F6"
    } else {
        ""
    };
    let hint_width = u16::try_from(hints.len())
        .unwrap_or(u16::MAX)
        .min(area.width);
    let [left, right] = area.layout(&Layout::horizontal([
        Constraint::Min(1),
        Constraint::Length(hint_width),
    ]));

    let status = app.status_line();
    let status_color = if status.starts_with("error:") {
        theme.error
    } else if status.starts_with("ready") {
        theme.success
    } else if status == "type a note to begin" {
        theme.muted
    } else {
        theme.warning
    };
    let line = Line::from(vec![
        Span::styled(
            " EDIT ",
            Style::default()
                .fg(Color::Black)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            app.focus_label(),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(status, Style::default().fg(status_color)),
    ]);
    frame.render_widget(Paragraph::new(line), left);
    if !hints.is_empty() {
        frame.render_widget(
            Paragraph::new(hints)
                .alignment(Alignment::Right)
                .style(Style::default().fg(theme.muted)),
            right,
        );
    }
}

fn render_help(frame: &mut Frame, theme: &Theme) {
    let area = centered_rect(frame.area(), 72, 15);
    frame.render_widget(Clear, area);
    let help = Text::from(vec![
        Line::from("Editing"),
        Line::from("  Type normally · Tab inserts four spaces · Ctrl-U clears"),
        Line::from("  Arrow keys, Home, End, Backspace, Delete"),
        Line::from(""),
        Line::from("Panes"),
        Line::from("  F6 / Shift-F6 cycles Source, LaTeX, and Preview"),
        Line::from("  In inspector panes: h/l changes focus, j/k scrolls"),
        Line::from("  PageUp/PageDown changes PDF pages in Preview"),
        Line::from(""),
        Line::from("Global"),
        Line::from("  F1 closes help · Esc or Ctrl-C quits"),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" mathnote help ")
        .border_style(Style::default().fg(theme.border));
    frame.render_widget(
        Paragraph::new(help)
            .block(block)
            .style(Style::default().fg(theme.foreground))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn centered_rect(area: Rect, preferred_width: u16, preferred_height: u16) -> Rect {
    let width = preferred_width.min(area.width.saturating_sub(2)).max(1);
    let height = preferred_height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn pane_border(theme: &Theme, focused: bool) -> Style {
    Style::default().fg(if focused {
        theme.accent
    } else {
        theme.inactive
    })
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn render_screen(width: u16, height: u16, show_help: bool) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::default();
        if show_help {
            app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        }
        terminal
            .draw(|frame| render(frame, &mut app))
            .expect("render succeeds");

        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .filter_map(|x| buffer.cell((x, y)))
                    .map(|cell| cell.symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn status_bar_replaces_the_static_header() {
        let screen = render_screen(100, 24, false);
        assert!(screen.contains("Natural note"));
        assert!(screen.contains("Generated LaTeX body"));
        assert!(screen.contains("LaTeX document"));
        assert!(screen.contains("EDIT"));
        assert!(screen.contains("SOURCE"));
        assert!(screen.contains("F1 help"));
        assert!(!screen.contains("natural-language mathematics"));
    }

    #[test]
    fn help_overlay_is_visible_in_the_rendered_buffer() {
        let screen = render_screen(100, 24, true);
        assert!(screen.contains("mathnote help"));
        assert!(screen.contains("Shift-F6 cycles"));
        assert!(screen.contains("PageUp/PageDown"));
    }

    #[test]
    fn narrow_layout_keeps_all_panes_available() {
        let screen = render_screen(50, 18, false);
        assert!(screen.contains("Natural note"));
        assert!(screen.contains("Generated LaTeX body"));
        assert!(screen.contains("LaTeX document"));
    }
}
