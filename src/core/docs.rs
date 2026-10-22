use crate::core::graph::{DocMeta, DocTerm};
use petgraph::graph::NodeIndex;
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use serde_yaml::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;

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
    /// Membership test only: mentions are kept in `mentions`, in document order.
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
    /// Lookup-only.
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

/// Files larger than this are only OpenAPI if their name says so; they are never sniffed.
pub const OPENAPI_SNIFF_MAX_SIZE: u64 = 1024 * 1024;
/// How much of a file the content sniff looks at.
pub const OPENAPI_HEAD_LEN: usize = 1024;

const HTTP_METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// Operations and schemas of one OpenAPI 3 or Swagger 2 document, in document order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApiOutline {
    pub operations: Vec<ApiOperation>,
    pub schemas: Vec<ApiSchema>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiOperation {
    /// Upper case, e.g. `GET`.
    pub method: String,
    pub path: String,
    pub operation_id: Option<String>,
    pub summary: Option<String>,
    pub location: ApiLocation,
}

impl ApiOperation {
    /// `METHOD /path`, the node name.
    pub fn name(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiSchema {
    pub name: String,
    pub location: ApiLocation,
}

/// Where a method or schema key is written: its 1-based line, and bytes from the
/// key to the end of that line. Values carry no spans, so keys are found by text
/// search; a key that cannot be found keeps the file start (line 1, bytes 0..0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiLocation {
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

impl Default for ApiLocation {
    fn default() -> Self {
        Self {
            line: 1,
            start_byte: 0,
            end_byte: 0,
        }
    }
}

/// `yaml`, `yml` or `json`, the only extensions an OpenAPI spec can have here.
fn api_extension(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "yaml" => Some("yaml"),
        "yml" => Some("yml"),
        "json" => Some("json"),
        _ => None,
    }
}

/// True if `path` is named `openapi.{yaml,yml,json}` or `swagger.{yaml,yml,json}`,
/// or is a YAML/JSON file of at most 1 MB whose first 1 KB (`head`, after a BOM)
/// has a top-level `openapi` or `swagger` key. `head` is ignored for larger files.
pub fn is_openapi(path: &Path, head: &[u8], size: u64) -> bool {
    let Some(ext) = api_extension(path) else {
        return false;
    };
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name == "openapi" || name == "swagger" {
        return true;
    }
    if size > OPENAPI_SNIFF_MAX_SIZE {
        return false;
    }
    let head = &head[..head.len().min(OPENAPI_HEAD_LEN)];
    let head = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
    let is_flow = head.trim_ascii_start().starts_with(b"{");
    if ext == "json" || is_flow {
        json_has_top_level_key(head)
    } else {
        yaml_has_top_level_key(head)
    }
}

/// `is_openapi` for a file on disk, reading at most `OPENAPI_HEAD_LEN` bytes.
pub fn is_openapi_file(path: &Path) -> bool {
    if api_extension(path).is_none() {
        return false;
    }
    let Ok(size) = fs::metadata(path).map(|m| m.len()) else {
        return false;
    };
    let mut head = Vec::with_capacity(OPENAPI_HEAD_LEN);
    if size <= OPENAPI_SNIFF_MAX_SIZE {
        let read = fs::File::open(path)
            .and_then(|f| f.take(OPENAPI_HEAD_LEN as u64).read_to_end(&mut head));
        if read.is_err() {
            return false;
        }
    }
    is_openapi(path, &head, size)
}

fn is_api_key(key: &[u8]) -> bool {
    key == b"openapi" || key == b"swagger"
}

/// A line starting at column 0 with `openapi:` or `swagger:`, the key optionally
/// quoted. Comments and indented (nested) keys never match.
fn yaml_has_top_level_key(head: &[u8]) -> bool {
    head.split(|&b| b == b'\n').any(|line| {
        let (key, rest) = match line.first() {
            Some(&q @ (b'"' | b'\'')) => match line[1..].iter().position(|&b| b == q) {
                Some(end) => (&line[1..end + 1], &line[end + 2..]),
                None => return false,
            },
            _ => {
                let end = line
                    .iter()
                    .position(|&b| b == b':' || b == b' ')
                    .unwrap_or(line.len());
                (&line[..end], &line[end..])
            }
        };
        is_api_key(key) && rest.trim_ascii_start().starts_with(b":")
    })
}

/// A key of the outermost JSON object named `openapi` or `swagger`; strings are
/// skipped with their escapes so braces inside them do not change the depth.
fn json_has_top_level_key(head: &[u8]) -> bool {
    let mut depth = 0usize;
    let mut i = 0;
    while i < head.len() {
        match head[i] {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth = depth.saturating_sub(1),
            b'"' => {
                let start = i + 1;
                let mut j = start;
                while j < head.len() && head[j] != b'"' {
                    j += if head[j] == b'\\' { 2 } else { 1 };
                }
                if j >= head.len() {
                    return false;
                }
                if depth == 1
                    && is_api_key(&head[start..j])
                    && head[j + 1..].trim_ascii_start().starts_with(b":")
                {
                    return true;
                }
                i = j;
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Operations and schemas of a parsed OpenAPI 3 / Swagger 2 document, or `None`
/// unless it is a mapping with a top-level `openapi` or `swagger` key. Locations
/// are left at the file start; `parse_openapi_text` fills them in.
pub fn parse_openapi(value: &Value) -> Option<ApiOutline> {
    let Value::Mapping(doc) = value else {
        return None;
    };
    if !doc.contains_key("openapi") && !doc.contains_key("swagger") {
        return None;
    }
    let text = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);

    let mut outline = ApiOutline::default();
    if let Some(Value::Mapping(paths)) = doc.get("paths") {
        for (path, item) in paths {
            let (Some(path), Value::Mapping(item)) = (path.as_str(), item) else {
                continue;
            };
            for (method, op) in item {
                let Some(method) = method.as_str().filter(|m| HTTP_METHODS.contains(m)) else {
                    continue;
                };
                outline.operations.push(ApiOperation {
                    method: method.to_ascii_uppercase(),
                    path: path.to_string(),
                    operation_id: text(op.get("operationId")),
                    summary: text(op.get("summary")),
                    location: ApiLocation::default(),
                });
            }
        }
    }
    let schema_tables = [
        doc.get("components").and_then(|c| c.get("schemas")),
        doc.get("definitions"),
    ];
    for table in schema_tables.into_iter().flatten() {
        if let Value::Mapping(schemas) = table {
            outline
                .schemas
                .extend(
                    schemas
                        .keys()
                        .filter_map(Value::as_str)
                        .map(|name| ApiSchema {
                            name: name.to_string(),
                            location: ApiLocation::default(),
                        }),
                );
        }
    }
    Some(outline)
}

/// Parses YAML or JSON (by extension) and extracts the API outline with locations.
pub fn parse_openapi_text(path: &Path, text: &str) -> Option<ApiOutline> {
    let body = text.trim_start_matches('\u{feff}');
    let value: Value = if api_extension(path)? == "json" {
        serde_json::from_str(body).ok()?
    } else {
        serde_yaml::from_str(body).ok()?
    };
    let mut outline = parse_openapi(&value)?;
    locate_api_keys(&mut outline, text);
    Some(outline)
}

/// Finds each method key after its path key inside `paths`, and each schema key
/// after `schemas` / `definitions`.
fn locate_api_keys(outline: &mut ApiOutline, text: &str) {
    let line_starts = line_starts(text);
    let locate = |at: usize| ApiLocation {
        line: line_of(&line_starts, at),
        start_byte: at,
        end_byte: text[at..].find('\n').map_or(text.len(), |n| at + n),
    };
    let paths_at = find_key(text, 0, "paths").unwrap_or(0);
    for op in &mut outline.operations {
        let method = op.method.to_ascii_lowercase();
        if let Some(at) =
            find_key(text, paths_at, &op.path).and_then(|path_at| find_key(text, path_at, &method))
        {
            op.location = locate(at);
        }
    }
    let tables = [
        find_key(text, 0, "schemas"),
        find_key(text, 0, "definitions"),
    ];
    for schema in &mut outline.schemas {
        if let Some(at) = tables
            .iter()
            .flatten()
            .find_map(|&from| find_key(text, from, &schema.name))
        {
            schema.location = locate(at);
        }
    }
}

/// Byte of the first `key:` / `"key":` / `'key':` at or after `from` that is a
/// whole key: preceded by whitespace, `{`, `,` or the start, and followed by ':'.
fn find_key(text: &str, from: usize, key: &str) -> Option<usize> {
    let mut search = from;
    while let Some(rel) = text.get(search..)?.find(key) {
        let at = search + rel;
        search = at + key.len();
        let quote = text[..at]
            .chars()
            .next_back()
            .filter(|c| matches!(c, '"' | '\''));
        let key_start = at - quote.map_or(0, char::len_utf8);
        let boundary = text[..key_start]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || matches!(c, '{' | ','));
        let rest = &text[search..];
        let rest = match quote {
            Some(q) => match rest.strip_prefix(q) {
                Some(r) => r,
                None => continue,
            },
            None => rest,
        };
        if boundary && rest.trim_start_matches([' ', '\t']).starts_with(':') {
            return Some(key_start);
        }
    }
    None
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

/// A term must occur at least this often in a directory to be kept.
pub const TERM_MIN_COUNT: u32 = 2;
/// Terms kept per directory.
pub const TERMS_PER_DIR: usize = 50;
/// Sections kept per term.
pub const SECTIONS_PER_TERM: usize = 5;

/// Aggregates term candidates per directory (the doc file's parent, `.` for the
/// root). Terms occurring at least twice are kept, ordered by count descending
/// then term ascending, at most 50 per directory; each keeps at most 5 distinct
/// `(path, anchor)` sections, the first by path then byte offset. A term before the
/// first heading has the empty anchor. The result does not depend on input order.
pub fn aggregate_terms<'a>(
    docs: impl IntoIterator<Item = (&'a str, &'a DocOutline)>,
) -> BTreeMap<String, Vec<DocTerm>> {
    /// (count, [(path, byte, anchor)])
    type Seen<'a> = (u32, Vec<(&'a str, usize, &'a str)>);
    let mut dirs: BTreeMap<&str, BTreeMap<&str, Seen>> = BTreeMap::new();
    for (path, outline) in docs {
        let dir = path.rsplit_once('/').map_or(".", |(d, _)| d);
        let terms = dirs.entry(dir).or_default();
        for cand in &outline.term_candidates {
            let anchor = cand
                .section
                .map_or("", |i| outline.sections[i].anchor.as_str());
            let entry = terms.entry(cand.term.as_str()).or_default();
            entry.0 += 1;
            entry.1.push((path, cand.byte, anchor));
        }
    }

    let mut out = BTreeMap::new();
    for (dir, terms) in dirs {
        let mut kept: Vec<_> = terms
            .into_iter()
            .filter(|(_, (count, _))| *count >= TERM_MIN_COUNT)
            .collect();
        kept.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0.cmp(b.0)));
        kept.truncate(TERMS_PER_DIR);
        if kept.is_empty() {
            continue;
        }
        let list = kept
            .into_iter()
            .map(|(term, (count, mut at))| {
                at.sort_unstable_by_key(|&(path, byte, _)| (path, byte));
                let mut seen = HashSet::new();
                let sections = at
                    .into_iter()
                    .filter(|&(path, _, anchor)| seen.insert((path, anchor)))
                    .take(SECTIONS_PER_TERM)
                    .map(|(path, _, anchor)| (path.to_string(), anchor.to_string()))
                    .collect();
                DocTerm {
                    term: term.to_string(),
                    count,
                    sections,
                }
            })
            .collect();
        out.insert(dir.to_string(), list);
    }
    out
}

/// Bare names that are never linked: too generic to point at one symbol.
/// Names under `BARE_NAME_MIN_LEN` characters are never linked either.
const BARE_NAME_DENYLIST: &[&str] = &[
    "new", "main", "default", "init", "run", "test", "self", "build", "get", "set", "args",
    "clone", "close", "config", "create", "data", "delete", "drop", "error", "exit", "format",
    "from", "handle", "help", "index", "into", "item", "items", "list", "name", "next", "none",
    "open", "parse", "path", "process", "read", "result", "setup", "size", "some", "start",
    "state", "stop", "this", "type", "update", "value", "write",
];
const BARE_NAME_MIN_LEN: usize = 4;

/// How a mention resolved against the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The file or symbol nodes to draw `References` edges to.
    Linked(Vec<NodeIndex>),
    /// Nothing matched, or the name is too generic to try.
    Unresolved,
    /// More than one candidate; no guess is made.
    Ambiguous,
}

