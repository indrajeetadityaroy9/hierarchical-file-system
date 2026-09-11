//! Isolated synchronous LaTeX-to-PDF compilation for the live preview pipeline.
//!
//! Public API summary:
//! - [`CompileRequest`] carries the caller revision and generated LaTeX string.
//! - [`compile_latex`] blocks while Tectonic compiles and returns [`CompileOutput`].
//! - [`CompileOutput::pdf`] contains in-memory PDF bytes, while [`CompileOutput::diagnostics`]
//!   captures Tectonic status messages and error logs.
//!
//! Bundle integrity:
//! The compiler uses one exact format-33 URL and rejects the bundle unless Tectonic reports the
//! expected cryptographic content digest. After the first successful compile, the verified cache
//! is used in offline-only mode.

use std::{
    env,
    fmt::{self, Arguments, Display},
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use directories::ProjectDirs;
use tectonic::{
    driver::{OutputFormat, ProcessingSessionBuilder},
    status::{MessageKind, StatusBackend},
};
use tectonic_bundles::detect_bundle;

use crate::latex::LatexDocument;

const FORMAT_VERSION: u32 = 33;
const BUNDLE_URL: &str = "https://data1b.fullyjustified.net/tlextras-2022.0r0.tar";
const BUNDLE_DIGEST: &str = "6ffe055852f8faf66c0acbe1a7fb27f87b869a90bad1204f3bf4d9683f597c7c";
const TEX_INPUT_NAME: &str = "mathnote.tex";
const PDF_OUTPUT_NAME: &str = "mathnote.pdf";
const CACHE_READY_MARKER: &str = "bundle-v33.ready";

/// Input for a background-friendly synchronous compile operation.
#[derive(Debug, Clone)]
pub struct CompileRequest {
    /// Caller-owned revision used to discard stale preview results.
    revision: u64,
    /// Complete generated LaTeX document source.
    latex: String,
}

impl CompileRequest {
    /// Create a compile request from the canonical emitter output.
    ///
    /// Accepting [`LatexDocument`] rather than an arbitrary string keeps cache warm-up ownership
    /// and compiler safety tied to mathnote's one fixed document mechanism.
    pub fn new(revision: u64, document: &LatexDocument) -> Self {
        Self {
            revision,
            latex: document.source().to_owned(),
        }
    }
}

/// Optional controls for the compiler.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// Use this cache root instead of the default application cache directory.
    pub cache_dir: Option<PathBuf>,
    /// Require all bundle files to already be cached. Use after first successful warm-up.
    pub only_cached: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        let cache_dir = default_cache_dir();
        Self {
            only_cached: cache_marker_is_valid(&cache_dir),
            cache_dir: Some(cache_dir),
        }
    }
}

pub fn cache_is_warmed() -> bool {
    cache_marker_is_valid(&default_cache_dir())
}

/// Successful compile output.
#[derive(Debug, Clone)]
pub struct CompileOutput {
    pub revision: u64,
    pub pdf: Vec<u8>,
    pub diagnostics: CompileDiagnostics,
    pub cache_dir: PathBuf,
    pub bundle_url: String,
    pub bundle_digest: String,
}

/// Captured status messages and engine logs.
#[derive(Debug, Clone, Default)]
pub struct CompileDiagnostics {
    pub messages: Vec<DiagnosticMessage>,
    pub error_logs: Vec<String>,
}

impl CompileDiagnostics {
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.error_logs.is_empty()
    }
}

/// One structured status message emitted by Tectonic.
#[derive(Debug, Clone)]
pub struct DiagnosticMessage {
    pub kind: DiagnosticKind,
    pub message: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    Note,
    Warning,
    Error,
}

/// Structured compilation failures.
#[derive(Debug)]
pub enum CompileError {
    CacheDirectory {
        path: PathBuf,
        message: String,
    },
    Bundle {
        message: String,
        diagnostics: CompileDiagnostics,
    },
    Session {
        message: String,
        diagnostics: CompileDiagnostics,
    },
    Engine {
        message: String,
        diagnostics: CompileDiagnostics,
    },
    MissingPdf {
        diagnostics: CompileDiagnostics,
    },
}

