# mathnote

`mathnote` is a local-first terminal notebook for prose, proofs, and mathematical expressions. You type ordinary notes in the left editor. The application converts only explicit or unambiguous mathematics into canonical LaTeX, compiles a real document with embedded Tectonic, rasterizes its PDF with Hayro, and displays the page in the right pane.

It does not simulate radicals or integrals with terminal characters. All mathematical typography comes from LaTeX and Latin Modern Math.

## Run

```sh
cargo run
```

Tectonic is embedded as a Rust library. No external TeX executable or PDF viewer is used at runtime. Tectonic still links to native text libraries, so the build host and resulting executable need compatible ICU, FreeType, Graphite2, and libpng libraries. On macOS, the repository Cargo configuration discovers an existing Homebrew ICU installation from either the Apple Silicon or Intel prefix. The produced macOS binary is host-local rather than a standalone relocatable bundle, so rebuild it on a destination with matching native libraries.

The first launch retrieves only the files required by mathnote's fixed TeX template from one pinned format-33 Tectonic bundle. Mathnote rejects any bundle whose cryptographic content digest differs from the compiled-in expected digest, then caches the verified resources under the platform cache directory. Later compilations lock Tectonic to cache-only operation and work offline. If that verified cache becomes unusable, mathnote invalidates it and makes one online repair attempt. Bootstrap or repair failures are reported without discarding the authored note.

## Notebook workflow

The interface has one fixed layout:

- **Natural note**: multiline editable source.
- **Generated LaTeX body**: read-only conversion output. The immutable document preamble is hidden.
- **LaTeX document**: the actual compiled PDF page.
- **Status line**: bootstrap, compilation, rasterization, readiness, or source-mapped errors.

The preview uses Kitty, Sixel, or iTerm2 images when detected. Other terminals receive the same PDF bitmap through a half-block transport. The mathematical renderer never changes.

## Authoring

Ordinary English remains ordinary English:

```text
The words square root remain readable in this sentence.
```

Use paired dollar signs to identify exact inline mathematics written with natural phrases:

```text
Since $root of 81$ equals 9, the result follows.
```

Use double-dollar fences for display mathematics:

```text
$$
x squared plus y squared equals z squared
$$
```

Supported deterministic language includes arithmetic, fractions, powers, roots, relations, number words, common Greek names, and indefinite integrals. Examples:

```text
$x squared plus y squared$
$x over 2$
$root of 81$
$x is greater than or equal to 0$
$integral of x squared plus 1$
```

Ambiguous or incomplete automatic phrases remain prose. Malformed explicit math and raw LaTeX commands fail closed with a diagnostic instead of being compiled. Prose is LaTeX-escaped, so text such as `\input{file}` cannot inject a TeX command.

## Keys

- Arrow keys: move through the multiline source.
- `Home` / `End`: move to the beginning or end of the current line.
- `Enter`: insert a new line.
- `Backspace` / `Delete`: edit Unicode text safely.
- `Ctrl-U`: clear the complete document safely.
- `Ctrl-Up` / `Ctrl-Down`: scroll the PDF page.
- `PageUp` / `PageDown`: move between PDF pages.
- `Esc` or `Ctrl-C`: quit.

Compilation starts after 350 ms without an edit. Tectonic and Hayro run outside the event thread. Every request carries a document revision, so a slower old result cannot replace a newer note. While a new revision is compiling, the last successful page remains visible.

## Architecture

```text
Rope text buffer
    ↓
source-spanned semantic document
    ↓
deterministic LaTeX emitter and fixed A4 template
    ↓
embedded Tectonic PDF compiler
    ↓
Hayro page rasterizer
    ↓
ratatui-image protocol transport
```

The fixed document template uses A4 paper, 11-point text, 20 mm margins, Latin Modern Roman, Latin Modern Math, and the standard AMS proof and mathematics packages. The compiler uses restrictive Tectonic defaults, disables shell escape and external output files, and operates inside an application cache sandbox.

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

The integration coverage exercises mixed prose and mathematics, exact source spans, malformed delimiters, Unicode multiline editing and clearing, embedded PDF compilation, Hayro parsing, and visible raster output.