/// A code symbol a mention can resolve to.
#[derive(Debug, Clone)]
pub struct SymbolRef {
    pub node: NodeIndex,
    pub name: String,
    /// 1-based start line.
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    /// A Rust `impl` block, indexed under its type's name. It locates members but
    /// is not a second definition of the type.
    pub is_impl: bool,
}

/// Lookup tables for the link pass, built once per build.
///
/// Paths are as indexed, which keeps the build root as a prefix when it was given
/// relative. Mentions are resolved in root-relative form (doc code paths are
/// written relative to the index root, Markdown links relative to the document)
/// and the prefix is put back for the lookup.
#[derive(Debug, Default)]
pub struct ResolveTables {
    // The maps below are lookup-only; the candidate lists inside them keep insertion order.
    prefix: String,
    files: HashMap<String, NodeIndex>,
    file_symbols: HashMap<String, Vec<SymbolRef>>,
    /// name -> (file path, index into `file_symbols[path]`)
    by_name: HashMap<String, Vec<(String, usize)>>,
    line_starts: HashMap<String, Vec<usize>>,
}

impl ResolveTables {
    /// `prefix` is the indexed form of the build root ("" when paths are root-relative).
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.trim_end_matches('/').to_string(),
            ..Self::default()
        }
    }

    pub fn add_file(&mut self, path: &str, node: NodeIndex) {
        self.files.insert(path.to_string(), node);
    }

    pub fn add_symbol(&mut self, path: &str, symbol: SymbolRef) {
        let list = self.file_symbols.entry(path.to_string()).or_default();
        self.by_name
            .entry(symbol.name.clone())
            .or_default()
            .push((path.to_string(), list.len()));
        list.push(symbol);
    }

    /// Line starts of an indexed file, needed to resolve `path:line` mentions
    /// (symbols carry a start line and a byte range, not an end line).
    pub fn set_line_text(&mut self, path: &str, text: &str) {
        self.line_starts.insert(path.to_string(), line_starts(text));
    }

    /// The indexed file a `path:line` mention points at, if it is one.
    pub fn line_file(&self, form: &MentionForm) -> Option<&str> {
        let MentionForm::PathLine { path, .. } = form else {
            return None;
        };
        let indexed = self.code_path(path)?;
        self.files.get_key_value(&indexed).map(|(k, _)| k.as_str())
    }

    /// Root-relative form of an indexed path.
    fn to_rel<'a>(&self, indexed: &'a str) -> Option<&'a str> {
        if self.prefix.is_empty() {
            return Some(indexed);
        }
        indexed.strip_prefix(&self.prefix)?.strip_prefix('/')
    }

    fn to_indexed(&self, rel: &str) -> String {
        if self.prefix.is_empty() {
            rel.to_string()
        } else {
            format!("{}/{}", self.prefix, rel)
        }
    }

    /// Indexed form of a path written relative to the index root.
    fn code_path(&self, path: &str) -> Option<String> {
        Some(self.to_indexed(&join_rel("", path)?))
    }

    /// Indexed form of a Markdown link target, relative to the document.
    fn link_path(&self, doc_path: &str, target: &str) -> Option<String> {
        let doc_rel = self.to_rel(doc_path)?;
        let dir = doc_rel.rsplit_once('/').map_or("", |(d, _)| d);
        Some(self.to_indexed(&join_rel(dir, target)?))
    }

    fn file(&self, indexed: Option<String>) -> Option<(String, NodeIndex)> {
        let indexed = indexed?;
        let node = *self.files.get(&indexed)?;
        Some((indexed, node))
    }

    /// Symbols named `name` in one file. Impl blocks only count if nothing else
    /// has the name.
    fn named_in_file(&self, path: &str, name: &str) -> Vec<&SymbolRef> {
        let all: Vec<_> = self
            .file_symbols
            .get(path)
            .into_iter()
            .flatten()
            .filter(|s| s.name == name)
            .collect();
        prefer_definitions(all)
    }

    /// Members named `member` whose range lies inside a symbol named `owner` in
    /// the same file, in `only` if given, else anywhere.
    fn members(&self, only: Option<&str>, owner: &str, member: &str) -> Vec<NodeIndex> {
        let Some(candidates) = self.by_name.get(member) else {
            return Vec::new();
        };
        let owners = self.by_name.get(owner).map(Vec::as_slice).unwrap_or(&[]);
        let mut found = Vec::new();
        for (path, i) in candidates {
            if only.is_some_and(|p| p != path) {
                continue;
            }
            let c = &self.file_symbols[path][*i];
            let inside = owners.iter().any(|(owner_path, j)| {
                let o = &self.file_symbols[owner_path][*j];
                owner_path == path
                    && o.node != c.node
                    && o.start_byte <= c.start_byte
                    && c.end_byte <= o.end_byte
            });
            if inside && !found.contains(&c.node) {
                found.push(c.node);
            }
        }
        found
    }
}

