use crate::core::graph::DocMeta;
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use serde_yaml::Value;
use std::collections::{HashMap, HashSet};

/// Everything the doc pass extracts from one Markdown file.
/// Byte offsets and lines are relative to the original file, frontmatter included.
#[derive(Debug, Clone, Default)]
pub struct DocOutline {
    pub path: String,
    pub meta: Option<DocMeta>,
    pub sections: Vec<DocSection>,
    /// In document order, deduplicated per (section, kind, form).
    pub mentions: Vec<Mention>,
    /// In document order, one entry per occurrence.
    pub term_candidates: Vec<TermCandidate>,
}

/// One heading and its body, up to the next heading of the same or higher level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocSection {
    /// Heading text with inline markup flattened.
    pub name: String,
    pub level: u8,
    /// 1-based line of the heading.
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub anchor: String,
    /// Index into `DocOutline::sections`; `None` for a top section.
    pub parent: Option<usize>,
}

impl DocSection {
    pub fn kind(&self) -> &'static str {
        ["h1", "h2", "h3", "h4", "h5", "h6"][usize::from(self.level.clamp(1, 6)) - 1]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MentionKind {
    MarkdownLink,
    CodeSpan,
    FencedCode,
}

/// The rule form a mention was written in; resolution happens in the link pass.
/// Paths have a leading `./` stripped and are otherwise as written: Markdown link
/// targets are relative to the document, code paths to the index root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MentionForm {
    Path(String),
    PathSymbol { path: String, symbol: String },
    PathLine { path: String, line: usize },
    TypeMember { owner: String, member: String },
    BareName(String),
}

/// A raw doc -> code mention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub kind: MentionKind,
    pub form: MentionForm,
    pub byte: usize,
    /// Deepest section containing the mention. `None` means text before the
    /// first heading; the link pass attributes those to the file itself.
    pub section: Option<usize>,
}

/// One occurrence of a candidate term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermCandidate {
    pub term: String,
    pub byte: usize,
    /// `None` before the first heading, as for mentions.
    pub section: Option<usize>,
}

/// Leading words stripped from a capitalised run: they start sentences, not names.
const TERM_STOPWORDS: &[&str] = &[
    "The", "A", "An", "This", "That", "These", "Those", "Our", "Each", "Every", "When", "If", "In",
    "For", "Its", "Their", "Some", "All", "Any", "Then", "And", "But", "Or", "So", "Also", "See",
    "Use", "Note",
];

/// Marks a point in prose text that a capitalised run must not cross
/// (code spans, bold spans, images).
const RUN_BREAK: char = '\u{1}';

