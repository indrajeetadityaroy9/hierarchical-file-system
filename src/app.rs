use std::io;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use image::{DynamicImage, ImageBuffer, Rgba, imageops};
use ratatui::DefaultTerminal;
use ratatui::layout::Size;
use ratatui_image::errors::Errors as ImageError;
use ratatui_image::picker::Picker;
use ratatui_image::thread::{ResizeRequest, ResizeResponse, ThreadProtocol};

use crate::compiler::{
    CompileError, CompileRequest, DiagnosticKind, cache_is_warmed, compile_latex,
};
use crate::document::{Document, SourceSpan, TextBuffer};
use crate::latex::{LatexDocument, emit_latex};
use crate::preview::{inspect_pdf, rasterize_page};
use crate::ui;

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const COMPILE_DEBOUNCE: Duration = Duration::from_millis(120);
const DEFAULT_RASTER_WIDTH: u32 = 800;

pub struct App {
    buffer: TextBuffer,
    should_quit: bool,
    focus: PaneFocus,
    revision: u64,
    generated_revision: Option<u64>,
    generated: Option<LatexDocument>,
    last_edit: Instant,
    submitted_revision: Option<u64>,
    worker_tx: mpsc::Sender<WorkerRequest>,
    worker_rx: mpsc::Receiver<WorkerEvent>,
    status: PipelineStatus,
    diagnostic_span: Option<SourceSpan>,

    picker: Picker,
    image_state: ThreadProtocol,
    resize_rx: mpsc::Receiver<Result<ResizeResponse, ImageError>>,
    pdf: Option<Arc<Vec<u8>>>,
    pdf_revision: Option<u64>,
    page_count: usize,
    page_index: usize,
    full_page: Option<DynamicImage>,
    full_page_width: u32,
    requested_raster: Option<(u64, usize, u32)>,

    source_size: Size,
    source_scroll_y: usize,
    source_scroll_x: u16,
    latex_size: Size,
    latex_scroll_rows: u16,
    preview_size: Size,
    preview_scroll_rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneFocus {
    Source,
    Latex,
    Preview,
}

impl PaneFocus {
    fn next(self) -> Self {
        match self {
            Self::Source => Self::Latex,
            Self::Latex => Self::Preview,
            Self::Preview => Self::Source,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Source => Self::Preview,
            Self::Latex => Self::Source,
            Self::Preview => Self::Latex,
        }
    }
}

#[derive(Debug, Clone)]
enum PipelineStatus {
    Empty,
    Waiting,
    Compiling { bootstrap: bool },
    Rasterizing,
    Ready { elapsed: Duration, warnings: usize },
    Error(String),
}

enum WorkerRequest {
    Compile {
        revision: u64,
        document: LatexDocument,
        page_index: usize,
        target_width: u32,
    },
    Raster {
        revision: u64,
        pdf: Arc<Vec<u8>>,
        page_index: usize,
        target_width: u32,
    },
}

enum WorkerEvent {
    Ready {
        revision: u64,
        pdf: Arc<Vec<u8>>,
        page_count: usize,
        page_index: usize,
        image: DynamicImage,
        width: u32,
        elapsed: Duration,
        warnings: usize,
    },
    RasterReady {
        revision: u64,
        page_index: usize,
        image: DynamicImage,
        width: u32,
    },
    Failed {
        revision: u64,
        message: String,
        tex_line: Option<usize>,
    },
}

impl Default for App {
    fn default() -> Self {
        Self::new(Picker::halfblocks())
    }
}

impl App {
    pub fn new(mut picker: Picker) -> Self {
        picker.set_background_color(Some(Rgba([255, 255, 255, 255])));

        let (worker_tx, worker_requests) = mpsc::channel();
        let (worker_events, worker_rx) = mpsc::channel();
        thread::spawn(move || pipeline_worker(worker_requests, worker_events));

        let (resize_tx, resize_requests) = mpsc::channel::<ResizeRequest>();
        let (resize_events, resize_rx) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(request) = resize_requests.recv() {
                if resize_events.send(request.resize_encode()).is_err() {
                    break;
                }
            }
        });

