# doc-and-spec-indexing - Specification

Index documentation alongside code so agents can read a repository's intent as well as its structure: Markdown heading sections, ADR frontmatter, OpenAPI operations and schemas, and links from docs to code symbols, exposed through compact TOON-formatted MCP tools. Requested by OST-Manager (P4), whose Casting Director extracts personas from this index.

**Target version**: `0.2.0` → `0.3.0` (minor: new node and edge types, index `schema_version` 2 → 3, new MCP tools; existing tools unchanged except their descriptions).
**Requested by**: OST-Manager roadmap P4 (`/home/benmurray/Projects/OST-Manager/projects/ROADMAP.md`). OST-Manager's P5 is gated on this project being released.

## Prerequisites

None. The earlier feature `index-integrity-and-concurrent-watcher-safety` was archived on 2026-10-21 (`projects/completed/2026-10-12_index-integrity-and-concurrent-watcher-safety/`).

## Background

Today `.md`, `.json` and `.toml` files are indexed as file nodes with **no symbols** (`parser.rs`, the `"md" | "json" | "toml"` arm), so an agent can find code but not what the documentation says about it. OST-Manager needs, deterministically and cheaply:
1. the structure and text of a repository's documentation, to extract personas (actors, stakeholders, constraints) with an LLM on its side;
2. the links between documentation and code, to ground arguments ("ADR-004 says audit logs are immutable; `audit/log.rs::append` is the writer");
3. API surface from OpenAPI specs.

This tool stays deterministic: no LLM, no persona logic. Persona extraction is OST-Manager's job.

## Core Features & Scope

### In-Scope
- **Markdown sections**: each heading becomes a `DocSection` node (name = heading text, kind = `h1`…`h6`, line, byte range, GitHub-style anchor slug in `role`). The section body (up to the next heading of the same or higher level) is its range. `Contains` edges: file → top sections → subsections. Duplicate anchors within a file follow GitHub: the first is `foo`, later ones `foo-1`, `foo-2`, ….
- **Frontmatter**: YAML frontmatter in Markdown (ADRs, specs) is parsed. Recognised keys (`id`, `title`, `status`, `date`, `domain`, `tags`, `supersedes`) become a `DocMeta` record for the file. `tags` and `supersedes` accept a scalar or a list. `doc_search` matches `id`, `title` and `tags`, scoring toward the file's first section.
- **Doc → code links**: `References` edges from a section to the files and symbols it mentions. A mention is attributed to the **deepest** section containing it, never to its ancestors. A mention before the first heading is attributed to the file itself (its `References` edge starts at the file node); it is never dropped. Mentions are deduplicated per (section, kind, form). Matching is exact, with no fuzzy guessing, and follows these rules in order:
  1. **Markdown links** to relative paths: resolved against the document's directory and normalised (`#fragment` stripped); linked only if the target is an indexed file. External URLs are ignored.
  2. **Path forms** in inline code spans and fenced code (`src/core/graph.rs`, `./src/core/graph.rs`): a file edge if the normalised path is indexed.
  3. **Path-qualified symbols** (`audit/log.rs::append`, `src/core/graph.rs:191`): the path is resolved first, then the symbol named after `::` within that file, or for `:line` the innermost symbol whose range contains that line (falling back to the file).
  4. **Type-qualified symbols** (`Graph::load`, `Graph.load`): a symbol named by the last segment whose byte range lies inside a symbol named by the preceding segment, in the same file. The index has no symbol → symbol `Contains` edges, so nesting comes from ranges; Rust `impl` blocks are indexed under the type's name, which makes this work. Linked only if exactly one such symbol exists.
  5. **Bare names** (`load`): linked only if **exactly one** indexed symbol has that name, the name is at least 4 characters, and it is not in a small denylist of generic names (`new`, `main`, `default`, `init`, `run`, `test`, `self`, `build`, `get`, `set`, ...).

  Notes on the rules:
  - A Rust `impl` block is indexed under its type's name but is not a second definition: for rules 3 and 5, a name that matches a struct and its impl block counts as one symbol (the struct); `impl` blocks only locate members for rule 4.
  - Bare names that are shorter than 4 characters or on the denylist are never linked and resolve as unresolved, even if only one symbol has the name.
  - A `path:line` mention whose line lies in no symbol links to the file.

  Anything not linked is counted as either unresolved or ambiguous, and both counts are reported by `status`, with one exception: a **bare name in fenced code** that matches no symbol is not counted. Fenced blocks are mostly shell, config and example identifiers (on `/opt/wde` they are the largest mention source, and almost none resolve), so counting them would bury the unresolved links and paths. An ambiguous fenced name still counts, because it matches real symbols.