pub fn parse_markdown(path: &str, text: &str) -> DocOutline {
    let line_starts = line_starts(text);
    let mut sections: Vec<DocSection> = Vec::new();
    // Indices of sections still open, levels strictly increasing.
    let mut open: Vec<usize> = Vec::new();
    // (level, start_byte, text) of the heading being read.
    let mut heading: Option<(u8, usize, String)> = None;
    let mut slugger = Slugger::default();
    let mut c = Collector::default();

    let (meta, body_start) = match split_frontmatter(text) {
        Some((yaml, body_start)) => (parse_frontmatter(yaml), body_start),
        None => (None, 0),
    };
    // Some(true) inside a fenced block, Some(false) inside an indented one.
    let mut code_block: Option<bool> = None;
    // Open paragraphs and list items: the only places term text comes from.
    let mut prose_depth = 0usize;
    let mut strong_depth = 0usize;
    // Byte of the first text and the text of the outermost bold span, when it
    // counts as prose.
    let mut bold: Option<(usize, String)> = None;
    let mut image_depth = 0usize;

    // Frontmatter is cut off before parsing, so a '# comment' in it is never a
    // heading; event offsets are shifted back to be file-relative.
    for (event, range) in Parser::new(&text[body_start..]).into_offset_iter() {
        let range = range.start + body_start..range.end + body_start;
        // A mention inside a heading belongs to the section that heading opens.
        let section = if heading.is_some() {
            Some(sections.len())
        } else {
            open.last().copied()
        };
        match event {
            Event::Start(tag) => {
                if !is_inline(&tag) {
                    c.flush_prose(section);
                }
                match tag {
                    Tag::Heading { level, .. } => {
                        heading = Some((level as u8, range.start, String::new()));
                    }
                    Tag::CodeBlock(kind) => {
                        code_block = Some(matches!(kind, CodeBlockKind::Fenced(_)));
                    }
                    Tag::Paragraph | Tag::Item => prose_depth += 1,
                    Tag::Strong => {
                        if strong_depth == 0 && prose_depth > 0 && heading.is_none() {
                            bold = Some((range.start, String::new()));
                        }
                        strong_depth += 1;
                        c.prose.push_break();
                    }
                    Tag::Image { .. } => {
                        image_depth += 1;
                        c.prose.push_break();
                    }
                    Tag::Link { dest_url, .. } => {
                        if let Some(target) = link_target(&dest_url) {
                            c.mention(
                                MentionKind::MarkdownLink,
                                MentionForm::Path(target.to_string()),
                                range.start,
                                section,
                            );
                        }
                    }
                    _ => {}
                }
            }
            Event::End(tag) => {
                if !is_inline_end(&tag) {
                    c.flush_prose(section);
                }
                match tag {
                    TagEnd::Heading(_) => {
                        let Some((level, start, name)) = heading.take() else {
                            continue;
                        };
                        while let Some(&top) = open.last() {
                            if sections[top].level < level {
                                break;
                            }
                            sections[top].end_byte = start;
                            open.pop();
                        }
                        let anchor = slugger.slug(name.trim());
                        sections.push(DocSection {
                            name: collapse_whitespace(&name),
                            level,
                            line: line_of(&line_starts, start),
                            start_byte: start,
                            end_byte: text.len(),
                            anchor,
                            parent: open.last().copied(),
                        });
                        open.push(sections.len() - 1);
                    }
                    TagEnd::CodeBlock => code_block = None,
                    TagEnd::Paragraph | TagEnd::Item => prose_depth = prose_depth.saturating_sub(1),
                    TagEnd::Strong => {
                        strong_depth = strong_depth.saturating_sub(1);
                        if strong_depth == 0 {
                            if let Some((start, span)) = bold.take() {
                                c.bold_term(&span, start, section);
                            }
                        }
                        c.prose.push_break();
                    }
                    TagEnd::Image => {
                        image_depth = image_depth.saturating_sub(1);
                        c.prose.push_break();
                    }
                    _ => {}
                }
            }
            Event::Text(t) => {
                if let Some((_, _, name)) = heading.as_mut() {
                    name.push_str(&t);
                } else if let Some(fenced) = code_block {
                    if fenced {
                        c.code_mentions(MentionKind::FencedCode, &t, range.start, section);
                    }
                } else if image_depth > 0 {
                } else if let Some((start, span)) = bold.as_mut() {
                    if span.is_empty() {
                        *start = range.start;
                    }
                    span.push_str(&t);
                } else if prose_depth > 0 {
                    c.prose.push(&t, range.start);
                }
            }
            Event::Code(t) => {
                if let Some((_, _, name)) = heading.as_mut() {
                    name.push_str(&t);
                }
                c.code_mentions(MentionKind::CodeSpan, &t, range.start, section);
                c.prose.push_break();
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, _, name)) = heading.as_mut() {
                    name.push(' ');
                } else if let Some((_, span)) = bold.as_mut() {
                    span.push(' ');
                } else if prose_depth > 0 {
                    c.prose.push(" ", range.start);
                }
            }
            _ => {}
        }
    }
    c.flush_prose(open.last().copied());
    c.terms.sort_by_key(|t| t.byte);

    DocOutline {
        path: path.to_string(),
        meta,
        sections,
        mentions: c.mentions,
        term_candidates: c.terms,
    }
}

fn is_inline(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::Link { .. }
            | Tag::Image { .. }
    )
}

fn is_inline_end(tag: &TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::Link
            | TagEnd::Image
    )
}

/// Mentions and term candidates gathered during the event walk.
#[derive(Default)]
struct Collector {
    mentions: Vec<Mention>,
    seen: HashSet<(Option<usize>, MentionKind, MentionForm)>,
    terms: Vec<TermCandidate>,
    prose: Prose,
}

impl Collector {
    fn mention(
        &mut self,
        kind: MentionKind,
        form: MentionForm,
        byte: usize,
        section: Option<usize>,
    ) {
        if self.seen.insert((section, kind, form.clone())) {
            self.mentions.push(Mention {
                kind,
                form,
                byte,
                section,
            });
        }
    }

    fn code_mentions(
        &mut self,
        kind: MentionKind,
        code: &str,
        byte: usize,
        section: Option<usize>,
    ) {
        for (offset, token) in code_tokens(code) {
            if let Some(form) = classify_code_token(token) {
                self.mention(kind, form, byte + offset, section);
            }
        }
    }

    fn bold_term(&mut self, span: &str, byte: usize, section: Option<usize>) {
        let trimmed = span.trim_matches(|c: char| !c.is_alphanumeric());
        let byte = byte
            + (span.len()
                - span
                    .trim_start_matches(|c: char| !c.is_alphanumeric())
                    .len());
        let term = collapse_whitespace(trimmed);
        let words = term.split(' ').filter(|w| !w.is_empty()).count();
        if (1..=4).contains(&words) {
            self.terms.push(TermCandidate {
                term,
                byte,
                section,
            });
        }
    }

    /// Turns the buffered paragraph or list-item text into capitalised-run terms.
    fn flush_prose(&mut self, section: Option<usize>) {
        for (pos, term) in capitalised_runs(&self.prose.buf) {
            self.terms.push(TermCandidate {
                term,
                byte: self.prose.source_byte(pos),
                section,
            });
        }
        self.prose = Prose::default();
    }
}

/// Prose text of one block, with a map back to source bytes.
#[derive(Default)]
struct Prose {
    buf: String,
    /// (offset in `buf`, source byte) at the start of each pushed piece.
    pieces: Vec<(usize, usize)>,
}

impl Prose {
    fn push(&mut self, text: &str, source_byte: usize) {
        self.pieces.push((self.buf.len(), source_byte));
        self.buf.push_str(text);
    }