        let buffer = TextBuffer::default();
        let mut app = Self {
            buffer,
            should_quit: false,
            focus: PaneFocus::Source,
            revision: 1,
            generated_revision: None,
            generated: None,
            last_edit: Instant::now() - COMPILE_DEBOUNCE,
            submitted_revision: None,
            worker_tx,
            worker_rx,
            status: PipelineStatus::Waiting,
            diagnostic_span: None,
            picker,
            image_state: ThreadProtocol::new(resize_tx, None),
            resize_rx,
            pdf: None,
            pdf_revision: None,
            page_count: 0,
            page_index: 0,
            full_page: None,
            full_page_width: 0,
            requested_raster: None,
            source_size: Size::default(),
            source_scroll_y: 0,
            source_scroll_x: 0,
            latex_size: Size::default(),
            latex_scroll_rows: 0,
            preview_size: Size::new(80, 24),
            preview_scroll_rows: 0,
        };
        app.rebuild_document();
        app
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            self.process_background_events();
            self.maybe_submit_compile();
            self.maybe_submit_raster();

            terminal.draw(|frame| ui::render(frame, &mut self))?;

            self.maybe_submit_compile();
            self.maybe_submit_raster();

            if event::poll(EVENT_POLL_INTERVAL)? {
                match event::read()? {
                    Event::Key(key) => self.handle_key(key),
                    Event::Paste(text) => {
                        self.buffer.insert_str(&text);
                        self.mark_edited();
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }

        if key.code == KeyCode::F(6) {
            self.focus = if key.modifiers.contains(KeyModifiers::SHIFT) {
                self.focus.previous()
            } else {
                self.focus.next()
            };
            self.ensure_cursor_visible();
            return;
        }

        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }
        if key.code == KeyCode::Char('u') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.buffer.clear();
            self.mark_edited();
            return;
        }
        if key.code == KeyCode::Up && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.scroll_preview(-3);
            return;
        }
        if key.code == KeyCode::Down && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.scroll_preview(3);
            return;
        }

        if self.focus != PaneFocus::Source && self.handle_browse_key(key) {
            return;
        }

        let edited = match key.code {
            KeyCode::Esc => {
                self.should_quit = true;
                false
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.buffer.insert_char(character);
                true
            }
            KeyCode::Enter => {
                self.buffer.insert_char('\n');
                true
            }
            KeyCode::Tab => {
                self.buffer.insert_str("    ");
                true
            }
            KeyCode::Backspace => self.buffer.backspace(),
            KeyCode::Delete => self.buffer.delete(),
            KeyCode::Left => {
                self.buffer.move_left();
                false
            }
            KeyCode::Right => {
                self.buffer.move_right();
                false
            }
            KeyCode::Up => {
                self.buffer.move_up();
                false
            }
            KeyCode::Down => {
                self.buffer.move_down();
                false
            }
            KeyCode::Home => {
                self.buffer.move_home();
                false
            }
            KeyCode::End => {
                self.buffer.move_end();
                false
            }
            KeyCode::PageUp => {
                self.change_page(-1);
                false
            }
            KeyCode::PageDown => {
                self.change_page(1);
                false
            }
            _ => false,
        };

        if edited {
            self.mark_edited();
        } else {
            self.ensure_cursor_visible();
        }
    }

    fn handle_browse_key(&mut self, key: KeyEvent) -> bool {
        let plain_character = |expected| {
            key.code == KeyCode::Char(expected)
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        };

        if plain_character('q') {
            self.should_quit = true;
            return true;
        }
        if key.code == KeyCode::Left || plain_character('h') {
            self.focus = self.focus.previous();
            self.ensure_cursor_visible();
            return true;
        }
        if key.code == KeyCode::Right || key.code == KeyCode::Enter || plain_character('l') {
            self.focus = self.focus.next();
            self.ensure_cursor_visible();
            return true;
        }

        match self.focus {
            PaneFocus::Source => false,
            PaneFocus::Latex => match key.code {
                KeyCode::Up if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_latex(-1);
                    true
                }
                KeyCode::Down if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_latex(1);
                    true
                }
                KeyCode::PageUp => {
                    self.scroll_latex(
                        -i16::try_from(self.latex_size.height.max(1)).unwrap_or(i16::MAX),
                    );
                    true
                }
                KeyCode::PageDown => {
                    self.scroll_latex(
                        i16::try_from(self.latex_size.height.max(1)).unwrap_or(i16::MAX),
                    );
                    true
                }
                KeyCode::Home => {
                    self.latex_scroll_rows = 0;
                    true
                }
                KeyCode::End => {
                    self.latex_scroll_rows = self.max_latex_scroll();
                    true
                }
                KeyCode::Char('k') if plain_character('k') => {
                    self.scroll_latex(-1);
                    true
                }
                KeyCode::Char('j') if plain_character('j') => {
                    self.scroll_latex(1);
                    true
                }
                _ => false,
            },
            PaneFocus::Preview => match key.code {
                KeyCode::Up if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_preview(-1);
                    true
                }
                KeyCode::Down if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_preview(1);
                    true
                }
                KeyCode::Char('k') if plain_character('k') => {
                    self.scroll_preview(-1);
                    true
                }
                KeyCode::Char('j') if plain_character('j') => {
                    self.scroll_preview(1);
                    true
                }
                KeyCode::PageUp => {
                    self.change_page(-1);
                    true
                }
                KeyCode::PageDown => {
                    self.change_page(1);
                    true
                }
                KeyCode::Home => {
                    self.preview_scroll_rows = 0;
                    self.install_visible_preview();
                    true
                }
                KeyCode::End => {
                    self.scroll_preview(i16::MAX);
                    true
                }
                _ => false,
            },
        }
    }

    fn mark_edited(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.last_edit = Instant::now();
        self.submitted_revision = None;
        self.requested_raster = None;
        self.diagnostic_span = None;
        self.rebuild_document();
        self.ensure_cursor_visible();
    }

    fn rebuild_document(&mut self) {
        let source = self.buffer.text();
        if source.trim().is_empty() {
            self.generated = Document::parse(&source)
                .ok()
                .map(|document| emit_latex(&document));
            self.generated_revision = Some(self.revision);
            self.latex_scroll_rows = 0;
            self.status = PipelineStatus::Empty;
            self.diagnostic_span = None;
            return;
        }
        match Document::parse(&source) {
            Ok(document) => {
                self.generated = Some(emit_latex(&document));
                self.generated_revision = Some(self.revision);
                self.latex_scroll_rows = 0;
                self.status = PipelineStatus::Waiting;
                self.diagnostic_span = None;
            }
            Err(error) => {
                let byte = match &error {
                    crate::note::NoteError::InvalidMath { byte, .. } => *byte,
                };
                self.generated_revision = None;
                self.status = PipelineStatus::Error(error.to_string());
                self.diagnostic_span = Some(SourceSpan {
                    start: byte,
                    end: byte.saturating_add(1),
                });
            }
        }
    }

    fn maybe_submit_compile(&mut self) {
        if matches!(self.status, PipelineStatus::Empty)
            || self.generated_revision != Some(self.revision)
            || self.submitted_revision == Some(self.revision)
            || self.last_edit.elapsed() < COMPILE_DEBOUNCE
        {
            return;
        }
        let Some(generated) = &self.generated else {
            return;
        };

        let target_width = self.target_raster_width();
        let request = WorkerRequest::Compile {
            revision: self.revision,
            document: generated.clone(),
            page_index: 0,
            target_width,
        };
        if self.worker_tx.send(request).is_ok() {
            self.submitted_revision = Some(self.revision);
            self.status = PipelineStatus::Compiling {
                bootstrap: !cache_is_warmed(),
            };
        } else {
            self.status = PipelineStatus::Error(String::from("preview worker stopped"));
        }
    }

    fn maybe_submit_raster(&mut self) {
        let Some(pdf) = &self.pdf else {
            return;
        };
        if self.pdf_revision != Some(self.revision) {
            return;
        }
        let target_width = self.target_raster_width();
        let key = (self.revision, self.page_index, target_width);
        if self.requested_raster == Some(key)
            || (self.full_page_width == target_width && self.full_page.is_some())
        {
            return;
        }

        if self
            .worker_tx
            .send(WorkerRequest::Raster {
                revision: self.revision,
                pdf: Arc::clone(pdf),
                page_index: self.page_index,
                target_width,
            })
            .is_ok()
        {
            self.requested_raster = Some(key);
            self.status = PipelineStatus::Rasterizing;
        }
    }

    fn process_background_events(&mut self) {
        while let Ok(result) = self.resize_rx.try_recv() {
            match result {
                Ok(response) => {
                    self.image_state.update_resized_protocol(response);
                }
                Err(error) => {
                    self.status = PipelineStatus::Error(format!("terminal image error: {error}"));
                }
            }
        }

        while let Ok(event) = self.worker_rx.try_recv() {
            match event {
                WorkerEvent::Ready {
                    revision,
                    pdf,
                    page_count,
                    page_index,
                    image,
                    width,
                    elapsed,
                    warnings,
                } if revision == self.revision => {
                    self.pdf = Some(pdf);
                    self.pdf_revision = Some(revision);
                    self.page_count = page_count;
                    self.page_index = page_index.min(page_count.saturating_sub(1));
                    self.full_page = Some(image);
                    self.full_page_width = width;
                    self.requested_raster = None;
                    self.preview_scroll_rows = 0;
                    self.status = PipelineStatus::Ready { elapsed, warnings };
                    self.install_visible_preview();
                }
                WorkerEvent::RasterReady {
                    revision,
                    page_index,
                    image,
                    width,
                } if revision == self.revision && page_index == self.page_index => {
                    self.full_page = Some(image);
                    self.full_page_width = width;
                    self.requested_raster = None;
                    self.status = PipelineStatus::Ready {
                        elapsed: Duration::ZERO,
                        warnings: 0,
                    };
                    self.install_visible_preview();
                }
                WorkerEvent::Failed {
                    revision,
                    message,
                    tex_line,
                } if revision == self.revision => {
                    self.requested_raster = None;
                    self.status = PipelineStatus::Error(message);
                    self.diagnostic_span = tex_line.and_then(|line| {
                        self.generated
                            .as_ref()
                            .and_then(|generated| generated.source_span_for_output_line(line))
                    });
                }
                _ => {}
            }
        }
    }

    fn install_visible_preview(&mut self) {
        let Some(page) = &self.full_page else {
            return;
        };
        let font = self.picker.font_size();
        let viewport_height = u32::from(self.preview_size.height.max(1)) * u32::from(font.height);
        let viewport_height = viewport_height.max(1);
        let max_y = page.height().saturating_sub(viewport_height);
        let requested_y = u32::from(self.preview_scroll_rows) * u32::from(font.height);
        let y = requested_y.min(max_y);
        self.preview_scroll_rows = (y / u32::from(font.height.max(1))) as u16;

        let crop_height = viewport_height.min(page.height().saturating_sub(y)).max(1);
        let cropped = page.crop_imm(0, y, page.width(), crop_height).to_rgba8();
        let mut canvas =
            ImageBuffer::from_pixel(page.width(), viewport_height, Rgba([255, 255, 255, 255]));
        imageops::replace(&mut canvas, &cropped, 0, 0);
        let protocol = self
            .picker
            .new_resize_protocol(DynamicImage::ImageRgba8(canvas));
        self.image_state.replace_protocol(protocol);
    }

    fn scroll_preview(&mut self, rows: i16) {
        let Some(page) = &self.full_page else {
            return;
        };
        let font_height = u32::from(self.picker.font_size().height.max(1));
        let viewport = u32::from(self.preview_size.height.max(1)) * font_height;
        let max_rows = page.height().saturating_sub(viewport).div_ceil(font_height) as u16;
        self.preview_scroll_rows = self
            .preview_scroll_rows
            .saturating_add_signed(rows)
            .min(max_rows);
        self.install_visible_preview();
    }

    fn scroll_latex(&mut self, rows: i16) {
        self.latex_scroll_rows = self
            .latex_scroll_rows
            .saturating_add_signed(rows)
            .min(self.max_latex_scroll());
    }

    fn max_latex_scroll(&self) -> u16 {
        let line_count = self.generated_body().lines().count().max(1);
        let viewport = usize::from(self.latex_size.height.max(1));
        line_count
            .saturating_sub(viewport)
            .min(usize::from(u16::MAX)) as u16
    }

    fn change_page(&mut self, delta: isize) {
        if self.page_count == 0 {
            return;
        }
        let next = self
            .page_index
            .saturating_add_signed(delta)
            .min(self.page_count - 1);
        if next != self.page_index {
            self.page_index = next;
            self.preview_scroll_rows = 0;
            self.full_page = None;
            self.full_page_width = 0;
            self.requested_raster = None;
            self.maybe_submit_raster();
        }
    }

    fn target_raster_width(&self) -> u32 {
        if self.preview_size.width == 0 {
            return DEFAULT_RASTER_WIDTH;
        }
        let width = u32::from(self.preview_size.width) * u32::from(self.picker.font_size().width);
        width.clamp(320, 2400)
    }

    fn ensure_cursor_visible(&mut self) {
        let (line, column) = self.buffer.cursor_line_column();
        let height = usize::from(self.source_size.height.max(1));
        if line < self.source_scroll_y {
            self.source_scroll_y = line;
        } else if line >= self.source_scroll_y + height {
            self.source_scroll_y = line + 1 - height;
        }

        let line_text = self.buffer.line(line);
        let prefix: String = line_text.chars().take(column).collect();
        let display_column = unicode_width::UnicodeWidthStr::width(prefix.as_str()) as u16;
        let width = self.source_size.width.max(1);
        if display_column < self.source_scroll_x {
            self.source_scroll_x = display_column;
        } else if display_column >= self.source_scroll_x.saturating_add(width) {
            self.source_scroll_x = display_column + 1 - width;
        }
    }

    pub(crate) fn configure_layout(
        &mut self,
        source_size: Size,
        latex_size: Size,
        preview_size: Size,
    ) {
        let old_target = self.target_raster_width();
        self.source_size = source_size;
        self.latex_size = latex_size;
        self.preview_size = preview_size;
        self.latex_scroll_rows = self.latex_scroll_rows.min(self.max_latex_scroll());
        self.ensure_cursor_visible();
        if self.target_raster_width() != old_target {
            self.full_page_width = 0;
            self.requested_raster = None;
        }
    }

    pub(crate) fn source_line_count(&self) -> usize {
        self.buffer.len_lines()
    }

    pub(crate) fn source_line(&self, line: usize) -> String {
        self.buffer.line(line)
    }

    pub(crate) fn source_scroll(&self) -> (u16, u16) {
        (
            self.source_scroll_y.min(u16::MAX as usize) as u16,
            self.source_scroll_x,
        )
    }

    pub(crate) fn cursor_screen_position(&self) -> (u16, u16) {
        let (line, column) = self.buffer.cursor_line_column();
        let line_text = self.buffer.line(line);
        let prefix: String = line_text.chars().take(column).collect();
        let display_column = unicode_width::UnicodeWidthStr::width(prefix.as_str()) as u16;
        (
            display_column.saturating_sub(self.source_scroll_x),
            (line.saturating_sub(self.source_scroll_y)).min(u16::MAX as usize) as u16,
        )
    }

    pub(crate) fn generated_body(&self) -> &str {
        self.generated.as_ref().map_or("", LatexDocument::body)
    }

    pub(crate) fn focus(&self) -> PaneFocus {
        self.focus
    }

    pub(crate) fn latex_scroll(&self) -> u16 {
        self.latex_scroll_rows
    }

    pub(crate) fn diagnostic_line(&self) -> Option<usize> {
        let span = self.diagnostic_span.as_ref()?;
        let source = self.buffer.text();
        let byte = span.start.min(source.len());
        Some(source[..byte].bytes().filter(|byte| *byte == b'\n').count())
    }

    pub(crate) fn status_line(&self) -> String {
        match &self.status {
            PipelineStatus::Empty => String::from("type a note to begin"),
            PipelineStatus::Waiting => String::from("waiting for input to settle"),
            PipelineStatus::Compiling { bootstrap: true } => {
                String::from("preparing local LaTeX resources and compiling…")
            }
            PipelineStatus::Compiling { bootstrap: false } => {
                String::from("compiling with embedded Tectonic…")
            }
            PipelineStatus::Rasterizing => String::from("rendering PDF page…"),
            PipelineStatus::Ready { elapsed, warnings } if *warnings > 0 => format!(
                "ready in {} ms with {warnings} warning(s)",
                elapsed.as_millis()
            ),
            PipelineStatus::Ready { elapsed, .. } if !elapsed.is_zero() => {
                format!("ready in {} ms", elapsed.as_millis())
            }
            PipelineStatus::Ready { .. } => String::from("ready"),
            PipelineStatus::Error(message) => format!("error: {message}"),
        }
    }

    pub(crate) fn page_label(&self) -> String {
        if self.page_count == 0 {
            String::from("no page")
        } else {
            format!("page {}/{}", self.page_index + 1, self.page_count)
        }
    }

    pub(crate) fn protocol_label(&self) -> String {
        format!("{:?}", self.picker.protocol_type())
    }

    pub(crate) fn has_preview(&self) -> bool {
        self.full_page.is_some()
    }

    pub(crate) fn preview_placeholder(&self) -> &'static str {
        if matches!(self.status, PipelineStatus::Empty) {
            "Start typing to build a LaTeX document."
        } else {
            "Compiling the document preview…"
        }
    }

    pub(crate) fn image_state_mut(&mut self) -> &mut ThreadProtocol {
        &mut self.image_state
    }
}