impl CompileError {
    pub fn diagnostics(&self) -> Option<&CompileDiagnostics> {
        match self {
            Self::CacheDirectory { .. } => None,
            Self::Bundle { diagnostics, .. }
            | Self::Session { diagnostics, .. }
            | Self::Engine { diagnostics, .. }
            | Self::MissingPdf { diagnostics } => Some(diagnostics),
        }
    }
}

impl Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CacheDirectory { path, message } => {
                write!(
                    f,
                    "failed to prepare cache directory {}: {message}",
                    path.display()
                )
            }
            Self::Bundle { message, .. } => write!(f, "failed to load Tectonic bundle: {message}"),
            Self::Session { message, .. } => {
                write!(f, "failed to create Tectonic session: {message}")
            }
            Self::Engine { message, .. } => write!(f, "LaTeX engine failed: {message}"),
            Self::MissingPdf { .. } => {
                write!(f, "Tectonic completed without producing {PDF_OUTPUT_NAME}")
            }
        }
    }
}

impl std::error::Error for CompileError {}

/// Compile LaTeX to PDF bytes using default options.
pub fn compile_latex(request: CompileRequest) -> Result<CompileOutput, CompileError> {
    let options = CompileOptions::default();
    match compile_latex_with_options(request.clone(), options.clone()) {
        Ok(output) => Ok(output),
        Err(_) if options.only_cached => {
            let cache_dir = options.cache_dir.unwrap_or_else(default_cache_dir);
            invalidate_cache(&cache_dir);
            compile_latex_with_options(
                request,
                CompileOptions {
                    cache_dir: Some(cache_dir),
                    only_cached: false,
                },
            )
        }
        Err(error) => Err(error),
    }
}

/// Compile LaTeX to PDF bytes synchronously. Safe to call from a background worker.
pub fn compile_latex_with_options(
    request: CompileRequest,
    options: CompileOptions,
) -> Result<CompileOutput, CompileError> {
    let cache_dir = options.cache_dir.unwrap_or_else(default_cache_dir);
    fs::create_dir_all(&cache_dir).map_err(|err| CompileError::CacheDirectory {
        path: cache_dir.clone(),
        message: err.to_string(),
    })?;
    let format_cache = cache_dir.join("formats");
    let sandbox = cache_dir.join("sandbox");
    for path in [&format_cache, &sandbox] {
        fs::create_dir_all(path).map_err(|err| CompileError::CacheDirectory {
            path: path.clone(),
            message: err.to_string(),
        })?;
    }

    let mut status = CapturingStatus::default();
    let bundle_url = String::from(BUNDLE_URL);
    if options.only_cached {
        refresh_cached_digest_check(&cache_dir).map_err(|message| CompileError::Bundle {
            message,
            diagnostics: status.diagnostics.clone(),
        })?;
    }
    let mut bundle = detect_bundle(
        bundle_url.clone(),
        options.only_cached,
        Some(cache_dir.clone()),
    )
    .map_err(|err| CompileError::Bundle {
        message: err.to_string(),
        diagnostics: status.diagnostics.clone(),
    })?
    .ok_or_else(|| CompileError::Bundle {
        message: format!("could not detect bundle source {bundle_url}"),
        diagnostics: status.diagnostics.clone(),
    })?;
    let bundle_digest = bundle
        .get_digest()
        .map_err(|err| CompileError::Bundle {
            message: format!("could not verify bundle digest: {err}"),
            diagnostics: status.diagnostics.clone(),
        })?
        .to_string();
    if bundle_digest != BUNDLE_DIGEST {
        return Err(CompileError::Bundle {
            message: format!(
                "bundle digest mismatch: expected {BUNDLE_DIGEST}, received {bundle_digest}"
            ),
            diagnostics: status.diagnostics.clone(),
        });
    }

    let mut builder = ProcessingSessionBuilder::default();
    builder
        .bundle(bundle)
        .primary_input_buffer(request.latex.as_bytes())
        .tex_input_name(TEX_INPUT_NAME)
        .filesystem_root(&sandbox)
        .format_name("latex")
        .format_cache_path(format_cache)
        .keep_logs(true)
        .keep_intermediates(false)
        .print_stdout(false)
        .output_format(OutputFormat::Pdf)
        .do_not_write_output_files();

    let mut session = builder
        .create(&mut status)
        .map_err(|err| CompileError::Session {
            message: err.to_string(),
            diagnostics: status.diagnostics.clone(),
        })?;

    session
        .run(&mut status)
        .map_err(|err| CompileError::Engine {
            message: err.to_string(),
            diagnostics: status.diagnostics.clone(),
        })?;

    let mut files = session.into_file_data();
    let pdf = files
        .remove(PDF_OUTPUT_NAME)
        .map(|file| file.data)
        .ok_or_else(|| CompileError::MissingPdf {
            diagnostics: status.diagnostics.clone(),
        })?;

    fs::write(cache_dir.join(CACHE_READY_MARKER), cache_marker_contents()).map_err(|err| {
        CompileError::CacheDirectory {
            path: cache_dir.join(CACHE_READY_MARKER),
            message: err.to_string(),
        }
    })?;

    Ok(CompileOutput {
        revision: request.revision,
        pdf,
        diagnostics: status.diagnostics,
        cache_dir,
        bundle_url,
        bundle_digest,
    })
}