    fn push_break(&mut self) {
        if !self.buf.is_empty() {
            self.buf.push(RUN_BREAK);
        }
    }

    fn source_byte(&self, pos: usize) -> usize {
        let i = self.pieces.partition_point(|&(b, _)| b <= pos).max(1) - 1;
        self.pieces
            .get(i)
            .map_or(0, |&(b, src)| src + pos.saturating_sub(b))
    }
}

/// Runs of 2-4 consecutive capitalised words, after stripping leading stopwords.
/// Words are separated only by whitespace; any punctuation ends a run. A run
/// longer than 4 words (a title or a shouted sentence) is dropped, not split,
/// because any split would be an arbitrary fragment.
fn capitalised_runs(text: &str) -> Vec<(usize, String)> {
    fn flush(run: &mut Vec<(usize, &str)>, out: &mut Vec<(usize, String)>) {
        let skip = run
            .iter()
            .take_while(|(_, w)| TERM_STOPWORDS.contains(w))
            .count();
        let words = &run[skip..];
        if (2..=4).contains(&words.len()) {
            let term = words.iter().map(|(_, w)| *w).collect::<Vec<_>>().join(" ");
            out.push((words[0].0, term));
        }
        run.clear();
    }

    let mut out = Vec::new();
    let mut run: Vec<(usize, &str)> = Vec::new();
    let mut word_start: Option<usize> = None;
    for (i, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if ch.is_alphanumeric() || ch == '-' {
            word_start.get_or_insert(i);
            continue;
        }
        if let Some(start) = word_start.take() {
            let word = &text[start..i];
            if word.starts_with(char::is_uppercase) {
                run.push((start, word));
            } else {
                flush(&mut run, &mut out);
            }
        }
        if !ch.is_whitespace() {
            flush(&mut run, &mut out);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// The path part of a relative Markdown link target, without `#fragment` or
/// `?query`. External (any URL scheme or `//host`) and pure `#anchor` links give `None`.
fn link_target(dest: &str) -> Option<&str> {
    let dest = dest.trim();
    if dest.starts_with("//") || has_url_scheme(dest) {
        return None;
    }
    let path = dest.split(['#', '?']).next().unwrap_or("");
    let path = path.strip_prefix("./").unwrap_or(path);
    (!path.is_empty()).then_some(path)
}

/// `scheme:` per RFC 3986, at least two characters so a Windows drive is not one.
fn has_url_scheme(s: &str) -> bool {
    let Some((scheme, _)) = s.split_once(':') else {
        return false;
    };
    scheme.len() >= 2
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// Maximal runs of identifier and path characters, with their byte offsets.
fn code_tokens(code: &str) -> impl Iterator<Item = (usize, &str)> {
    let is_token_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '-');
    let mut rest = code;
    let mut offset = 0;
    std::iter::from_fn(move || {
        let start = rest.find(is_token_char)?;
        let len = rest[start..]
            .find(|c: char| !is_token_char(c))
            .unwrap_or(rest.len() - start);
        let token = &rest[start..start + len];
        let at = offset + start;
        rest = &rest[start + len..];
        offset = at + len;
        Some((at, token))
    })
}

/// Extensions that make a slash-free token like `graph.rs` a path, not `Type.member`.
const PATH_EXTENSIONS: &[&str] = &[
    "rs", "py", "ts", "tsx", "js", "jsx", "kt", "lua", "php", "tf", "gd", "sql", "vue", "md",
    "yaml", "yml", "json", "toml",
];

/// Classifies one code token into the rule form it is written in, or `None` if it
/// is neither path- nor identifier-like (numbers, URLs, operators, `//` comments).
fn classify_code_token(token: &str) -> Option<MentionForm> {
    if token.contains("//") {
        return None;
    }
    let t = token
        .trim_start_matches([':', '-'])
        .trim_end_matches(['.', ':', '-']);
    let t = t.strip_prefix("./").unwrap_or(t);
    if t.is_empty() {
        return None;
    }

    if let Some((head, tail)) = t.split_once("::") {
        if is_path(head) {
            return tail
                .split("::")
                .all(is_identifier)
                .then(|| MentionForm::PathSymbol {
                    path: head.to_string(),
                    symbol: tail.to_string(),
                });
        }
        return type_member(t.split("::"));
    }
    if let Some((head, tail)) = t.rsplit_once(':') {
        let line = tail.parse().ok().filter(|_| is_path(head))?;
        return Some(MentionForm::PathLine {
            path: head.to_string(),
            line,
        });
    }
    if is_path(t) {
        return Some(MentionForm::Path(t.to_string()));
    }
    if t.contains('.') {
        return type_member(t.split('.'));
    }
    is_identifier(t).then(|| MentionForm::BareName(t.to_string()))
}

/// `a::b::Type::member` or `a.Type.member`: the last two segments, if all are identifiers.
fn type_member<'a>(segments: impl Iterator<Item = &'a str>) -> Option<MentionForm> {
    let segments: Vec<_> = segments.collect();
    if segments.len() < 2 || !segments.iter().all(|s| is_identifier(s)) {
        return None;
    }
    let [owner, member] = segments[segments.len() - 2..] else {
        return None;
    };
    Some(MentionForm::TypeMember {
        owner: owner.to_string(),
        member: member.to_string(),
    })
}

/// Contains a '/' between non-empty segments, or ends in a known source extension.
fn is_path(s: &str) -> bool {
    let segment_ok = |seg: &str| {
        !seg.is_empty()
            && seg
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
    };
    if s.contains('/') {
        return s.split('/').all(segment_ok) && s.chars().any(char::is_alphanumeric);
    }
    match s.rsplit_once('.') {
        Some((stem, ext)) => segment_ok(stem) && PATH_EXTENSIONS.contains(&ext),
        None => false,
    }
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// A YAML block fenced by `---` lines at the very start of the file (after an
/// optional BOM), closed by `---` or `...`; its first line may not be blank.
/// Returns the YAML text and the byte where the body starts. Anywhere else, or
/// unclosed, `---` is an ordinary thematic break.
fn split_frontmatter(text: &str) -> Option<(&str, usize)> {
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let mut lines = text[bom..].split_inclusive('\n');
    let opening = lines.next()?;
    if opening.trim_end() != "---" {
        return None;
    }
    let yaml_start = bom + opening.len();
    let mut pos = yaml_start;
    for (i, line) in lines.enumerate() {
        let trimmed = line.trim_end();
        let closing = trimmed == "---" || trimmed == "...";
        if i == 0 && (closing || trimmed.is_empty()) {
            return None;
        }
        if closing {
            return Some((&text[yaml_start..pos], pos + line.len()));
        }
        pos += line.len();
    }
    None
}

/// Recognised frontmatter keys, or `None` if the YAML is malformed, is not a
/// mapping, or has none of them. Scalars keep the text as written where YAML
/// would retype it (numbers, dates).
fn parse_frontmatter(yaml: &str) -> Option<DocMeta> {
    let Value::Mapping(map) = serde_yaml::from_str::<Value>(yaml).ok()? else {
        return None;
    };
    let scalar = |key: &str| {
        map.get(key)
            .and_then(|v| scalar_as_written(v, raw_scalar(yaml, key)))
    };
    let list = |key: &str| match map.get(key) {
        Some(Value::Sequence(items)) => items
            .iter()
            .filter_map(|v| scalar_as_written(v, None))
            .collect(),
        Some(v) => scalar_as_written(v, raw_scalar(yaml, key))
            .into_iter()
            .collect(),
        None => Vec::new(),
    };
    let meta = DocMeta {
        id: scalar("id"),
        title: scalar("title"),
        status: scalar("status"),
        date: scalar("date"),
        domain: scalar("domain"),
        tags: list("tags"),
        supersedes: list("supersedes"),
    };
    (meta != DocMeta::default()).then_some(meta)
}

fn scalar_as_written(value: &Value, raw: Option<&str>) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(raw.map_or_else(|| n.to_string(), str::to_string)),
        Value::Bool(b) => Some(raw.map_or_else(|| b.to_string(), str::to_string)),
        Value::Tagged(tagged) => scalar_as_written(&tagged.value, None),
        _ => None,
    }
}

/// The unparsed value of a top-level `key: value` line, without a trailing comment.
fn raw_scalar<'a>(yaml: &'a str, key: &str) -> Option<&'a str> {
    yaml.lines().find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix(':')?;
        let value = value.split(" #").next().unwrap_or("").trim();
        (!value.is_empty()).then_some(value)
    })
}

