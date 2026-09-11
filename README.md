# mathnote

`mathnote` is a ground-up Rust replacement for the former C++ snippet manager. It is becoming a
local-first terminal notebook where students can enter mathematics naturally, inspect the generated
LaTeX, and see an immediate terminal-rendered preview.

## Current foundation

- Ratatui application shell and Crossterm input handling
- Editable single-line LaTeX input
- Live terminal math preview through txm
- Unicode-safe cursor movement and editing
- Headless UI and editing tests

Run it with:

```sh
cargo run
```

Type a supported LaTeX expression. Press `Esc` or `Ctrl-C` to quit.

## Migration roadmap

1. Introduce notebook and ordered block models backed by SQLite.
2. Replace the foundation editor with a multiline Rope-backed note editor.
3. Add deterministic natural-language mathematics parsing into a constrained internal AST.
4. Add an optional local LLM fallback for ambiguous expressions.
5. Validate generated LaTeX against txm's supported command set.
6. Add Markdown and `.tex` export, search, autosave, and recovery.

The original natural-language phrase and generated LaTeX will be stored separately so conversion
can always be reviewed, corrected, or regenerated.