fn pipeline_worker(requests: mpsc::Receiver<WorkerRequest>, events: mpsc::Sender<WorkerEvent>) {
    while let Ok(mut request) = requests.recv() {
        while let Ok(newer) = requests.try_recv() {
            request = newer;
        }

        match request {
            WorkerRequest::Compile {
                revision,
                document,
                page_index,
                target_width,
            } => {
                let started = Instant::now();
                match compile_latex(CompileRequest::new(revision, &document)) {
                    Ok(output) => {
                        let warnings = output
                            .diagnostics
                            .messages
                            .iter()
                            .filter(|message| message.kind == DiagnosticKind::Warning)
                            .count();
                        let pdf = Arc::new(output.pdf);
                        match inspect_pdf(pdf.as_slice()).and_then(|info| {
                            let selected = page_index.min(info.page_count.saturating_sub(1));
                            rasterize_page(pdf.as_slice(), selected, target_width)
                                .map(|preview| (info.page_count, preview))
                        }) {
                            Ok((page_count, preview)) => {
                                let _ = events.send(WorkerEvent::Ready {
                                    revision,
                                    pdf,
                                    page_count,
                                    page_index: preview.page_index,
                                    image: preview.image,
                                    width: preview.width,
                                    elapsed: started.elapsed(),
                                    warnings,
                                });
                            }
                            Err(error) => {
                                let _ = events.send(WorkerEvent::Failed {
                                    revision,
                                    message: error.to_string(),
                                    tex_line: None,
                                });
                            }
                        }
                    }
                    Err(error) => {
                        let (message, tex_line) = summarize_compile_error(&error);
                        let _ = events.send(WorkerEvent::Failed {
                            revision,
                            message,
                            tex_line,
                        });
                    }
                }
            }
            WorkerRequest::Raster {
                revision,
                pdf,
                page_index,
                target_width,
            } => match rasterize_page(pdf.as_slice(), page_index, target_width) {
                Ok(preview) => {
                    let _ = events.send(WorkerEvent::RasterReady {
                        revision,
                        page_index,
                        image: preview.image,
                        width: preview.width,
                    });
                }
                Err(error) => {
                    let _ = events.send(WorkerEvent::Failed {
                        revision,
                        message: error.to_string(),
                        tex_line: None,
                    });
                }
            },
        }
    }
}