/// GitHub's heading anchor (github-slugger): lowercase, keep letters, digits,
/// '-' and '_', turn each space into '-', drop everything else. Unicode
/// combining marks are dropped too; github-slugger keeps them, but they are rare
/// in headings and std has no mark test.
pub fn github_slug(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' | '_' => Some(c),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .collect()
}

/// Per-file anchor deduplication, matching github-slugger: the first `foo` stays
/// `foo`, later ones become `foo-1`, `foo-2`, skipping any suffix already taken.
#[derive(Default)]
struct Slugger {
    occurrences: HashMap<String, usize>,
}

impl Slugger {
    fn slug(&mut self, text: &str) -> String {
        let original = github_slug(text);
        let mut result = original.clone();
        while self.occurrences.contains_key(&result) {
            let count = self.occurrences.entry(original.clone()).or_default();
            *count += 1;
            result = format!("{original}-{count}");
        }
        self.occurrences.insert(result.clone(), 0);
        result
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

fn line_of(line_starts: &[usize], byte: usize) -> usize {
    line_starts.partition_point(|&s| s <= byte)
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUIDE: &str = include_str!("../../tests/fixtures/docs/docs/guide.md");
    const RETENTION: &str = include_str!("../../tests/fixtures/docs/docs/adr/005-retention.md");
    const MALFORMED: &str = include_str!("../../tests/fixtures/docs/docs/adr/006-malformed.md");
    const IMMUTABLE: &str =
        include_str!("../../tests/fixtures/docs/docs/adr/004-immutable-audit-log.md");
    const LINKS: &str = include_str!("../../tests/fixtures/docs/docs/links.md");

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn summary(outline: &DocOutline) -> Vec<(&str, &str, usize, Option<usize>)> {
        outline
            .sections
            .iter()
            .map(|s| (s.name.as_str(), s.kind(), s.line, s.parent))
            .collect()
    }

    #[test]
    fn test_guide_section_tree() {
        let outline = parse_markdown("docs/guide.md", GUIDE);
        assert_eq!(outline.path, "docs/guide.md");
        assert_eq!(
            summary(&outline),
            vec![
                ("Operator Guide", "h1", 3, None),
                ("Usage", "h2", 7, Some(0)),
                ("Usage", "h2", 11, Some(0)),
                ("Usage 1", "h2", 15, Some(0)),
                ("Usage", "h2", 19, Some(0)),
                ("Skipped Level Target", "h3", 23, Some(4)),
                ("Deep Detail", "h4", 25, Some(5)),
                ("Step 2: Configure AuditLog (fast!) 🚀", "h2", 29, Some(0)),
                ("Überblick & Café", "h2", 41, Some(0)),
                ("Setext Heading Level Two", "h2", 45, Some(0)),
                ("Casting Director Notes", "h1", 50, None),
                ("Skipped From H1", "h3", 54, Some(10)),
            ]
        );
    }

    #[test]
    fn test_section_ranges_nest() {
        let outline = parse_markdown("docs/guide.md", GUIDE);
        let s = &outline.sections;
        for (i, section) in s.iter().enumerate() {
            assert_eq!(
                &GUIDE[section.start_byte..section.start_byte + 1],
                if section.name.starts_with("Setext") {
                    "S"
                } else {
                    "#"
                },
                "section {i} starts at its heading"
            );
            assert!(section.start_byte < section.end_byte);
            if let Some(p) = section.parent {
                assert!(s[p].start_byte < section.start_byte && section.end_byte <= s[p].end_byte);
            }
        }
        // Ends at the next heading of the same or higher level, or EOF.
        assert_eq!(s[0].end_byte, s[10].start_byte);
        assert_eq!(s[1].end_byte, s[2].start_byte);
        assert_eq!(s[4].end_byte, s[7].start_byte);
        assert_eq!(s[5].end_byte, s[7].start_byte);
        assert_eq!(s[6].end_byte, s[7].start_byte);
        assert_eq!(s[9].end_byte, s[10].start_byte);
        assert_eq!(s[10].end_byte, GUIDE.len());
        assert_eq!(s[11].end_byte, GUIDE.len());
    }

    #[test]
    fn test_text_before_first_heading_and_fenced_headings() {
        let outline = parse_markdown("docs/guide.md", GUIDE);
        let preamble = GUIDE.find("Preamble").unwrap();
        assert!(outline.sections[0].start_byte > preamble);
        assert!(outline.sections.iter().all(|s| s.name != "Not A Heading"));

        let none = parse_markdown("plain.md", "Just text.\n\nNo headings here.\n");
        assert!(none.sections.is_empty());
    }

    #[test]
    fn test_frontmatter_excluded_and_offsets_file_relative() {
        let outline = parse_markdown("docs/adr/005-retention.md", RETENTION);
        let names: Vec<_> = outline.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["ADR-005: Retention window", "Decision", "Status"]
        );
        let first = &outline.sections[0];
        assert_eq!(first.line, 13);
        assert_eq!(first.start_byte, RETENTION.find("# ADR-005").unwrap());
        // The later '---' is a thematic break inside "Decision", not a boundary.
        assert_eq!(
            outline.sections[1].end_byte,
            RETENTION.find("## Status").unwrap()
        );

        let yaml_comment = "---\ntitle: x\n# not a heading\n---\n# Real\n";
        let outline = parse_markdown("x.md", yaml_comment);
        assert_eq!(summary(&outline), vec![("Real", "h1", 5, None)]);
        assert_eq!(
            outline.sections[0].start_byte,
            yaml_comment.find("# Real").unwrap()
        );
    }

    #[test]
    fn test_malformed_frontmatter_still_excluded() {
        let outline = parse_markdown("docs/adr/006-malformed.md", MALFORMED);
        assert_eq!(
            summary(&outline),
            vec![("ADR-006: Malformed frontmatter", "h1", 7, None)]
        );
    }

    #[test]
    fn test_guide_anchors() {
        let outline = parse_markdown("docs/guide.md", GUIDE);
        let anchors: Vec<_> = outline.sections.iter().map(|s| s.anchor.as_str()).collect();
        assert_eq!(
            anchors,
            vec![
                "operator-guide",
                "usage",
                "usage-1",
                "usage-1-1",
                "usage-2",
                "skipped-level-target",
                "deep-detail",
                "step-2-configure-auditlog-fast-",
                "überblick--café",
                "setext-heading-level-two",
                "casting-director-notes",
                "skipped-from-h1",
            ]
        );
    }

    #[test]
    fn test_github_slug() {
        assert_eq!(github_slug("Hello, World!"), "hello-world");
        assert_eq!(github_slug("snake_case and-dash"), "snake_case-and-dash");
        assert_eq!(github_slug("🚀 Launch"), "-launch");
        assert_eq!(github_slug("Ärger über Straße 2"), "ärger-über-straße-2");
        assert_eq!(github_slug("日本語 見出し"), "日本語-見出し");
        assert_eq!(github_slug("v1.2 (beta)"), "v12-beta");
    }

    #[test]
    fn test_duplicate_anchors() {
        let mut slugger = Slugger::default();
        let got: Vec<_> = ["Foo", "Foo", "Foo"]
            .iter()
            .map(|t| slugger.slug(t))
            .collect();
        assert_eq!(got, vec!["foo", "foo-1", "foo-2"]);

        // A heading that already looks like a suffix pushes later duplicates on.
        let mut slugger = Slugger::default();
        let got: Vec<_> = ["Foo", "Foo 1", "Foo", "Foo"]
            .iter()
            .map(|t| slugger.slug(t))
            .collect();
        assert_eq!(got, vec!["foo", "foo-1", "foo-2", "foo-3"]);

        // Inline code contributes its text to the slug.
        let outline = parse_markdown("x.md", "## Use `Foo::bar()` now\n## Use Foo::bar() now\n");
        let anchors: Vec<_> = outline.sections.iter().map(|s| s.anchor.as_str()).collect();
        assert_eq!(anchors, vec!["use-foobar-now", "use-foobar-now-1"]);
    }

    #[test]
    fn test_heading_markup_flattened() {
        let outline = parse_markdown("x.md", "## A *very*  **bold** `code` [link](y.md)\n");
        assert_eq!(outline.sections[0].name, "A very bold code link");
    }

    #[test]
    fn test_frontmatter_every_key() {
        let meta = parse_markdown("docs/adr/004.md", IMMUTABLE).meta.unwrap();
        assert_eq!(
            meta,
            DocMeta {
                id: Some("ADR-004".into()),
                title: Some("Audit logs are immutable".into()),
                status: Some("accepted".into()),
                date: Some("2026-10-01".into()),
                domain: Some("audit".into()),
                tags: strings(&["audit", "compliance"]),
                supersedes: strings(&["ADR-001"]),
            }
        );
    }

    #[test]
    fn test_frontmatter_typed_scalars_and_lists() {
        let meta = parse_markdown("docs/adr/005.md", RETENTION).meta.unwrap();
        assert_eq!(meta.id.as_deref(), Some("5"));
        assert_eq!(meta.date.as_deref(), Some("2026-10-02"));
        assert_eq!(meta.tags, strings(&["retention"]));
        assert_eq!(meta.supersedes, strings(&["ADR-002", "ADR-003"]));

        // Numbers keep the text as written, not YAML's reading of it.
        let doc = "---\nid: 007\ndate: 2026.10\ntitle: x # note\n---\n# T\n";
        let meta = parse_markdown("x.md", doc).meta.unwrap();
        assert_eq!(meta.id.as_deref(), Some("007"));
        assert_eq!(meta.date.as_deref(), Some("2026.10"));
        assert_eq!(meta.title.as_deref(), Some("x"));

        let bom = "\u{feff}---\r\ntitle: x\r\n...\r\n# T\r\n";
        let outline = parse_markdown("x.md", bom);
        assert_eq!(outline.meta.as_ref().unwrap().title.as_deref(), Some("x"));
        assert_eq!(summary(&outline), vec![("T", "h1", 4, None)]);

        let doc = "---\ntags:\nsupersedes: [3, ADR-4]\n---\n";
        let meta = parse_markdown("x.md", doc).meta.unwrap();
        assert!(meta.tags.is_empty());
        assert_eq!(meta.supersedes, strings(&["3", "ADR-4"]));
    }

    #[test]
    fn test_frontmatter_absent_malformed_or_unrecognised() {
        assert_eq!(parse_markdown("g.md", GUIDE).meta, None);
        assert_eq!(parse_markdown("m.md", MALFORMED).meta, None);
        assert_eq!(
            parse_markdown("x.md", "---\nowner: me\n---\n# T\n").meta,
            None
        );
        assert_eq!(parse_markdown("x.md", "---\n- a\n- b\n---\n").meta, None);

        // '---' after the first line, or with no closing fence, is a thematic break.
        let later = "# T\n\n---\nIntro\n---\n";
        let outline = parse_markdown("x.md", later);
        assert_eq!(outline.meta, None);
        let names: Vec<_> = outline.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["T", "Intro"],
            "mid-file fences are not swallowed"
        );
        let unclosed = "---\ntitle: x\n\n# T\n";
        let outline = parse_markdown("x.md", unclosed);
        assert_eq!(outline.meta, None);
        assert_eq!(outline.sections.len(), 1);
    }

