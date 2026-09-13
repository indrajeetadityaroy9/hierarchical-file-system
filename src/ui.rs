use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, StatefulWidget, Wrap};
use ratatui_image::StatefulImage;

use crate::App;
use crate::app::PaneFocus;
use crate::layout;

pub(crate) fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let [header_area, content_area, status_area] = area.layout(&Layout::vertical([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
    ]));
    let panes = layout::split(content_area, app.focus());

    render_header(frame, header_area);
    let source_inner = render_source(frame, app, panes.source);
    let latex_inner = render_generated_latex(frame, app, panes.latex);
    let preview_inner = render_preview(frame, app, panes.preview);
    app.configure_layout(
        source_inner.into(),
        latex_inner.into(),
        preview_inner.into(),
    );
    render_status(frame, app, status_area);
}

fn render_header(frame: &mut Frame, area: Rect) {
    let title = Line::from(vec![
        Span::styled(
            "mathnote",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  natural-language mathematics → live LaTeX document"),
    ]);
    let help = Line::from(
        "F6 panes  Ctrl-U clear  inspector: h/l focus · j/k scroll  PgUp/PgDn pages  Esc quit",
    )
    .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(Paragraph::new(vec![title, help]), area);
}

fn render_source(frame: &mut Frame, app: &App, area: Rect) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Natural note ")
        .border_style(pane_border(app.focus() == PaneFocus::Source));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return inner;
    }

    let diagnostic_line = app.diagnostic_line();
    let lines: Vec<Line<'static>> = (0..app.source_line_count())
        .map(|line_index| {
            let style = if diagnostic_line == Some(line_index) {
                Style::default().fg(Color::White).bg(Color::Red)
            } else {
                Style::default().fg(Color::White)
            };
            Line::styled(app.source_line(line_index), style)
        })
        .collect();
    let (scroll_y, scroll_x) = app.source_scroll();
    frame.render_widget(
        Paragraph::new(Text::from(lines)).scroll((scroll_y, scroll_x)),
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

fn render_generated_latex(frame: &mut Frame, app: &App, area: Rect) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Generated LaTeX body ")
        .border_style(pane_border(app.focus() == PaneFocus::Latex));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return inner;
    }

    let body = app.generated_body();
    let content = if body.trim().is_empty() {
        Text::from(Line::styled(
            "The generated document body will appear here.",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        Text::from(body.to_owned())
    };
    frame.render_widget(
        Paragraph::new(content)
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: false })
            .scroll((app.latex_scroll(), 0)),
        inner,
    );
    inner
}

fn render_preview(frame: &mut Frame, app: &mut App, area: Rect) -> Rect {
    let title = format!(
        " LaTeX document · {} · {} ",
        app.page_label(),
        app.protocol_label()
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(pane_border(app.focus() == PaneFocus::Preview));
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
            Paragraph::new("Compiling the document preview…")
                .style(Style::default().fg(Color::DarkGray).bg(Color::White))
                .alignment(Alignment::Center),
            inner,
        );
    }
    inner
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let status = app.status_line();
    let style = if status.starts_with("error:") {
        Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(Paragraph::new(status).style(style), area);
}

fn pane_border(focused: bool) -> Style {
    Style::default().fg(if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    })
}