/// Keeps non-`impl` symbols if there are any (a struct and its impl block share a name).
fn prefer_definitions(all: Vec<&SymbolRef>) -> Vec<&SymbolRef> {
    if all.iter().any(|s| !s.is_impl) {
        all.into_iter().filter(|s| !s.is_impl).collect()
    } else {
        all
    }
}

/// `base` joined with `rel`, with `.` and `..` resolved lexically. `None` if the
/// result leaves the root. A leading `/` in `rel` means root-relative.
fn join_rel(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let base = if rel.starts_with('/') { "" } else { base };
    for part in base.split('/').chain(rel.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

fn one_or_ambiguous(nodes: Vec<NodeIndex>) -> Resolution {
    match nodes.len() {
        0 => Resolution::Unresolved,
        1 => Resolution::Linked(nodes),
        _ => Resolution::Ambiguous,
    }
}

/// Resolves one mention. The mention's form selects the rule, so the first
/// matching rule always decides:
/// 1. Markdown link, 2. path, 3. `path::symbol` and `path:line`,
/// 4. `Type::member`, 5. bare name.
pub fn resolve_mention(mention: &Mention, doc_path: &str, tables: &ResolveTables) -> Resolution {
    match (mention.kind, &mention.form) {
        (MentionKind::MarkdownLink, MentionForm::Path(target)) => {
            link_file(tables.file(tables.link_path(doc_path, target)))
        }
        (MentionKind::MarkdownLink, _) => Resolution::Unresolved,
        (_, MentionForm::Path(path)) => link_file(tables.file(tables.code_path(path))),
        (_, MentionForm::PathSymbol { path, symbol }) => {
            let Some((file, _)) = tables.file(tables.code_path(path)) else {
                return Resolution::Unresolved;
            };
            match symbol.rsplit_once("::") {
                Some((owner, member)) => {
                    let owner = owner.rsplit("::").next().unwrap_or(owner);
                    one_or_ambiguous(tables.members(Some(&file), owner, member))
                }
                None => one_or_ambiguous(
                    tables
                        .named_in_file(&file, symbol)
                        .iter()
                        .map(|s| s.node)
                        .collect(),
                ),
            }
        }
        (_, MentionForm::PathLine { path, line }) => {
            let Some((file, file_node)) = tables.file(tables.code_path(path)) else {
                return Resolution::Unresolved;
            };
            Resolution::Linked(vec![innermost(tables, &file, *line).unwrap_or(file_node)])
        }
        (_, MentionForm::TypeMember { owner, member }) => {
            one_or_ambiguous(tables.members(None, owner, member))
        }
        (_, MentionForm::BareName(name)) => {
            if name.chars().count() < BARE_NAME_MIN_LEN
                || BARE_NAME_DENYLIST.contains(&name.as_str())
            {
                return Resolution::Unresolved;
            }
            let all: Vec<_> = tables
                .by_name
                .get(name)
                .into_iter()
                .flatten()
                .map(|(path, i)| &tables.file_symbols[path][*i])
                .collect();
            one_or_ambiguous(prefer_definitions(all).iter().map(|s| s.node).collect())
        }
    }
}

fn link_file(file: Option<(String, NodeIndex)>) -> Resolution {
    file.map_or(Resolution::Unresolved, |(_, node)| {
        Resolution::Linked(vec![node])
    })
}

/// The smallest symbol of `file` whose lines contain `line`.
fn innermost(tables: &ResolveTables, file: &str, line: usize) -> Option<NodeIndex> {
    let starts = tables.line_starts.get(file)?;
    tables
        .file_symbols
        .get(file)?
        .iter()
        .filter(|s| {
            let last = s.end_byte.saturating_sub(1).max(s.start_byte);
            s.line <= line && line <= line_of(starts, last)
        })
        .min_by_key(|s| s.end_byte - s.start_byte)
        .map(|s| s.node)
}

/// Whether an unlinked mention goes into the `status` counts. Bare names in
/// fenced code that match nothing are not counted: fenced blocks are mostly
/// shell, config and example identifiers, and counting them would bury the
/// unresolved links and paths. An ambiguous fenced name is a real collision
/// and still counts.
pub fn counts_as_unmatched(mention: &Mention, resolution: &Resolution) -> bool {
    match resolution {
        Resolution::Linked(_) => false,
        Resolution::Ambiguous => true,
        Resolution::Unresolved => {
            !(mention.kind == MentionKind::FencedCode
                && matches!(mention.form, MentionForm::BareName(_)))
        }
    }
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

    const API_DIR: &str = "tests/fixtures/docs/api";

    fn fixture(rel: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    #[test]
    fn test_openapi_detection_fixtures() {
        for name in [
            "openapi.yaml",
            "swagger.json",
            "billing-api.yml",
            "empty-openapi.json",
        ] {
            assert!(
                is_openapi_file(&fixture(&format!("{API_DIR}/{name}"))),
                "{name}"
            );
        }
        assert!(!is_openapi_file(&fixture(
            "tests/fixtures/docs/config/settings.yaml"
        )));
        assert!(!is_openapi_file(&fixture(
            "tests/fixtures/docs/docs/guide.md"
        )));
        assert!(!is_openapi_file(&fixture(
            "tests/fixtures/docs/missing.yaml"
        )));
    }

    #[test]
    fn test_openapi_detection_by_name_and_content() {
        let p = Path::new;
        // By name, whatever the content; case-insensitive; only YAML/JSON.
        assert!(is_openapi(p("a/OpenAPI.YML"), b"", 0));
        assert!(is_openapi(
            p("swagger.yaml"),
            b"",
            10 * OPENAPI_SNIFF_MAX_SIZE
        ));
        assert!(!is_openapi(p("openapi.md"), b"openapi: 3", 10));
        assert!(!is_openapi(p("my-openapi.yaml"), b"x: 1", 4));

        let yaml = |head: &str| is_openapi(p("api.yaml"), head.as_bytes(), head.len() as u64);
        assert!(yaml("# c\n\nopenapi: 3.0.0\n"));
        assert!(yaml("\u{feff}swagger: '2.0'\n"));
        assert!(yaml("\"openapi\": 3.1.0\n"));
        assert!(yaml("info:\n  title: x\nopenapi : 3\n"));
        assert!(!yaml("service:\n  openapi: 3.0.0\n"), "nested key");
        assert!(!yaml("# openapi: 3.0.0\n"), "comment");
        assert!(!yaml("openapi_version: 3\n"));
        assert!(!yaml("openapi\n"));
        assert!(
            yaml("{ \"openapi\": \"3.0.0\" }"),
            "flow mapping in a .yaml file"
        );

        let json = |head: &str| is_openapi(p("api.json"), head.as_bytes(), head.len() as u64);
        assert!(json("{\"info\": {\"t\": \"{\"}, \"openapi\": \"3\"}"));
        assert!(json("\u{feff}  {\n  \"swagger\" : \"2.0\"}"));
        assert!(!json("{\"info\": {\"openapi\": \"3\"}}"), "nested key");
        assert!(!json("[\"openapi\", \"swagger\"]"), "values, not keys");
        assert!(!json("{\"name\": \"openapi\"}"));
        assert!(!json("{\"x\": \"a \\\" openapi\": 1}"), "escaped quote");

        // Only the first 1 KB counts.
        let late = format!("{}openapi: 3\n", "# pad\n".repeat(200));
        assert!(!yaml(&late));
    }

    #[test]
    fn test_openapi_detection_skips_large_files() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.json");
        let body = format!(
            "{{\"openapi\": \"3.0.0\", \"pad\": \"{}\"}}",
            "x".repeat(OPENAPI_SNIFF_MAX_SIZE as usize)
        );
        fs::write(&big, &body).unwrap();
        let size = fs::metadata(&big).unwrap().len();
        assert!(size > OPENAPI_SNIFF_MAX_SIZE);

        // The head alone would match; the size gate says no.
        let head = &body.as_bytes()[..OPENAPI_HEAD_LEN];
        assert!(is_openapi(
            Path::new("small.json"),
            head,
            OPENAPI_HEAD_LEN as u64
        ));
        assert!(!is_openapi(&big, head, size));
        assert!(!is_openapi_file(&big));

        // Named specs are detected at any size.
        let named = dir.path().join("openapi.json");
        fs::rename(&big, &named).unwrap();
        assert!(is_openapi_file(&named));
    }

    /// (name, operationId, summary, line) per operation; (name, line) per schema.
    type OpRow = (String, Option<String>, Option<String>, usize);

    fn api_summary(rel: &str) -> (Vec<OpRow>, Vec<(String, usize)>) {
        let path = fixture(&format!("{API_DIR}/{rel}"));
        let text = fs::read_to_string(&path).unwrap();
        let outline = parse_openapi_text(&path, &text).unwrap();
        for op in &outline.operations {
            let key = &text[op.location.start_byte..op.location.end_byte];
            assert!(
                key.trim_start_matches('"')
                    .starts_with(&op.method.to_ascii_lowercase()),
                "{op:?} located at {key:?}"
            );
        }
        (
            outline
                .operations
                .iter()
                .map(|o| {
                    (
                        o.name(),
                        o.operation_id.clone(),
                        o.summary.clone(),
                        o.location.line,
                    )
                })
                .collect(),
            outline
                .schemas
                .iter()
                .map(|s| (s.name.clone(), s.location.line))
                .collect(),
        )
    }

    fn op(name: &str, id: Option<&str>, summary: Option<&str>, line: usize) -> OpRow {
        (
            name.into(),
            id.map(Into::into),
            summary.map(Into::into),
            line,
        )
    }

    #[test]
    fn test_openapi3_yaml_extraction() {
        let (ops, schemas) = api_summary("openapi.yaml");
        assert_eq!(
            ops,
            vec![
                op(
                    "GET /entries",
                    Some("listEntries"),
                    Some("List audit entries"),
                    10
                ),
                op(
                    "POST /entries",
                    Some("appendEntry"),
                    Some("Append an audit entry"),
                    13
                ),
                op("GET /entries/{id}", Some("getEntry"), None, 17),
            ],
            "parameters and $ref are not operations"
        );
        assert_eq!(
            schemas,
            vec![("AuditEntry".into(), 22), ("AuditPage".into(), 24)]
        );

        let (ops, schemas) = api_summary("billing-api.yml");
        assert_eq!(
            ops,
            vec![
                op(
                    "PATCH /retention",
                    Some("setRetention"),
                    Some("Change the retention window"),
                    9
                ),
                op("TRACE /retention", None, Some("Trace retention"), 12),
            ]
        );
        assert!(schemas.is_empty());
    }

    #[test]
    fn test_swagger2_json_extraction() {
        let (ops, schemas) = api_summary("swagger.json");
        assert_eq!(
            ops,
            vec![
                op(
                    "GET /invoices",
                    Some("listInvoices"),
                    Some("List invoices"),
                    6
                ),
                op("PUT /invoices", None, Some("Replace invoices"), 7),
                op("DELETE /invoices/{id}", Some("deleteInvoice"), None, 10),
            ]
        );
        assert_eq!(schemas, vec![("Invoice".into(), 14)]);

        let (ops, schemas) = api_summary("empty-openapi.json");
        assert!(ops.is_empty() && schemas.is_empty());
    }

    #[test]
    fn test_parse_openapi_rejects_non_specs() {
        let yaml = |s: &str| serde_yaml::from_str::<Value>(s).unwrap();
        assert_eq!(parse_openapi(&yaml("service:\n  openapi: 3\n")), None);
        assert_eq!(parse_openapi(&yaml("- openapi\n")), None);
        assert_eq!(
            parse_openapi_text(Path::new("openapi.yaml"), "openapi: [unclosed"),
            None
        );
        assert_eq!(
            parse_openapi_text(Path::new("openapi.json"), "{\"openapi\": "),
            None
        );
        assert_eq!(
            parse_openapi_text(Path::new("openapi.toml"), "openapi = 3"),
            None
        );

        // Upper-case or unknown method keys and non-mapping path items are skipped;
        // an unlocatable key keeps the file start.
        let outline = parse_openapi(&yaml(
            "openapi: 3\npaths:\n  /a:\n    GET: {}\n    servers: []\n    get: {}\n  /b: []\n",
        ))
        .unwrap();
        let names: Vec<_> = outline.operations.iter().map(ApiOperation::name).collect();
        assert_eq!(names, vec!["GET /a"]);
        assert_eq!(outline.operations[0].location, ApiLocation::default());
    }

    #[test]
    fn test_find_key() {
        let text = "target: 1\nx:\n  get : 2\n  \"get\": 3\n";
        assert_eq!(find_key(text, 0, "get"), Some(text.find("get :").unwrap()));
        let after = text.find("get :").unwrap() + 1;
        assert_eq!(
            find_key(text, after, "get"),
            Some(text.find("\"get\"").unwrap())
        );
        assert_eq!(find_key("/a/b: 1\n/a: 2\n", 0, "/a"), Some(8));
        assert_eq!(find_key("x: get\n", 0, "get"), None);
    }

    // ---- Link resolution (4.2, 4.3) ----

    const FIXTURE_PREFIX: &str = "tests/fixtures/docs";
    const LINKS_DOC: &str = "tests/fixtures/docs/docs/links.md";

    /// Tables over the docs fixture, as the orchestrator would build them with the
    /// relative root `tests/fixtures/docs`. Node `i` is described by `labels[i]`.
    struct Fixture {
        tables: ResolveTables,
        labels: Vec<String>,
    }

    fn fixture_tables() -> Fixture {
        use crate::core::parser::CodeParser;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_PREFIX);
        let mut tables = ResolveTables::new(FIXTURE_PREFIX);
        let mut labels = Vec::new();
        let mut parser = CodeParser::new();
        let files = [
            "src/audit/log.rs",
            "src/audit/retention.rs",
            "src/billing/invoice.py",
            "docs/guide.md",
            "docs/links.md",
        ];
        for rel in files {
            let indexed = format!("{FIXTURE_PREFIX}/{rel}");
            let node = NodeIndex::new(labels.len());
            labels.push(indexed.clone());
            tables.add_file(&indexed, node);
            if rel.ends_with(".md") {
                continue;
            }
            let fs_path = root.join(rel);
            tables.set_line_text(&indexed, &fs::read_to_string(&fs_path).unwrap());
            for sym in parser.parse_file(&fs_path).unwrap().symbols {
                let node = NodeIndex::new(labels.len());
                labels.push(format!("{indexed}::{}", sym.name));
                tables.add_symbol(
                    &indexed,
                    SymbolRef {
                        node,
                        name: sym.name,
                        line: sym.line,
                        start_byte: sym.start_byte,
                        end_byte: sym.end_byte,
                        is_impl: sym.kind == "impl",
                    },
                );
            }
        }
        Fixture { tables, labels }
    }

    /// `linked:<label>`, `unresolved` or `ambiguous`.
    fn outcome(fx: &Fixture, mention: &Mention) -> String {
        match resolve_mention(mention, LINKS_DOC, &fx.tables) {
            Resolution::Linked(nodes) => {
                let names: Vec<_> = nodes
                    .iter()
                    .map(|n| fx.labels[n.index()].as_str())
                    .collect();
                format!("linked:{}", names.join(","))
            }
            Resolution::Unresolved => "unresolved".into(),
            Resolution::Ambiguous => "ambiguous".into(),
        }
    }

    fn resolve_all(fx: &Fixture) -> Vec<(MentionKind, MentionForm, String)> {
        parse_markdown(LINKS_DOC, LINKS)
            .mentions
            .iter()
            .map(|m| (m.kind, m.form.clone(), outcome(fx, m)))
            .collect()
    }

    fn lookup(
        all: &[(MentionKind, MentionForm, String)],
        kind: MentionKind,
        form: MentionForm,
    ) -> &str {
        &all.iter()
            .find(|(k, f, _)| *k == kind && *f == form)
            .unwrap_or_else(|| panic!("no mention {form:?}"))
            .2
    }

    fn log_rs(sym: &str) -> String {
        format!("linked:{FIXTURE_PREFIX}/src/audit/log.rs::{sym}")
    }

    #[test]
    fn test_rule1_markdown_links_resolve_against_the_document_directory() {
        use MentionKind::MarkdownLink as L;
        let all = resolve_all(&fixture_tables());
        let file = |rel: &str| format!("linked:{FIXTURE_PREFIX}/{rel}");
        assert_eq!(
            lookup(&all, L, path("../src/audit/log.rs")),
            file("src/audit/log.rs")
        );
        assert_eq!(lookup(&all, L, path("guide.md")), file("docs/guide.md"));
        // Missing target, and a link that climbs out of the index root.
        assert_eq!(lookup(&all, L, path("../src/missing.rs")), "unresolved");
        assert_eq!(
            lookup(&all, L, path("../../../../Cargo.toml")),
            "unresolved"
        );
    }

    #[test]
    fn test_rule2_paths_are_relative_to_the_index_root() {
        use MentionKind::CodeSpan as C;
        let all = resolve_all(&fixture_tables());
        let file = |rel: &str| format!("linked:{FIXTURE_PREFIX}/{rel}");
        assert_eq!(
            lookup(&all, C, path("src/audit/log.rs")),
            file("src/audit/log.rs")
        );
        assert_eq!(
            lookup(&all, C, path("src/audit/retention.rs")),
            file("src/audit/retention.rs")
        );
        // `./src/billing/invoice.py` is stored with the `./` stripped.
        assert_eq!(
            lookup(&all, C, path("src/billing/invoice.py")),
            file("src/billing/invoice.py")
        );
        assert_eq!(lookup(&all, C, path("src/audit/nothing.rs")), "unresolved");
    }

    #[test]
    fn test_rule3_path_symbol_and_path_line() {
        use MentionKind::CodeSpan as C;
        let all = resolve_all(&fixture_tables());
        let sym = MentionForm::PathSymbol {
            path: "src/audit/log.rs".into(),
            symbol: "append".into(),
        };
        let line = |n| MentionForm::PathLine {
            path: "src/audit/log.rs".into(),
            line: n,
        };
        assert_eq!(lookup(&all, C, sym), log_rs("append"));
        // Line 12 is inside `append` (and inside the impl block): the innermost wins.
        assert_eq!(lookup(&all, C, line(12)), log_rs("append"));
        // Line 1 is a doc comment outside any symbol: falls back to the file.
        assert_eq!(
            lookup(&all, C, line(1)),
            format!("linked:{FIXTURE_PREFIX}/src/audit/log.rs")
        );
    }

    #[test]
    fn test_rule3_symbol_missing_or_repeated_in_the_file() {
        let fx = fixture_tables();
        let at = |symbol: &str| Mention {
            kind: MentionKind::CodeSpan,
            form: MentionForm::PathSymbol {
                path: "src/audit/retention.rs".into(),
                symbol: symbol.into(),
            },
            byte: 0,
            section: None,
        };
        // `new` and `apply` each appear once in retention.rs.
        assert!(outcome(&fx, &at("apply")).starts_with("linked:"));
        assert_eq!(outcome(&fx, &at("nothing_here")), "unresolved");
        // A `Type::method` symbol resolves through the type's range.
        assert!(outcome(&fx, &at("RetentionPolicy::apply")).ends_with("::apply"));
        // `RetentionPolicy` is a struct and an impl block: the struct is the definition.
        assert!(outcome(&fx, &at("RetentionPolicy")).ends_with("::RetentionPolicy"));
    }

    #[test]
    fn test_rule4_type_member() {
        use MentionKind::CodeSpan as C;
        let all = resolve_all(&fixture_tables());
        // Linked through the impl block's byte range.
        assert_eq!(
            lookup(&all, C, member("AuditLog", "append")),
            log_rs("append")
        );
        assert_eq!(lookup(&all, C, member("AuditLog", "new")), log_rs("new"));
        // RetentionPolicy::apply exists in a Rust file and a Python file.
        assert_eq!(
            lookup(&all, C, member("RetentionPolicy", "apply")),
            "ambiguous"
        );
    }

    #[test]
    fn test_rule4_owner_not_found_or_member_outside_it() {
        let fx = fixture_tables();
        let mention = |owner: &str, name: &str| Mention {
            kind: MentionKind::CodeSpan,
            form: member(owner, name),
            byte: 0,
            section: None,
        };
        assert_eq!(outcome(&fx, &mention("Nothing", "append")), "unresolved");
        // `verify_chain` exists but is a free function, not inside AuditLog.
        assert_eq!(
            outcome(&fx, &mention("AuditLog", "verify_chain")),
            "unresolved"
        );
    }

    #[test]
    fn test_rule5_bare_names() {
        use MentionKind::CodeSpan as C;
        let all = resolve_all(&fixture_tables());
        assert_eq!(
            lookup(&all, C, bare("rotate_segments")),
            log_rs("rotate_segments")
        );
        assert_eq!(lookup(&all, C, bare("flush_buffer")), "ambiguous");
        // Denylisted (`new`), under four characters (`run`) and absent names do not link.
        assert_eq!(lookup(&all, C, bare("new")), "unresolved");
        assert_eq!(lookup(&all, C, bare("run")), "unresolved");
        assert_eq!(lookup(&all, C, bare("does_not_exist")), "unresolved");
    }

    #[test]
    fn test_rule5_type_name_is_not_ambiguous_with_its_impl_block() {
        let fx = fixture_tables();
        let m = Mention {
            kind: MentionKind::CodeSpan,
            form: bare("AuditLog"),
            byte: 0,
            section: None,
        };
        assert_eq!(outcome(&fx, &m), log_rs("AuditLog"));
    }

    #[test]
    fn test_rule5_short_and_denylisted_names_are_not_linked_even_if_unique() {
        let mut tables = ResolveTables::new("");
        tables.add_file("a.rs", NodeIndex::new(0));
        for (i, name) in ["ab", "path", "unique_name"].into_iter().enumerate() {
            tables.add_symbol(
                "a.rs",
                SymbolRef {
                    node: NodeIndex::new(i + 1),
                    name: name.into(),
                    line: 1,
                    start_byte: 0,
                    end_byte: 1,
                    is_impl: false,
                },
            );
        }
        let resolve = |name: &str| {
            let m = Mention {
                kind: MentionKind::CodeSpan,
                form: bare(name),
                byte: 0,
                section: None,
            };
            resolve_mention(&m, "doc.md", &tables)
        };
        assert_eq!(resolve("ab"), Resolution::Unresolved);
        assert_eq!(resolve("path"), Resolution::Unresolved);
        assert_eq!(
            resolve("unique_name"),
            Resolution::Linked(vec![NodeIndex::new(3)])
        );
    }

    #[test]
    fn test_paths_resolve_without_a_root_prefix() {
        let mut tables = ResolveTables::new("");
        tables.add_file("docs/a.md", NodeIndex::new(0));
        tables.add_file("src/x.rs", NodeIndex::new(1));
        let link = |target: &str| {
            let m = Mention {
                kind: MentionKind::MarkdownLink,
                form: path(target),
                byte: 0,
                section: None,
            };
            resolve_mention(&m, "docs/a.md", &tables)
        };
        assert_eq!(
            link("../src/x.rs"),
            Resolution::Linked(vec![NodeIndex::new(1)])
        );
        // One level too many leaves the root.
        assert_eq!(link("../../src/x.rs"), Resolution::Unresolved);
    }

    #[test]
    fn test_unmatched_counting_skips_unresolved_fenced_bare_names_only() {
        use MentionKind::*;
        let m = |kind, form| Mention {
            kind,
            form,
            byte: 0,
            section: None,
        };
        let (unresolved, ambiguous) = (Resolution::Unresolved, Resolution::Ambiguous);
        let fenced_bare = m(FencedCode, bare("verify_chain"));
        assert!(!counts_as_unmatched(&fenced_bare, &unresolved));
        assert!(counts_as_unmatched(&fenced_bare, &ambiguous));
        assert!(!counts_as_unmatched(
            &fenced_bare,
            &Resolution::Linked(vec![])
        ));
        // Every other unresolved mention still counts.
        assert!(counts_as_unmatched(
            &m(CodeSpan, bare("x_y_z")),
            &unresolved
        ));
        assert!(counts_as_unmatched(
            &m(FencedCode, path("src/none.rs")),
            &unresolved
        ));
        assert!(counts_as_unmatched(
            &m(FencedCode, member("A", "b")),
            &unresolved
        ));
        assert!(counts_as_unmatched(
            &m(MarkdownLink, path("gone.md")),
            &unresolved
        ));
    }

    #[test]
    fn test_fixture_fenced_mentions_resolve() {
        let fx = fixture_tables();
        let all = resolve_all(&fx);
        let fenced: Vec<_> = all
            .iter()
            .filter(|(k, ..)| *k == MentionKind::FencedCode)
            .collect();
        let by = |form: MentionForm| {
            fenced
                .iter()
                .find(|(_, f, _)| *f == form)
                .unwrap()
                .2
                .clone()
        };
        assert_eq!(by(member("AuditLog", "new")), log_rs("new"));
        assert_eq!(by(bare("verify_chain")), log_rs("verify_chain"));
    }

    // ---- Term aggregation (4.5) ----

    /// (term, count) kept for `docs/x.md`'s directory.
    fn kept_terms(outline: &DocOutline) -> Vec<(String, u32)> {
        aggregate_terms([("docs/x.md", outline)])
            .remove("docs")
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.term, t.count))
            .collect()
    }

    fn doc_with_terms(text: &str) -> DocOutline {
        parse_markdown("docs/x.md", text)
    }

    #[test]
    fn test_terms_once_only_is_dropped_and_order_is_count_then_term() {
        let text =
            "# H\n\nWiden Beta, Zed Yak here.\n\nZed Yak again and Widen Beta once more.\n\n\
                    Zed Yak a third time. Solo Term once.\n";
        let outline = doc_with_terms(text);
        assert_eq!(
            kept_terms(&outline),
            [("Zed Yak".to_string(), 3), ("Widen Beta".to_string(), 2)]
        );
    }

    #[test]
    fn test_terms_tie_is_broken_by_term_ascending() {
        let outline = doc_with_terms(
            "# H\n\nBeta Two, Alpha One.\n\nBeta Two. Alpha One. Gamma Six, Gamma Six.\n",
        );
        assert_eq!(
            kept_terms(&outline),
            [("Alpha One", 2), ("Beta Two", 2), ("Gamma Six", 2)].map(|(t, c)| (t.to_string(), c))
        );
    }

    #[test]
    fn test_terms_cap_of_fifty_per_directory_keeps_the_top_ranked() {
        // 60 distinct two-word terms; term i occurs 2 + i % 3 times, one paragraph each.
        let term = |i: usize| {
            format!(
                "K{} M{}",
                (b'a' + (i % 26) as u8) as char,
                (b'a' + (i / 26) as u8) as char
            )
        };
        let mut text = String::from("# H\n\n");
        for i in 0..60 {
            for _ in 0..2 + i % 3 {
                text.push_str(&format!("{} here.\n\n", term(i)));
            }
        }
        let outline = doc_with_terms(&text);
        let got = kept_terms(&outline);
        assert_eq!(got.len(), 50);

        let mut expected: Vec<(String, u32)> =
            (0..60).map(|i| (term(i), 2 + (i % 3) as u32)).collect();
        expected.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        expected.truncate(50);
        assert_eq!(got, expected);
    }

    #[test]
    fn test_terms_keep_at_most_five_distinct_sections_in_path_then_byte_order() {
        let mut a = String::new();
        for n in 0..8 {
            a.push_str(&format!(
                "## S{n}\n\nCasting Director here.\n\nCasting Director again.\n\n"
            ));
        }
        let first = parse_markdown("docs/b.md", &a);
        let second = parse_markdown("docs/a.md", "# Only\n\nCasting Director too.\n");
        // Input order must not matter: a.md sorts before b.md.
        let forward = aggregate_terms([("docs/b.md", &first), ("docs/a.md", &second)]);
        let backward = aggregate_terms([("docs/a.md", &second), ("docs/b.md", &first)]);
        assert_eq!(forward, backward);
        let term = &forward["docs"][0];
        assert_eq!((term.term.as_str(), term.count), ("Casting Director", 17));
        let sections: Vec<_> = term
            .sections
            .iter()
            .map(|(p, a)| (p.as_str(), a.as_str()))
            .collect();
        assert_eq!(
            sections,
            [
                ("docs/a.md", "only"),
                ("docs/b.md", "s0"),
                ("docs/b.md", "s1"),
                ("docs/b.md", "s2"),
                ("docs/b.md", "s3"),
            ]
        );
    }

    #[test]
    fn test_terms_are_grouped_by_parent_directory() {
        let a = doc_with_terms("# H\n\nCasting Director, Casting Director.\n");
        let by_dir = aggregate_terms([("docs/x.md", &a), ("other/y.md", &a), ("top.md", &a)]);
        assert_eq!(
            by_dir.keys().map(String::as_str).collect::<Vec<_>>(),
            [".", "docs", "other"]
        );
        // A term before the first heading has the empty anchor.
        let pre = parse_markdown("pre.md", "Casting Director and Casting Director.\n");
        let by_dir = aggregate_terms([("pre.md", &pre)]);
        assert_eq!(
            by_dir["."][0].sections,
            [("pre.md".to_string(), String::new())]
        );
    }

    #[test]
    fn test_terms_fixture_guide() {
        let outline = parse_markdown("docs/guide.md", GUIDE);
        let got = kept_terms(&outline);
        let count = |t: &str| got.iter().find(|(n, _)| n == t).map(|(_, c)| *c);
        assert!(count("Casting Director").unwrap_or(0) >= 2);
        assert!(count("Compliance Officer").unwrap_or(0) >= 2);
        // Once only, and the 6-word run, are not kept.
        assert_eq!(count("Release Manager"), None);
        assert_eq!(count("This Week"), None);
    }
}