- **OpenAPI**: a file is an OpenAPI spec if it is named `openapi.{yaml,yml,json}` or `swagger.*`, or if it is a YAML/JSON file of at most **1 MB** whose first 1 KB (after a BOM, whitespace and comments) has a top-level `openapi`/`swagger` key. Only then is it fully parsed. Detected specs produce `ApiOperation` nodes (`METHOD /path`, `operationId`, summary) and `ApiSchema` nodes (component schema names), located at their method or schema key (line, and bytes to the end of that line; found by key search because parsed values carry no spans, falling back to the file start) with `Contains` edges from the spec file, and **skip** the generic YAML symbol extraction (`extract_yaml_symbols`), so they are not indexed twice. Other JSON files are never fully parsed for detection.
- **Terms**: a cheap, deterministic hint list of candidate actors and domain vocabulary, computed at index time and stored, because section text is not stored. Candidates, taken from paragraph and list text only (not headings, code or link URLs), are:
  - runs of 2–4 consecutive capitalised words, after stripping leading stopwords (`The`, `A`, `An`, `This`, `That`, `These`, `Those`, `Our`, `Each`, `Every`, `When`, `If`, `In`, `For`, ...). Words are separated by whitespace only, so punctuation, code spans and bold spans end a run. A run still longer than 4 words after stripping is dropped, not split, because any split would be an arbitrary fragment;
  - bold spans (`**…**`) of 1–4 words.

  Terms are case-sensitive, with whitespace collapsed. A term is kept if it occurs at least twice in the directory. Each directory stores at most its top **50** terms, ordered by count descending and then by term ascending. Each term keeps at most **5** sections (the first by path, then byte order).
- **MCP tools** (TOON output, size-capped):
  - `doc_search(query, limit=10)`: ranked sections (path, anchor, heading, a two-line snippet).
  - `doc_outline(path)`: heading tree with anchors, plus frontmatter.
  - `doc_section(path, anchor, max_chars=4000)`: raw section text, truncated with a marker.
  - `doc_links(path, symbol?)`: sections that reference a file or symbol (code → docs), and symbols a document's sections reference (docs → code).
  - `doc_terms(path_prefix?, limit=30)`: the term list above.
  - `api_surface(path?)`: operations and schemas.
- The existing `search` stays **code-only**. Its description points to `doc_search` for documentation.
- `status` reports doc counts (doc files, sections, references, unresolved and ambiguous mentions, API operations).
- **Tool descriptions say when to use each tool instead of grep** (requested by OST-Manager on 2026-10-21: in its transcripts agents made about 30 map navigation calls against about 1,900 grep commands over 44 sessions). Every MCP tool description, the existing seven and the six new ones, names the question it answers better than a text search, for example `blast_radius`: "who depends on this symbol, across files and languages (grep cannot follow imports)"; `fetch_symbol`: "one function's or class's source without reading the whole file"; `query`: "a file's outline or a symbol's location". Only the description strings change; tool names, parameters and output are unchanged.
- The CLI gains matching subcommands (`doc search|outline|section|links|terms`, `api`).

### Out-of-Scope
- Any LLM use, embeddings or semantic ranking. `doc_search` uses the existing keyword/topic scoring extended to section headings and text.
- Non-Markdown documentation formats (reStructuredText, AsciiDoc, HTML) and docstring extraction changes.
- Persona extraction (OST-Manager P5).