    /// (kind, form, anchor of the attributed section or "" for file level).
    fn mention_summary(outline: &DocOutline) -> Vec<(MentionKind, MentionForm, &str)> {
        outline
            .mentions
            .iter()
            .map(|m| {
                let anchor = m
                    .section
                    .map_or("", |i| outline.sections[i].anchor.as_str());
                (m.kind, m.form.clone(), anchor)
            })
            .collect()
    }

    fn path(p: &str) -> MentionForm {
        MentionForm::Path(p.into())
    }

    fn bare(name: &str) -> MentionForm {
        MentionForm::BareName(name.into())
    }

    fn member(owner: &str, member: &str) -> MentionForm {
        MentionForm::TypeMember {
            owner: owner.into(),
            member: member.into(),
        }
    }

    #[test]
    fn test_links_fixture_mentions() {
        use MentionKind::*;
        let outline = parse_markdown("docs/links.md", LINKS);
        let got = mention_summary(&outline);
        let (fenced, rest): (Vec<_>, Vec<_>) = got.into_iter().partition(|m| m.0 == FencedCode);
        assert_eq!(
            rest,
            vec![
                (CodeSpan, path("src/audit/log.rs"), ""),
                (MarkdownLink, path("../src/audit/log.rs"), "markdown-links"),
                (MarkdownLink, path("guide.md"), "markdown-links"),
                (
                    MarkdownLink,
                    path("../../../../Cargo.toml"),
                    "markdown-links"
                ),
                (MarkdownLink, path("../src/missing.rs"), "markdown-links"),
                (CodeSpan, path("src/audit/retention.rs"), "paths"),
                (CodeSpan, path("src/billing/invoice.py"), "paths"),
                (CodeSpan, path("src/audit/nothing.rs"), "paths"),
                (
                    CodeSpan,
                    MentionForm::PathSymbol {
                        path: "src/audit/log.rs".into(),
                        symbol: "append".into()
                    },
                    "qualified-symbols"
                ),
                (
                    CodeSpan,
                    MentionForm::PathLine {
                        path: "src/audit/log.rs".into(),
                        line: 12
                    },
                    "qualified-symbols"
                ),
                (
                    CodeSpan,
                    MentionForm::PathLine {
                        path: "src/audit/log.rs".into(),
                        line: 1
                    },
                    "qualified-symbols"
                ),
                (CodeSpan, member("AuditLog", "append"), "qualified-symbols"),
                (CodeSpan, member("AuditLog", "new"), "qualified-symbols"),
                (
                    CodeSpan,
                    member("RetentionPolicy", "apply"),
                    "qualified-symbols"
                ),
                (CodeSpan, bare("rotate_segments"), "bare-names"),
                (CodeSpan, bare("flush_buffer"), "bare-names"),
                (CodeSpan, bare("new"), "bare-names"),
                (CodeSpan, bare("run"), "bare-names"),
                (CodeSpan, bare("does_not_exist"), "bare-names"),
            ]
        );
        // Fenced tokens, deduplicated per section ('log' occurs twice); the
        // '// see' comment marker is not a path.
        let fenced: Vec<_> = fenced.into_iter().map(|(_, form, a)| (form, a)).collect();
        assert_eq!(
            fenced,
            vec![
                (bare("let"), "fenced-code"),
                (bare("mut"), "fenced-code"),
                (bare("log"), "fenced-code"),
                (member("AuditLog", "new"), "fenced-code"),
                (bare("verify_chain"), "fenced-code"),
                (bare("see"), "fenced-code"),
                (path("src/audit/retention.rs"), "fenced-code"),
            ]
        );
    }

