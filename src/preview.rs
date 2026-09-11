//! In-memory PDF preview rendering for the live LaTeX pipeline.
//!
//! Public API summary:
//! - [`page_count`] parses PDF bytes with Hayro and returns the number of pages.
//! - [`rasterize_page`] renders one zero-based page to an opaque white RGBA [`image::DynamicImage`]
//!   at a requested target width.
//! - Errors are structured as [`PreviewError`].
//!
//! This module deliberately contains no ratatui or application UI state.

use std::fmt::{self, Display};

use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{
    RenderCache, RenderSettings, hayro_interpret::InterpreterSettings, hayro_syntax::Pdf, render,
};
use image::{DynamicImage, ImageBuffer, Rgba};

/// Metadata extracted from an in-memory PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfInfo {
    pub page_count: usize,
}

/// Successful page render output.
#[derive(Debug)]
pub struct PagePreview {
    pub page_index: usize,
    pub width: u32,
    pub height: u32,
    pub image: DynamicImage,
}

/// Structured PDF preview failures.
#[derive(Debug)]
pub enum PreviewError {
    EmptyPdf,
    Parse {
        message: String,
    },
    InvalidTargetWidth {
        target_width: u32,
    },
    PageOutOfRange {
        requested: usize,
        page_count: usize,
    },
    DimensionsTooLarge {
        width: u32,
        height: u32,
    },
    ImageBufferSize {
        width: u32,
        height: u32,
        byte_len: usize,
    },
}

impl Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPdf => write!(f, "PDF buffer is empty"),
            Self::Parse { message } => write!(f, "failed to parse PDF: {message}"),
            Self::InvalidTargetWidth { target_width } => {
                write!(
                    f,
                    "target width must be greater than zero, got {target_width}"
                )
            }
            Self::PageOutOfRange {
                requested,
                page_count,
            } => write!(
                f,
                "page index {requested} is out of range for {page_count} page(s)"
            ),
            Self::DimensionsTooLarge { width, height } => write!(
                f,
                "render dimensions {width}x{height} exceed Hayro's u16 viewport limit"
            ),
            Self::ImageBufferSize {
                width,
                height,
                byte_len,
            } => write!(
                f,
                "could not build RGBA image {width}x{height} from {byte_len} bytes"
            ),
        }
    }
}

impl std::error::Error for PreviewError {}

/// Parse PDF bytes and return page metadata.
pub fn inspect_pdf(pdf_bytes: &[u8]) -> Result<PdfInfo, PreviewError> {
    let pdf = parse_pdf(pdf_bytes)?;
    Ok(PdfInfo {
        page_count: pdf.pages().len(),
    })
}

/// Parse PDF bytes and return the number of pages.
pub fn page_count(pdf_bytes: &[u8]) -> Result<usize, PreviewError> {
    inspect_pdf(pdf_bytes).map(|info| info.page_count)
}

/// Rasterize a zero-based page to an opaque white RGBA image at `target_width` pixels.
pub fn rasterize_page(
    pdf_bytes: &[u8],
    page_index: usize,
    target_width: u32,
) -> Result<PagePreview, PreviewError> {
    if target_width == 0 {
        return Err(PreviewError::InvalidTargetWidth { target_width });
    }

    let pdf = parse_pdf(pdf_bytes)?;
    let pages = pdf.pages();
    let page_count = pages.len();
    let page = pages.get(page_index).ok_or(PreviewError::PageOutOfRange {
        requested: page_index,
        page_count,
    })?;

    let (native_width, native_height) = page.render_dimensions();
    let scale = target_width as f32 / native_width.max(1.0);
    let target_height = ((native_height * scale).round() as u32).max(1);

    if target_width > u16::MAX as u32 || target_height > u16::MAX as u32 {
        return Err(PreviewError::DimensionsTooLarge {
            width: target_width,
            height: target_height,
        });
    }

    let cache = RenderCache::new();
    let interpreter_settings = InterpreterSettings::default();
    let render_settings = RenderSettings {
        x_scale: scale,
        y_scale: scale,
        width: Some(target_width as u16),
        height: Some(target_height as u16),
        bg_color: WHITE,
    };

    let pixmap = render(page, &cache, &interpreter_settings, &render_settings);
    let rgba = flatten_premultiplied_rgba_over_white(pixmap.data_as_u8_slice());
    let width = u32::from(pixmap.width());
    let height = u32::from(pixmap.height());
    let byte_len = rgba.len();
    let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, rgba)
        .map(DynamicImage::ImageRgba8)
        .ok_or(PreviewError::ImageBufferSize {
            width,
            height,
            byte_len,
        })?;

    Ok(PagePreview {
        page_index,
        width,
        height,
        image,
    })
}

fn parse_pdf(pdf_bytes: &[u8]) -> Result<Pdf, PreviewError> {
    if pdf_bytes.is_empty() {
        return Err(PreviewError::EmptyPdf);
    }

    Pdf::new(pdf_bytes.to_vec()).map_err(|err| PreviewError::Parse {
        message: format!("{err:?}"),
    })
}

fn flatten_premultiplied_rgba_over_white(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());

    for pixel in bytes.as_chunks::<4>().0 {
        let alpha = u16::from(pixel[3]);
        let inverse_alpha = 255_u16.saturating_sub(alpha);

        out.push((u16::from(pixel[0]) + inverse_alpha).min(255) as u8);
        out.push((u16::from(pixel[1]) + inverse_alpha).min(255) as u8);
        out.push((u16::from(pixel[2]) + inverse_alpha).min(255) as u8);
        out.push(255);
    }

    out
}