## Technical Constraints
- **Language/Runtime**: Rust 2021, existing crate. Markdown parsing with `pulldown-cmark` (no tree-sitter Markdown grammar, which ships as two grammars and is more than needed). Frontmatter with `serde_yaml`. OpenAPI from `serde_yaml`/`serde_json` values, not a full OpenAPI model.
- **Index compatibility**:
  - `schema_version` 3. Nothing checks the version today (`Graph::load` ignores it), so this is new work.
  - An index with schema version below 3, or with no metadata block, is loaded **read-only**: `status` reports it as stale and says to run `refresh`, and the doc tools return the same message. A reader never rewrites it (honouring the 0.2.0 index-integrity guarantees).
  - The new `IndexMeta` count fields are `#[serde(default)]`, so 0.3 can read a version-2 metadata block.
- **Forward guard**: the builder refuses to overwrite an index whose `schema_version` is **newer** than its own. It returns a typed error and leaves the index untouched; the watcher logs it and skips the rebuild. This protects future upgrades. It cannot protect 0.3 indexes from the 0.2.x binaries already deployed: their watchers do a full `build_index` on any change and would write schema 2 over a schema-3 index. The rollout step below covers that.
- **Rollout**: after installing 0.3.0, restart **every** running `project-map-cli-rust mcp` process before the first 0.3 build, so no 0.2.x watcher is left to downgrade the index. This step goes in the README upgrade notes and the CHANGELOG.
- **Key constraints**: section text is **not** stored in the index (only byte ranges). `doc_section` reads the file on demand, like `fetch_symbol`. Every new tool's output is capped (default 4 000 characters) and says when it truncates. Indexing time for this repository and for `/opt/wde` grows by no more than 20 %.

## User Stories
- As an agent, I want to find which documents discuss a topic, so that I can read intent before code.
  - [ ] `doc_search("audit log")` returns the matching sections with path, anchor and snippet in TOON.
- As OST-Manager's Casting Director, I want the documentation's structure and recurring terms, so that the LLM can extract personas cheaply.
  - [ ] `doc_outline` and `doc_terms` for `tests/fixtures/docs/` together stay under 2 000 tokens (measured as characters ÷ 4). The same measurement on OST-Manager's `docs/` is recorded in the phase report.
- As the Megaphone, I want to connect a symbol to the docs that constrain it, so that arguments cite both.
  - [ ] `doc_links(path="src/audit/log.rs", symbol="append")` lists the sections that mention it; `doc_links(path="docs/adr/004.md")` lists the files and symbols that document's sections reference.
- As an agent, I want an API's surface without reading the whole spec, so that I can reason about endpoints.
  - [ ] `api_surface` lists `METHOD /path (operationId)` lines and schema names for each OpenAPI file.

## Overview & Architecture

Indexing gains a documentation pass after the code pass, because doc → code references need the symbol table:

1. **Code pass** (unchanged, except that detected OpenAPI files skip generic YAML extraction): files and symbols for supported languages.
2. **Doc pass**: for each `.md` file, parse frontmatter, then walk `pulldown-cmark` events to build sections and collect links, code-span and fenced-code mentions, and term candidates. For each YAML/JSON file, run the cheap OpenAPI detection and, on a match, extract operations and schemas.
3. **Link pass**: resolve collected mentions against the path and symbol tables using the ordered rules above, add `References` edges from the deepest section, and count unresolved and ambiguous mentions. Aggregate terms per directory.

The watcher already does a full `build_index` per change, so the link pass always sees a consistent symbol table.

New `NodeType` variants: `DocSection`, `ApiOperation`, `ApiSchema`. `EdgeType::References` is reused. The `#[serde(other)] Unknown` fallbacks already in `NodeType` and `EdgeType` keep 0.2.x readers from failing on a 0.3 index.

## Technical Stack
- **Backend**: Rust, petgraph (existing), `pulldown-cmark`, `serde_yaml` (existing), `rust-mcp-sdk` 0.9 (existing).
- **Utilities**: fixtures under `tests/fixtures/docs/` (Markdown with frontmatter, ADRs, OpenAPI YAML and JSON, a small code tree they reference, including a duplicate bare symbol name and a denylisted name).

