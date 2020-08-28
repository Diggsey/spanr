use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use html_escape::encode_text;
use proc_macro2::{Delimiter, LineColumn, Spacing, Span, TokenStream, TokenTree};

const TEMPLATE_HTML: &str = include_str!("template.html");

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Position {
    line: usize,
    column: usize,
}

impl From<LineColumn> for Position {
    fn from(other: LineColumn) -> Self {
        Self {
            line: other.line,
            column: other.column,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum SeparatorType {
    Start,
    End,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct RangeSeparator {
    pos: Position,
    sep: SeparatorType,
    idx: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct Range {
    source: usize,
    start: Position,
    end: Position,
}

#[derive(Default, Debug, Clone)]
struct Ranges {
    sources: HashMap<PathBuf, usize>,
    ranges: HashMap<Range, usize>,
    generated: Vec<(String, Option<usize>)>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum NeedsSpace {
    Never,
    Always,
    IfNotPunct,
}

#[derive(Debug, Clone)]
struct TokenVisitor {
    ranges: Ranges,
    indent: usize,
    newline: bool,
    needs_space: NeedsSpace,
}

impl TokenVisitor {
    fn add_span(&mut self, span: Span) -> Option<usize> {
        let source_file = span.source_file();
        if source_file.is_real() {
            let num_sources = self.ranges.sources.len();
            let source = *self
                .ranges
                .sources
                .entry(source_file.path())
                .or_insert(num_sources);
            let range = Range {
                source,
                start: span.start().into(),
                end: span.end().into(),
            };
            let num_ranges = self.ranges.ranges.len();
            let range_idx = *self.ranges.ranges.entry(range).or_insert(num_ranges);
            Some(range_idx)
        } else {
            None
        }
    }
    fn add_str(&mut self, s: &str, range_idx: Option<usize>) {
        self.ranges.generated.push((s.into(), range_idx));
    }
    fn visit_str(&mut self, s: &str, span: Span) {
        if s.is_empty() {
            return;
        }
        let range_idx = self.add_span(span);
        match s {
            "}" => {
                self.indent -= 1;
                if !self.newline {
                    self.newline = true;
                    self.add_str("\n", None);
                }
            }
            _ => {}
        }
        if self.newline {
            self.newline = false;
            for _ in 0..self.indent {
                self.add_str("    ", None);
            }
        }
        self.add_str(s, range_idx);
        match s {
            "{" => {
                self.indent += 1;
                self.newline = true;
                self.add_str("\n", None);
            }
            ";" | "}" => {
                self.newline = true;
                self.add_str("\n", None);
            }
            _ => {}
        }
        if self.newline {
            self.needs_space = NeedsSpace::Never;
        }
    }
    fn visit_token_stream(&mut self, token_stream: TokenStream) {
        for token_tree in token_stream {
            self.visit_token_tree(token_tree);
        }
    }
    fn visit_token_tree(&mut self, token_tree: TokenTree) {
        match token_tree {
            TokenTree::Group(group) => {
                self.visit_str(
                    match group.delimiter() {
                        Delimiter::Parenthesis => "(",
                        Delimiter::Brace => "{",
                        Delimiter::Bracket => "[",
                        Delimiter::None => "",
                    },
                    group.span_open(),
                );
                self.visit_token_stream(group.stream());
                self.visit_str(
                    match group.delimiter() {
                        Delimiter::Parenthesis => ")",
                        Delimiter::Brace => "}",
                        Delimiter::Bracket => "]",
                        Delimiter::None => "",
                    },
                    group.span_close(),
                );
            }
            TokenTree::Ident(ident) => {
                if self.needs_space != NeedsSpace::Never {
                    self.add_str(" ", None);
                }
                self.visit_str(&ident.to_string(), ident.span());
                self.needs_space = NeedsSpace::IfNotPunct;
            }
            TokenTree::Punct(punct) => {
                if self.needs_space == NeedsSpace::Always {
                    self.add_str(" ", None);
                }
                self.visit_str(&punct.to_string(), punct.span());
                if punct.spacing() == Spacing::Alone {
                    self.needs_space = NeedsSpace::Always;
                }
            }
            TokenTree::Literal(literal) => {
                if self.needs_space != NeedsSpace::Never {
                    self.add_str(" ", None);
                }
                self.visit_str(&literal.to_string(), literal.span());
                self.needs_space = NeedsSpace::IfNotPunct;
            }
        }
    }
}

fn generate_ranges(token_stream: TokenStream) -> Ranges {
    let mut res = TokenVisitor {
        ranges: Default::default(),
        indent: 0,
        newline: true,
        needs_space: NeedsSpace::Never,
    };
    res.visit_token_stream(token_stream);
    res.ranges
}

struct LoadedSource {
    path: PathBuf,
    text: Vec<String>,
    range_seps: Vec<RangeSeparator>,
}

struct SourceParts {
    parts: Vec<(String, Vec<usize>)>,
}

impl SourceParts {
    fn add_unspanned(&mut self, s: &str) {
        self.parts.push((s.into(), Vec::new()));
    }
    fn add(&mut self, s: String, indexes: &HashSet<usize>) {
        self.parts.push((s, indexes.iter().copied().collect()));
    }
}

fn load_original_source(ranges: &Ranges) -> SourceParts {
    let mut loaded_sources: HashMap<_, _> = ranges
        .sources
        .iter()
        .filter_map(|(path, &source)| {
            let text = fs::read_to_string(path).ok()?;
            Some((
                source,
                LoadedSource {
                    path: path.clone(),
                    text: text.lines().map(Into::into).collect(),
                    range_seps: Vec::new(),
                },
            ))
        })
        .collect();

    for (&range, &idx) in &ranges.ranges {
        if let Some(loaded_source) = loaded_sources.get_mut(&range.source) {
            loaded_source.range_seps.push(RangeSeparator {
                pos: range.start,
                sep: SeparatorType::Start,
                idx,
            });
            loaded_source.range_seps.push(RangeSeparator {
                pos: range.end,
                sep: SeparatorType::End,
                idx,
            });
        }
    }

    let mut source_parts = SourceParts { parts: Vec::new() };
    for loaded_source in loaded_sources.values_mut() {
        loaded_source.range_seps.sort();

        source_parts.add_unspanned("\n");
        source_parts.add_unspanned("//======================================");
        source_parts.add_unspanned("\n");
        source_parts.add_unspanned("// ");
        source_parts.add_unspanned(&loaded_source.path.display().to_string());
        source_parts.add_unspanned("\n");
        source_parts.add_unspanned("//======================================");
        source_parts.add_unspanned("\n");

        let mut indexes = HashSet::new();
        let mut pos = loaded_source.range_seps.first().unwrap().pos;
        pos.column = 0;

        for range_sep in &loaded_source.range_seps {
            while pos.line < range_sep.pos.line {
                if let Some(line_text) = loaded_source.text.get(pos.line.wrapping_sub(1)) {
                    let s: String = line_text.chars().skip(pos.column).collect();
                    source_parts.add(s, &indexes);
                }
                source_parts.add_unspanned("\n");
                pos.line += 1;
                pos.column = 0;
            }
            if pos.column < range_sep.pos.column {
                if let Some(line_text) = loaded_source.text.get(pos.line.wrapping_sub(1)) {
                    let s: String = line_text
                        .chars()
                        .skip(pos.column)
                        .take(range_sep.pos.column - pos.column)
                        .collect();
                    source_parts.add(s, &indexes);
                }
                pos.column = range_sep.pos.column;
            }
            match range_sep.sep {
                SeparatorType::Start => {
                    indexes.insert(range_sep.idx);
                }
                SeparatorType::End => {
                    indexes.remove(&range_sep.idx);
                }
            }
        }
        if let Some(line_text) = loaded_source.text.get(pos.line.wrapping_sub(1)) {
            let s: String = line_text.chars().skip(pos.column).collect();
            source_parts.add(s, &indexes);
        }
        source_parts.add_unspanned("\n");
    }
    source_parts
}

fn generate_html_parts<T: IntoIterator<Item = usize>>(parts: Vec<(String, T)>) -> String {
    let mut res = String::new();
    res += "<div>";
    for (text, indexes) in parts {
        if text == "\n" {
            res += "</div><div>";
        } else {
            res += "<span class=\"";
            for idx in indexes {
                res += &format!("c{} ", idx);
            }
            res += "\">";
            res += &encode_text(&text);
            res += "</span>"
        }
    }
    res += "</div>";
    res
}

pub fn generate_html(token_stream: TokenStream) -> String {
    let ranges = generate_ranges(token_stream);
    let source_parts = load_original_source(&ranges);
    let lhs = generate_html_parts(ranges.generated);
    let rhs = generate_html_parts(source_parts.parts);
    TEMPLATE_HTML
        .replace("{LEFT}", &lhs)
        .replace("{RIGHT}", &rhs)
}

pub fn save_html(token_stream: TokenStream, path: impl AsRef<Path>) -> Result<(), std::io::Error> {
    let html = generate_html(token_stream);
    fs::write(path, html)
}

#[cfg(test)]
mod tests {
    use proc_macro2::TokenStream;

    #[test]
    fn it_works() {
        let x: TokenStream = "fn foo() -> i32 {\n42\n}\n".parse().unwrap();
        dbg!(x);
    }
}