fn summarize_compile_error(error: &CompileError) -> (String, Option<usize>) {
    let mut details = error.to_string();
    let mut tex_line = None;
    if let Some(diagnostics) = error.diagnostics() {
        let logs = diagnostics.error_logs.join("\n");
        let messages = diagnostics
            .messages
            .iter()
            .map(|message| {
                message.error.as_ref().map_or_else(
                    || message.message.clone(),
                    |error| format!("{}: {error}", message.message),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        tex_line = extract_tex_line(&format!("{logs}\n{messages}"));
        if let Some(line) = logs
            .lines()
            .find(|line| line.starts_with('!') || line.contains("error:"))
        {
            details.push_str(": ");
            details.push_str(line.trim());
        } else if let Some(message) = diagnostics.messages.last() {
            details.push_str(": ");
            details.push_str(&message.message);
        }
    }
    (details, tex_line)
}

fn extract_tex_line(log: &str) -> Option<usize> {
    let mut remainder = log;
    while let Some(index) = remainder.find("mathnote.tex:") {
        remainder = &remainder[index + "mathnote.tex:".len()..];
        let digits: String = remainder
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect();
        if let Ok(line) = digits.parse() {
            return Some(line);
        }
    }

    for marker in ["\nl.", " l."] {
        let mut remainder = log;
        while let Some(index) = remainder.find(marker) {
            remainder = &remainder[index + marker.len()..];
            let digits: String = remainder
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect();
            if let Ok(line) = digits.parse() {
                return Some(line);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn f6_cycles_focus_without_repurposing_tab() {
        let mut app = App::default();
        assert_eq!(app.focus(), PaneFocus::Source);

        app.handle_key(press(KeyCode::F(6)));
        assert_eq!(app.focus(), PaneFocus::Latex);
        app.handle_key(press(KeyCode::F(6)));
        assert_eq!(app.focus(), PaneFocus::Preview);
        app.handle_key(press(KeyCode::F(6)));
        assert_eq!(app.focus(), PaneFocus::Source);

        app.handle_key(press(KeyCode::Tab));
        assert_eq!(app.buffer.text(), "    ");
    }

    #[test]
    fn inspector_navigation_uses_h_and_l_for_focus() {
        let mut app = App::default();
        app.handle_key(press(KeyCode::F(6)));
        assert_eq!(app.focus(), PaneFocus::Latex);

        app.handle_key(press(KeyCode::Char('l')));
        assert_eq!(app.focus(), PaneFocus::Preview);
        app.handle_key(press(KeyCode::Char('h')));
        assert_eq!(app.focus(), PaneFocus::Latex);
    }

    #[test]
    fn blank_notes_do_not_submit_a_compile() {
        let mut app = App::default();
        app.maybe_submit_compile();

        assert!(matches!(app.status, PipelineStatus::Empty));
        assert_eq!(app.submitted_revision, None);
        assert_eq!(app.status_line(), "type a note to begin");
        assert_eq!(
            app.preview_placeholder(),
            "Start typing to build a LaTeX document."
        );
    }

    #[test]
    fn whitespace_only_notes_remain_in_the_empty_state() {
        let mut app = App::default();
        app.buffer.insert_str(" \n\t");
        app.mark_edited();
        app.maybe_submit_compile();

        assert!(matches!(app.status, PipelineStatus::Empty));
        assert_eq!(app.submitted_revision, None);
    }
}