## Components and Interfaces
- `src/core/docs.rs` (new): `parse_markdown(path, text) -> DocOutline { meta, sections, mentions, term_candidates }`; `is_openapi(path, head_bytes, size) -> bool`; `parse_openapi(value) -> Option<ApiOutline>`; `resolve_mention(mention, tables) -> Resolution { Linked(targets) | Unresolved | Ambiguous }`.
- `src/core/graph.rs`: `schema_version` check on load; `IndexMeta` gains `#[serde(default)]` fields `doc_file_count`, `section_count`, `reference_count`, `unresolved_mention_count`, `ambiguous_mention_count`, `api_operation_count`; side tables `doc_meta` and `doc_terms`.
- `src/core/orchestrator.rs`: doc pass, link pass, forward guard before writing.
- `src/core/parser.rs`: OpenAPI files skip `extract_yaml_symbols`.
- `src/core/doc_queries.rs` (new, an `impl QueryEngine` block kept apart from `query_engine.rs`): `search_docs`, `doc_outline`, `doc_section`, `doc_links`, `doc_terms`, `api_surface`. Each returns `Result<_, String>`; the `Err` text is shown as is (the stale-index message, an unknown path or anchor, a stale range).
- `src/core/query_engine.rs`: the existing code tools stay **code-only**: `find_symbols`, `analyze_impact`, `check_blast_radius` (docs that mention a symbol are not code dependents), `find_symbol_in_path` and the symbol count skip doc and API nodes.
- `src/core/toon.rs`: formatters for each, with the shared truncation rule.
- `src/mcp/server.rs`: six new tools registered beside the existing seven; all thirteen descriptions rewritten.
- `src/error.rs`: `AppError::NewerIndex { found, supported }`.

## Data Models

```rust
pub enum NodeType { File, Symbol, KafkaTopic, DatabaseTable, ConfigEnvVar,
                    DocSection, ApiOperation, ApiSchema, #[serde(other)] Unknown }

// DocSection NodeData: path, name = heading text, kind = "h2", line, start_byte, end_byte,
//                      role = Some(anchor_slug), docstring = None
// ApiOperation NodeData: name = "METHOD /path", kind = lower-case method, role = operationId,
//                        docstring = summary, line/bytes at the method key
// ApiSchema NodeData:    name = schema name, kind = "schema", role = None, docstring = None
// Side tables serialised with the index (both #[serde(default)]):
//   doc_meta:  BTreeMap<String /* path */, DocMeta>
//   doc_terms: BTreeMap<String /* directory */, Vec<DocTerm>>   // ≤ 50 per directory
pub struct DocMeta { pub id: Option<String>, pub title: Option<String>, pub status: Option<String>,
                     pub date: Option<String>, pub domain: Option<String>, pub tags: Vec<String>,
                     pub supersedes: Vec<String> }
pub struct DocTerm { pub term: String, pub count: u32,
                     pub sections: Vec<(String, String)> } // (path, anchor), ≤ 5
```

## Verification Criteria
1. Fixture tests for `docs.rs`:
   - the section tree, anchors (duplicates give `foo`, `foo-1`, `foo-2`) and frontmatter, including `supersedes` as a scalar and as a list;
   - each linking rule (Markdown link, path, `path::sym`, `path:line`, `Type::method`, unique bare name);
   - the non-links: a duplicate bare name and a denylisted name are not linked and are counted;
   - mentions attributed to the deepest section only;
   - term extraction (stopword stripping, bold terms, the ordering and caps).
2. OpenAPI: YAML and JSON fixtures produce operations and schemas, and a detected OpenAPI YAML produces **no** generic YAML symbols. A JSON file over 1 MB is not parsed for detection.
3. Compatibility:
   - A version-2 index (and a legacy index with no metadata) loads, is reported as stale, and is not rewritten by a query.
   - Building into a directory holding a `schema_version` 4 index fails with `NewerIndex` and leaves it untouched.
4. MCP tests for the six tools, including truncation markers and the stale-index message.
5. Indexing this repository and `/opt/wde`: time and index size before and after are recorded in the phase report; the growth limit above holds.
6. An MCP test lists the tools and checks that every description is non-empty and names its use case (the wording above).
7. README (including the restart-all-servers upgrade note) and CHANGELOG updated; released as 0.3.0.

**Post-release, not a release gate**: OST-Manager's acceptance. On OST-Manager's own `docs/`, `doc_terms` and `doc_outline` must produce usable persona-extraction input. This is checked by hand in OST-Manager P5's first task, and findings come back as a 0.3.x follow-up.
