use crate::document::{Block, Document, Inline, SourceMapEntry, SourceSpan};
use crate::note::unicode_math_command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatexDocument {
    source: String,
    body: String,
    source_map: Vec<SourceMapEntry>,
}

impl LatexDocument {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn source_map(&self) -> &[SourceMapEntry] {
        &self.source_map
    }

    pub fn source_span_for_output_line(&self, line: usize) -> Option<SourceSpan> {
        if line == 0 {
            return None;
        }
        let start = if line == 1 {
            0
        } else {
            self.source
                .match_indices('\n')
                .nth(line - 2)
                .map(|(byte, _)| byte + 1)?
        };
        let end = self.source[start..]
            .find('\n')
            .map_or(self.source.len(), |offset| start + offset);

        self.source_map
            .iter()
            .find(|entry| entry.output.start <= end && entry.output.end >= start)
            .or_else(|| {
                self.source_map
                    .iter()
                    .rev()
                    .find(|entry| entry.output.end <= start)
            })
            .map(|entry| entry.source.clone())
    }
}

pub fn emit_latex(document: &Document) -> LatexDocument {
    let mut emitter = Emitter {
        output: String::new(),
        source_map: Vec::new(),
    };

    for block in document.blocks() {
        match block {
            Block::Paragraph { inlines, .. } => {
                for inline in inlines {
                    match inline {
                        Inline::Text { text, source } => {
                            emitter.push(&escape_prose(text), Some(source.clone()));
                        }
                        Inline::Math { latex, span, .. } => {
                            emitter.push("\\(", None);
                            emitter.push(latex, Some(span.clone()));
                            emitter.push("\\)", None);
                        }
                    }
                }
                emitter.push("\n\n", None);
            }
            Block::DisplayMath { latex, span, .. } => {
                emitter.push("\\[\n", None);
                emitter.push(latex, Some(span.clone()));
                emitter.push("\n\\]\n\n", None);
            }
            Block::Blank { .. } => {
                emitter.push("\n", None);
            }
        }
    }

    let body = emitter.output;
    let mut source_map = emitter.source_map;
    for entry in &mut source_map {
        entry.output.start += TEMPLATE_PREFIX.len();
        entry.output.end += TEMPLATE_PREFIX.len();
    }

    LatexDocument {
        source: format!("{TEMPLATE_PREFIX}{body}{TEMPLATE_SUFFIX}"),
        body,
        source_map,
    }
}

pub fn escape_prose(input: &str) -> String {
    let mut escaped = String::new();
    for character in input.chars() {
        if let Some(command) = unicode_math_command(character) {
            escaped.push_str(r"\ensuremath{");
            escaped.push_str(command);
            escaped.push('}');
            continue;
        }
        match character {
            '\\' => escaped.push_str(r"\textbackslash{}"),
            '{' => escaped.push_str(r"\{"),
            '}' => escaped.push_str(r"\}"),
            '$' => escaped.push_str(r"\$"),
            '&' => escaped.push_str(r"\&"),
            '%' => escaped.push_str(r"\%"),
            '#' => escaped.push_str(r"\#"),
            '_' => escaped.push_str(r"\_"),
            '^' => escaped.push_str(r"\textasciicircum{}"),
            '~' => escaped.push_str(r"\textasciitilde{}"),
            '<' => escaped.push_str(r"\textless{}"),
            '>' => escaped.push_str(r"\textgreater{}"),
            _ => escaped.push(character),
        }
    }
    escaped
}

struct Emitter {
    output: String,
    source_map: Vec<SourceMapEntry>,
}

impl Emitter {
    fn push(&mut self, text: &str, source: Option<SourceSpan>) {
        let start = self.output.len();
        self.output.push_str(text);
        if let Some(source) = source {
            self.source_map.push(SourceMapEntry {
                source,
                output: SourceSpan {
                    start,
                    end: self.output.len(),
                },
            });
        }
    }
}

const TEMPLATE_PREFIX: &str = r"\documentclass[a4paper,11pt]{article}
\usepackage[margin=20mm]{geometry}
\pagestyle{plain}
\setlength{\parindent}{0pt}
\setlength{\parskip}{0.6em}
\emergencystretch=2em
\begin{document}
\null

";

const TEMPLATE_SUFFIX: &str = r"\end{document}
";