    #[test]
    fn test_mentions_attributed_to_deepest_section_only() {
        let outline = parse_markdown("docs/links.md", LINKS);
        let anchors: HashSet<_> = mention_summary(&outline)
            .into_iter()
            .map(|(_, _, a)| a)
            .collect();
        assert!(!anchors.contains("link-coverage"));
        assert!(!anchors.contains("code-spans"));

        // Bytes point into the right section.
        for m in &outline.mentions {
            match m.section {
                Some(i) => {
                    let s = &outline.sections[i];
                    assert!(s.start_byte <= m.byte && m.byte < s.end_byte, "{m:?}");
                }
                None => assert!(m.byte < outline.sections[0].start_byte),
            }
        }
    }

    #[test]
    fn test_link_targets() {
        assert_eq!(link_target("../a.rs#L1"), Some("../a.rs"));
        assert_eq!(link_target("./b.md?plain=1#x"), Some("b.md"));
        assert_eq!(link_target("https://x.org/a.rs"), None);
        assert_eq!(link_target("HTTP://x.org"), None);
        assert_eq!(link_target("mailto:a@b.c"), None);
        assert_eq!(link_target("ftp://x"), None);
        assert_eq!(link_target("//cdn.x/a.js"), None);
        assert_eq!(link_target("#top"), None);
        assert_eq!(link_target(""), None);
        assert_eq!(link_target("C:/x.rs"), Some("C:/x.rs"));

        // Mentions in a heading belong to the section it opens; images are not links.
        let outline = parse_markdown("x.md", "# Use `Graph::load`\n\n![alt](pic.png)\n");
        assert_eq!(
            mention_summary(&outline),
            vec![(
                MentionKind::CodeSpan,
                member("Graph", "load"),
                "use-graphload"
            )]
        );
    }