fn cache_marker_contents() -> String {
    format!("format={FORMAT_VERSION}\ndigest={BUNDLE_DIGEST}\n")
}

fn cached_bundle_dir(cache_dir: &std::path::Path) -> PathBuf {
    cache_dir.join("data").join(BUNDLE_DIGEST)
}

/// Keep `tectonic_bundles` from performing its seven-day remote digest refresh in cache-only
/// mode. The digest was verified during bootstrap and is also bound into our ready marker.
fn refresh_cached_digest_check(cache_dir: &std::path::Path) -> Result<(), String> {
    if !cached_bundle_dir(cache_dir).is_dir() {
        return Err(format!(
            "verified local bundle cache is missing: {}",
            cached_bundle_dir(cache_dir).display()
        ));
    }

    let hashes = cache_dir.join("hashes");
    let entries = fs::read_dir(&hashes)
        .map_err(|error| format!("could not inspect bundle cache metadata: {error}"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))?
        .as_secs();

    for entry in entries {
        let path = entry
            .map_err(|error| format!("could not inspect bundle cache metadata: {error}"))?
            .path();
        if path
            .extension()
            .is_some_and(|extension| extension == "lock")
            || !path.is_file()
        {
            continue;
        }
        if fs::read_to_string(&path).is_ok_and(|contents| contents.trim() == BUNDLE_DIGEST) {
            fs::write(path.with_extension("lock"), now.to_string())
                .map_err(|error| format!("could not lock bundle cache to offline mode: {error}"))?;
            return Ok(());
        }
    }

    Err(String::from(
        "verified bundle digest metadata is missing from the local cache",
    ))
}

fn invalidate_cache(cache_dir: &std::path::Path) {
    let _ = fs::remove_file(cache_dir.join(CACHE_READY_MARKER));
    let _ = fs::remove_dir_all(cached_bundle_dir(cache_dir));
    let _ = fs::remove_dir_all(cache_dir.join("formats"));
}

fn cache_marker_is_valid(cache_dir: &std::path::Path) -> bool {
    fs::read_to_string(cache_dir.join(CACHE_READY_MARKER))
        .is_ok_and(|contents| contents == cache_marker_contents())
}

fn default_cache_dir() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("", "", "mathnote") {
        return project_dirs.cache_dir().join("tectonic");
    }

    if let Ok(dir) = env::var("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join("mathnote").join("tectonic");
    }

    env::temp_dir().join("mathnote").join("tectonic")
}

#[derive(Debug, Default)]
struct CapturingStatus {
    diagnostics: CompileDiagnostics,
}

impl StatusBackend for CapturingStatus {
    fn report(&mut self, kind: MessageKind, args: Arguments<'_>, err: Option<&tectonic::Error>) {
        let kind = match kind {
            MessageKind::Note => DiagnosticKind::Note,
            MessageKind::Warning => DiagnosticKind::Warning,
            MessageKind::Error => DiagnosticKind::Error,
        };

        self.diagnostics.messages.push(DiagnosticMessage {
            kind,
            message: args.to_string(),
            error: err.map(ToString::to_string),
        });
    }

    fn dump_error_logs(&mut self, output: &[u8]) {
        self.diagnostics
            .error_logs
            .push(String::from_utf8_lossy(output).into_owned());
    }
}