    #[test]
    fn test_classify_code_token() {
        let c = classify_code_token;
        assert_eq!(c("src/core/graph.rs"), Some(path("src/core/graph.rs")));
        assert_eq!(c("./src/a.rs"), Some(path("src/a.rs")));
        assert_eq!(c("graph.rs"), Some(path("graph.rs")));
        assert_eq!(c("../docs/"), None, "empty segment");
        assert_eq!(
            c("audit/log.rs::Type::method"),
            Some(MentionForm::PathSymbol {
                path: "audit/log.rs".into(),
                symbol: "Type::method".into()
            })
        );
        assert_eq!(
            c("src/core/graph.rs:191:"),
            Some(MentionForm::PathLine {
                path: "src/core/graph.rs".into(),
                line: 191
            })
        );
        assert_eq!(c("src/a.rs:x"), None);
        assert_eq!(c("crate::core::Graph::load"), Some(member("Graph", "load")));
        assert_eq!(c("self.entries.clear"), Some(member("entries", "clear")));
        assert_eq!(
            c("Graph.load."),
            Some(member("Graph", "load")),
            "sentence dot"
        );
        assert_eq!(c("load"), Some(bare("load")));
        assert_eq!(c("--check"), Some(bare("check")));
        assert_eq!(c("0.13.5"), None);
        assert_eq!(c("v0.2.0"), None);
        assert_eq!(c("42"), None);
        assert_eq!(c("//"), None);
        assert_eq!(c("https://x.org/a.rs"), None);
        assert_eq!(c("foo-bar"), None);

        let tokens: Vec<_> = code_tokens("let x = a::b(c.d, \"src/e.rs\");").collect();
        assert_eq!(
            tokens,
            vec![
                (0, "let"),
                (4, "x"),
                (8, "a::b"),
                (13, "c.d"),
                (19, "src/e.rs")
            ]
        );
    }

    fn term_summary(outline: &DocOutline) -> Vec<(&str, &str)> {
        outline
            .term_candidates
            .iter()
            .map(|t| {
                let anchor = t
                    .section
                    .map_or("", |i| outline.sections[i].anchor.as_str());
                (t.term.as_str(), anchor)
            })
            .collect()
    }

    fn terms_of(text: &str) -> Vec<String> {
        parse_markdown("x.md", text)
            .term_candidates
            .into_iter()
            .map(|t| t.term)
            .collect()
    }

    #[test]
    fn test_guide_term_candidates() {
        let outline = parse_markdown("docs/guide.md", GUIDE);
        assert_eq!(
            term_summary(&outline),
            vec![
                ("Casting Director", ""),
                ("Casting Director", "operator-guide"),
                ("Audit Service", "operator-guide"),
                ("ledger", "operator-guide"),
                ("Compliance Officer", "usage"),
                ("ledger", "usage"),
                ("Compliance Officer", "deep-detail"),
                ("Audit Service", "step-2-configure-auditlog-fast-"),
                ("Release Manager", "step-2-configure-auditlog-fast-"),
            ]
        );
        for t in &outline.term_candidates {
            assert!(GUIDE[t.byte..].starts_with(&t.term), "{t:?}");
        }
    }

    #[test]
    fn test_term_runs_and_stopwords() {
        assert_eq!(
            terms_of("When The Release Manager ships.\n"),
            strings(&["Release Manager"])
        );
        assert_eq!(terms_of("This Week only.\n"), Vec::<String>::new());
        assert_eq!(
            terms_of("Use Audit Log Retention Service now.\n"),
            strings(&["Audit Log Retention Service"]),
            "4 words after stripping is kept"
        );
        assert_eq!(
            terms_of("A Very Long Capitalised Phrase Goes Here twice.\n"),
            Vec::<String>::new(),
            "runs over 4 words are dropped, not split"
        );
        assert_eq!(
            terms_of("Audit Service, Casting Director.\n"),
            strings(&["Audit Service", "Casting Director"]),
            "punctuation ends a run"
        );
        assert_eq!(
            terms_of("The Compliance Officer's review.\n"),
            strings(&["Compliance Officer"])
        );
        assert_eq!(
            terms_of("Audit\nService here.\n"),
            strings(&["Audit Service"])
        );
    }

    #[test]
    fn test_term_sources() {
        // Bold: 1-4 words, edge punctuation trimmed, and not part of a run.
        assert_eq!(terms_of("A **ledger.** entry.\n"), strings(&["ledger"]));
        let outline = parse_markdown("x.md", "See **(Audit Desk)** now.\n");
        assert_eq!(
            outline.term_candidates[0].byte, 7,
            "byte of the term itself"
        );
        assert_eq!(
            terms_of("**Audit Service** Team.\n"),
            strings(&["Audit Service"])
        );
        assert_eq!(
            terms_of("**one two three four five** words.\n"),
            Vec::<String>::new()
        );
        // List items, tight and loose, and block quotes are prose.
        assert_eq!(
            terms_of("- Casting Director\n- x\n\n  Audit Service\n\n> Release Manager\n"),
            strings(&["Casting Director", "Audit Service", "Release Manager"])
        );
        // Link text counts, URLs do not.
        assert_eq!(
            terms_of("[Audit Service](https://Casting.Director/Release-Manager)\n"),
            strings(&["Audit Service"])
        );
        // Headings, code, image alt text and frontmatter do not.
        assert_eq!(
            terms_of(
                "---\ntitle: Audit Service\n---\n# Casting Director\n\n\
                 Use `Audit Service` and ![Release Manager](x.png) here.\n\n\
                 ```\nCompliance Officer\n```\n\n    Compliance Officer\n"
            ),
            Vec::<String>::new()
        );
        // A code span breaks a run.
        assert_eq!(terms_of("Audit `x` Service.\n"), Vec::<String>::new());
    }
}
