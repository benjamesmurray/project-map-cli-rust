use crate::error::Result;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Index schema written by this build. 3 adds documentation and API nodes;
/// 4 adds Git fields in the meta block and the imports side table.
pub const SCHEMA_VERSION: u32 = 4;

/// Shown by `status` and returned by every doc tool when the index predates schema 3.
pub const STALE_INDEX_MESSAGE: &str = "STALE: Index predates documentation indexing (schema < 3). \
Code tools still work; rebuild with `project-map refresh` or the MCP `init` tool to enable doc tools.";

/// Shown by `status` when the index has doc data but predates schema 4.
pub const OLD_SCHEMA_MESSAGE: &str = "STALE: Index predates Git and import records (schema < 4). \
Doc and code tools still work; rebuild with `project-map refresh` or the MCP `init` tool to add them.";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    #[serde(alias = "file")]
    File,
    #[serde(alias = "symbol")]
    Symbol,
    #[serde(alias = "kafka_topic")]
    KafkaTopic,
    #[serde(alias = "database_table")]
    DatabaseTable,
    #[serde(alias = "config_env_var")]
    ConfigEnvVar,
    #[serde(alias = "doc_section")]
    DocSection,
    #[serde(alias = "api_operation")]
    ApiOperation,
    #[serde(alias = "api_schema")]
    ApiSchema,
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NodeData {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub node_type: NodeType,
    pub docstring: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum EdgeType {
    #[serde(alias = "contains")]
    Contains,
    #[serde(alias = "imports")]
    Imports,
    #[serde(alias = "calls")]
    Calls,
    #[serde(alias = "produces")]
    Produces,
    #[serde(alias = "consumes")]
    Consumes,
    #[serde(alias = "configures")]
    Configures,
    #[serde(alias = "references")]
    References,
    #[serde(other)]
    Unknown,
}

// Lazy on-demand models generated only during query presentation (not serialized in index)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SnippetPreview {
    pub target_line: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub lines: Vec<(usize, String)>,
    pub formatted: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntityMatch {
    pub name: String,
    pub kind: String,
    pub node_type: NodeType,
    pub path: String,
    pub line: usize,
    pub role: Option<String>, // "Producer", "Consumer", "Configuration", "Reference"
    pub preview: Option<SnippetPreview>,
}

impl NodeType {
    /// The snake_case name used in JSON output (the spelling the index also accepts).
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeType::File => "file",
            NodeType::Symbol => "symbol",
            NodeType::KafkaTopic => "kafka_topic",
            NodeType::DatabaseTable => "database_table",
            NodeType::ConfigEnvVar => "config_env_var",
            NodeType::DocSection => "doc_section",
            NodeType::ApiOperation => "api_operation",
            NodeType::ApiSchema => "api_schema",
            NodeType::Unknown => "unknown",
        }
    }

    /// Documentation and API nodes, counted separately from code symbols.
    pub fn is_doc(&self) -> bool {
        matches!(
            self,
            NodeType::DocSection | NodeType::ApiOperation | NodeType::ApiSchema
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexMeta {
    pub schema_version: u32,
    pub tool_version: String,
    pub root: String,
    pub built_at: String,
    pub builder_pid: u32,
    pub file_count: usize,
    pub symbol_count: usize,
    pub edge_count: usize,
    // Schema 3 doc counts; absent (zero) in schema-2 indexes.
    #[serde(default)]
    pub doc_file_count: usize,
    #[serde(default)]
    pub section_count: usize,
    #[serde(default)]
    pub reference_count: usize,
    #[serde(default)]
    pub unresolved_mention_count: usize,
    #[serde(default)]
    pub ambiguous_mention_count: usize,
    #[serde(default)]
    pub api_operation_count: usize,
    // Schema 4 Git state at build start; None when unavailable.
    /// Full 40-hex SHA of HEAD.
    #[serde(default)]
    pub git_commit: Option<String>,
    #[serde(default)]
    pub git_dirty: Option<bool>,
    /// None when HEAD is detached.
    #[serde(default)]
    pub git_branch: Option<String>,
}

/// One import statement, kept whether or not it resolved to an indexed target.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportEntry {
    /// Repository-relative, forward slashes.
    pub path: String,
    /// 1-based.
    pub line: usize,
    /// Trimmed import line, at most 200 chars.
    pub text: String,
    /// As written, e.g. "@sentry/node" or "serde_json::Value".
    pub module: String,
    /// Normalised external package; None for relative, crate-local or file imports.
    pub package: Option<String>,
    pub language: String,
    /// Linked to an indexed file or symbol in pass 2.
    pub resolved: bool,
}

/// Recognised YAML frontmatter of a Markdown file.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocMeta {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub supersedes: Vec<String>,
}

/// A recurring documentation term within one directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocTerm {
    pub term: String,
    pub count: u32,
    /// (path, anchor) of the sections it occurs in, capped.
    pub sections: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize)]
struct SerializableGraph {
    #[serde(default)]
    meta: Option<IndexMeta>,
    nodes: Vec<NodeData>,
    edges: Vec<(usize, usize, EdgeType)>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    doc_meta: BTreeMap<String, DocMeta>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    doc_terms: BTreeMap<String, Vec<DocTerm>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    imports: Vec<ImportEntry>,
}

/// Reads only the meta block; nodes and edges are skipped.
#[derive(Deserialize)]
struct MetaOnly {
    #[serde(default)]
    meta: Option<IndexMeta>,
}

#[derive(Debug)]
pub struct ProjectGraph {
    pub graph: DiGraph<NodeData, EdgeType>,
    pub meta: Option<IndexMeta>,
    /// Frontmatter keyed by normalised file path.
    pub doc_meta: BTreeMap<String, DocMeta>,
    /// Terms keyed by normalised directory.
    pub doc_terms: BTreeMap<String, Vec<DocTerm>>,
    /// Every import statement, sorted by path then line.
    pub imports: Vec<ImportEntry>,
}

impl Default for ProjectGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            meta: None,
            doc_meta: BTreeMap::new(),
            doc_terms: BTreeMap::new(),
            imports: Vec::new(),
        }
    }

    /// True for a legacy index (no meta) or one written with an older schema.
    /// Such an index is still served read-only; it is never rewritten here.
    pub fn is_stale(&self) -> bool {
        self.meta
            .as_ref()
            .is_none_or(|m| m.schema_version < SCHEMA_VERSION)
    }

    /// True when the index has no doc nodes (no meta or schema < 3).
    pub fn lacks_docs(&self) -> bool {
        self.meta.as_ref().is_none_or(|m| m.schema_version < 3)
    }

    pub fn add_node(&mut self, data: NodeData) -> NodeIndex {
        self.graph.add_node(data)
    }

    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, edge_type: EdgeType) {
        self.graph.add_edge(from, to, edge_type);
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let mut nodes = Vec::new();
        for i in 0..self.graph.node_count() {
            nodes.push(self.graph[NodeIndex::new(i)].clone());
        }

        let mut edges = Vec::new();
        for edge in self.graph.edge_indices() {
            let (from, to) = self.graph.edge_endpoints(edge).unwrap();
            edges.push((from.index(), to.index(), self.graph[edge].clone()));
        }

        let serializable = SerializableGraph {
            meta: self.meta.clone(),
            nodes,
            edges,
            doc_meta: self.doc_meta.clone(),
            doc_terms: self.doc_terms.clone(),
            imports: self.imports.clone(),
        };
        let json = serde_json::to_string(&serializable)?;
        Self::save_atomic_with_hook(path, &json, |_| Ok(()))
    }

    pub(crate) fn save_atomic_with_hook<F>(
        path: &Path,
        content: &str,
        mid_write_hook: F,
    ) -> Result<()>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("index.json");
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp_name = format!("{}.tmp.{}.{}", file_name, pid, nanos);
        let tmp_path = parent.join(tmp_name);

        let write_res = (|| -> Result<()> {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)?;
            }
            {
                use std::io::Write;
                let mut file = fs::File::create(&tmp_path)?;
                file.write_all(content.as_bytes())?;
                file.flush()?;
                file.sync_all()?;
            }
            mid_write_hook(&tmp_path)?;
            fs::rename(&tmp_path, path)?;
            Ok(())
        })();

        if let Err(e) = write_res {
            let _ = fs::remove_file(&tmp_path);
            return Err(e);
        }
        Ok(())
    }

    /// Schema version of the index at `path`, or None if it is missing,
    /// unreadable, corrupt or has no meta block.
    pub fn read_schema_version(path: &Path) -> Option<u32> {
        let json = fs::read_to_string(path).ok()?;
        let parsed: MetaOnly = serde_json::from_str(&json).ok()?;
        parsed.meta.map(|m| m.schema_version)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let json = fs::read_to_string(path)?;
        let serializable: SerializableGraph =
            serde_json::from_str(&json).map_err(|e| crate::error::AppError::CorruptIndex {
                path: path.to_path_buf(),
                source: e,
            })?;

        let mut graph = DiGraph::new();
        for mut node in serializable.nodes {
            node.path = crate::core::utils::normalize_rel_path(Path::new("."), &node.path);
            graph.add_node(node);
        }
        for (from, to, edge_type) in serializable.edges {
            graph.add_edge(NodeIndex::new(from), NodeIndex::new(to), edge_type);
        }

        Ok(Self {
            graph,
            meta: serializable.meta,
            doc_meta: serializable.doc_meta,
            doc_terms: serializable.doc_terms,
            imports: serializable.imports,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type_deserialization_backwards_compatibility() {
        // PascalCase existing
        assert_eq!(
            serde_json::from_str::<NodeType>("\"File\"").unwrap(),
            NodeType::File
        );
        assert_eq!(
            serde_json::from_str::<NodeType>("\"Symbol\"").unwrap(),
            NodeType::Symbol
        );

        // New variants
        assert_eq!(
            serde_json::from_str::<NodeType>("\"KafkaTopic\"").unwrap(),
            NodeType::KafkaTopic
        );
        assert_eq!(
            serde_json::from_str::<NodeType>("\"DatabaseTable\"").unwrap(),
            NodeType::DatabaseTable
        );
        assert_eq!(
            serde_json::from_str::<NodeType>("\"ConfigEnvVar\"").unwrap(),
            NodeType::ConfigEnvVar
        );

        // Snake_case aliases
        assert_eq!(
            serde_json::from_str::<NodeType>("\"file\"").unwrap(),
            NodeType::File
        );
        assert_eq!(
            serde_json::from_str::<NodeType>("\"symbol\"").unwrap(),
            NodeType::Symbol
        );
        assert_eq!(
            serde_json::from_str::<NodeType>("\"kafka_topic\"").unwrap(),
            NodeType::KafkaTopic
        );
        assert_eq!(
            serde_json::from_str::<NodeType>("\"database_table\"").unwrap(),
            NodeType::DatabaseTable
        );
        assert_eq!(
            serde_json::from_str::<NodeType>("\"config_env_var\"").unwrap(),
            NodeType::ConfigEnvVar
        );

        // Unknown fallback
        assert_eq!(
            serde_json::from_str::<NodeType>("\"FutureNewType\"").unwrap(),
            NodeType::Unknown
        );
        assert_eq!(
            serde_json::from_str::<NodeType>("\"something_else\"").unwrap(),
            NodeType::Unknown
        );
    }

    #[test]
    fn test_edge_type_deserialization_backwards_compatibility() {
        // PascalCase existing
        assert_eq!(
            serde_json::from_str::<EdgeType>("\"Contains\"").unwrap(),
            EdgeType::Contains
        );
        assert_eq!(
            serde_json::from_str::<EdgeType>("\"Imports\"").unwrap(),
            EdgeType::Imports
        );
        assert_eq!(
            serde_json::from_str::<EdgeType>("\"Calls\"").unwrap(),
            EdgeType::Calls
        );

        // New variants
        assert_eq!(
            serde_json::from_str::<EdgeType>("\"Produces\"").unwrap(),
            EdgeType::Produces
        );
        assert_eq!(
            serde_json::from_str::<EdgeType>("\"Consumes\"").unwrap(),
            EdgeType::Consumes
        );
        assert_eq!(
            serde_json::from_str::<EdgeType>("\"Configures\"").unwrap(),
            EdgeType::Configures
        );
        assert_eq!(
            serde_json::from_str::<EdgeType>("\"References\"").unwrap(),
            EdgeType::References
        );

        // Snake_case aliases
        assert_eq!(
            serde_json::from_str::<EdgeType>("\"produces\"").unwrap(),
            EdgeType::Produces
        );
        assert_eq!(
            serde_json::from_str::<EdgeType>("\"consumes\"").unwrap(),
            EdgeType::Consumes
        );

        // Unknown fallback
        assert_eq!(
            serde_json::from_str::<EdgeType>("\"SubscribesTo\"").unwrap(),
            EdgeType::Unknown
        );
    }

    #[test]
    fn test_node_data_without_role_deserializes() {
        let json = r#"{
            "path": "test.rs",
            "name": "my_fn",
            "kind": "function",
            "line": 10,
            "start_byte": 100,
            "end_byte": 200,
            "node_type": "Symbol",
            "docstring": null
        }"#;
        let data: NodeData = serde_json::from_str(json).unwrap();
        assert_eq!(data.name, "my_fn");
        assert_eq!(data.node_type, NodeType::Symbol);
        assert_eq!(data.role, None);
    }

    #[test]
    fn test_legacy_index_without_meta_loads() {
        let legacy_json = r#"{
            "nodes": [
                {
                    "path": "./src/main.rs",
                    "name": "main",
                    "kind": "function",
                    "line": 1,
                    "start_byte": 0,
                    "end_byte": 10,
                    "node_type": "Symbol",
                    "docstring": null
                }
            ],
            "edges": []
        }"#;

        let graph: SerializableGraph =
            serde_json::from_str(legacy_json).expect("Legacy index must parse");
        assert!(
            graph.meta.is_none(),
            "Legacy index without meta must have meta = None"
        );
        assert_eq!(graph.nodes.len(), 1);
    }

    #[test]
    fn test_forward_compatibility_0_2_0_with_0_1_18_struct() {
        // Copy of 0.1.18 SerializableGraph (without meta field)
        #[derive(Deserialize)]
        struct V0118SerializableGraph {
            nodes: Vec<NodeData>,
            edges: Vec<(usize, usize, EdgeType)>,
        }

        let v0_2_0_json = r#"{
            "meta": {
                "schema_version": 2,
                "tool_version": "0.2.0",
                "root": "/opt/project",
                "built_at": "2026-10-12T15:00:00Z",
                "builder_pid": 12345,
                "file_count": 1,
                "symbol_count": 1,
                "edge_count": 0
            },
            "nodes": [
                {
                    "path": "src/main.rs",
                    "name": "main",
                    "kind": "function",
                    "line": 1,
                    "start_byte": 0,
                    "end_byte": 10,
                    "node_type": "Symbol",
                    "docstring": null
                }
            ],
            "edges": []
        }"#;

        let old_reader: V0118SerializableGraph = serde_json::from_str(v0_2_0_json)
            .expect("0.1.18 reader must successfully parse 0.2.0 index with meta");
        assert_eq!(old_reader.nodes.len(), 1);
        assert_eq!(old_reader.edges.len(), 0);
    }

    #[test]
    fn test_doc_node_types_round_trip() {
        for (variant, pascal, snake) in [
            (NodeType::DocSection, "\"DocSection\"", "\"doc_section\""),
            (
                NodeType::ApiOperation,
                "\"ApiOperation\"",
                "\"api_operation\"",
            ),
            (NodeType::ApiSchema, "\"ApiSchema\"", "\"api_schema\""),
        ] {
            assert_eq!(serde_json::to_string(&variant).unwrap(), pascal);
            assert_eq!(serde_json::from_str::<NodeType>(pascal).unwrap(), variant);
            assert_eq!(serde_json::from_str::<NodeType>(snake).unwrap(), variant);
            assert!(variant.is_doc());
        }
        assert!(!NodeType::Symbol.is_doc());
        assert!(!NodeType::File.is_doc());
    }

    #[test]
    fn test_schema_2_meta_without_doc_counts_defaults_to_zero() {
        let v2_json = r#"{
            "meta": {
                "schema_version": 2,
                "tool_version": "0.2.0",
                "root": "/opt/project",
                "built_at": "2026-10-12T15:00:00Z",
                "builder_pid": 12345,
                "file_count": 1,
                "symbol_count": 1,
                "edge_count": 0
            },
            "nodes": [],
            "edges": []
        }"#;

        let graph: SerializableGraph =
            serde_json::from_str(v2_json).expect("Schema-2 index must parse");
        let meta = graph.meta.expect("meta present");
        assert_eq!(meta.schema_version, 2);
        assert_eq!(meta.doc_file_count, 0);
        assert_eq!(meta.section_count, 0);
        assert_eq!(meta.reference_count, 0);
        assert_eq!(meta.unresolved_mention_count, 0);
        assert_eq!(meta.ambiguous_mention_count, 0);
        assert_eq!(meta.api_operation_count, 0);
        assert!(graph.doc_meta.is_empty(), "missing side table must default");
        assert!(
            graph.doc_terms.is_empty(),
            "missing side table must default"
        );
    }

    fn sample_v4_graph() -> ProjectGraph {
        let mut g = ProjectGraph::new();
        let file = g.add_node(NodeData {
            path: "docs/adr/004.md".to_string(),
            name: "docs.adr.004".to_string(),
            kind: "file".to_string(),
            line: 1,
            start_byte: 0,
            end_byte: 0,
            node_type: NodeType::File,
            docstring: None,
            role: None,
        });
        let section = g.add_node(NodeData {
            path: "docs/adr/004.md".to_string(),
            name: "Decision".to_string(),
            kind: "h2".to_string(),
            line: 5,
            start_byte: 40,
            end_byte: 120,
            node_type: NodeType::DocSection,
            docstring: None,
            role: Some("decision".to_string()),
        });
        g.add_edge(file, section, EdgeType::Contains);
        g.meta = Some(IndexMeta {
            schema_version: SCHEMA_VERSION,
            tool_version: "0.3.0".to_string(),
            section_count: 1,
            git_commit: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            git_dirty: Some(true),
            git_branch: None,
            ..Default::default()
        });
        g.imports.push(ImportEntry {
            path: "web/app.js".to_string(),
            line: 3,
            text: "import * as Sentry from '@sentry/node';".to_string(),
            module: "@sentry/node".to_string(),
            package: Some("@sentry/node".to_string()),
            language: "javascript".to_string(),
            resolved: false,
        });
        g.doc_meta.insert(
            "docs/adr/004.md".to_string(),
            DocMeta {
                id: Some("ADR-004".to_string()),
                tags: vec!["audit".to_string()],
                supersedes: vec!["ADR-001".to_string()],
                ..Default::default()
            },
        );
        g.doc_terms.insert(
            "docs/adr".to_string(),
            vec![DocTerm {
                term: "Casting Director".to_string(),
                count: 2,
                sections: vec![("docs/adr/004.md".to_string(), "decision".to_string())],
            }],
        );
        g
    }

    #[test]
    fn test_v4_side_tables_round_trip_through_save_and_load() {
        use tempfile::tempdir;
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("index.json");

        let original = sample_v4_graph();
        original.save(&path).expect("save");
        let loaded = ProjectGraph::load(&path).expect("load");

        assert_eq!(loaded.meta, original.meta);
        assert_eq!(loaded.doc_meta, original.doc_meta);
        assert_eq!(loaded.doc_terms, original.doc_terms);
        assert_eq!(loaded.imports, original.imports);
        assert_eq!(loaded.imports.len(), 1);
        assert_eq!(
            loaded.graph[NodeIndex::new(1)].node_type,
            NodeType::DocSection
        );
    }

    #[test]
    fn test_forward_compatibility_0_2_0_reader_with_v3_index() {
        // Copies of the 0.2.0 types (no doc variants, fields or side tables)
        #[derive(Deserialize, Debug, PartialEq)]
        enum V020NodeType {
            File,
            Symbol,
            KafkaTopic,
            DatabaseTable,
            ConfigEnvVar,
            #[serde(other)]
            Unknown,
        }
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct V020NodeData {
            path: String,
            name: String,
            kind: String,
            line: usize,
            start_byte: usize,
            end_byte: usize,
            node_type: V020NodeType,
            docstring: Option<String>,
            #[serde(default)]
            role: Option<String>,
        }
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct V020IndexMeta {
            schema_version: u32,
            tool_version: String,
            root: String,
            built_at: String,
            builder_pid: u32,
            file_count: usize,
            symbol_count: usize,
            edge_count: usize,
        }
        #[derive(Deserialize)]
        struct V020SerializableGraph {
            #[serde(default)]
            meta: Option<V020IndexMeta>,
            nodes: Vec<V020NodeData>,
            edges: Vec<(usize, usize, EdgeType)>,
        }

        use tempfile::tempdir;
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("index.json");
        sample_v4_graph().save(&path).expect("save");

        let json = fs::read_to_string(&path).unwrap();
        let old_reader: V020SerializableGraph =
            serde_json::from_str(&json).expect("0.2.0 reader must parse a v3 index");
        assert_eq!(old_reader.meta.unwrap().schema_version, SCHEMA_VERSION);
        assert_eq!(old_reader.nodes[0].node_type, V020NodeType::File);
        assert_eq!(
            old_reader.nodes[1].node_type,
            V020NodeType::Unknown,
            "DocSection must map to Unknown for a 0.2.0 reader"
        );
        assert_eq!(old_reader.edges.len(), 1);
    }

    #[test]
    fn test_atomic_save_injected_failure_preserves_original_file() {
        use tempfile::tempdir;
        let temp = tempdir().expect("tempdir");
        let target_file = temp.path().join("index.json");

        // Write initial content
        let original_content = "ORIGINAL_VALID_CONTENT";
        fs::write(&target_file, original_content).expect("write original");

        // Attempt atomic save with injected failure mid-write
        let res = ProjectGraph::save_atomic_with_hook(&target_file, "NEW_CONTENT", |_tmp_path| {
            Err(crate::error::AppError::Generic(
                "injected mid-write failure".into(),
            ))
        });

        assert!(res.is_err(), "Atomic save with error hook must fail");

        // Verify original file is intact and byte-identical
        let content_after = fs::read_to_string(&target_file).expect("read target after");
        assert_eq!(
            content_after, original_content,
            "Original file must remain untouched"
        );

        // Verify no tmp files remain in the directory
        let entries: Vec<_> = fs::read_dir(temp.path())
            .expect("read_dir")
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "Only the target file should remain in directory: {:?}",
            entries
        );
        assert_eq!(entries[0], "index.json");
    }

    #[test]
    fn test_schema_3_index_loads_stale_with_empty_imports() {
        let v3_json = r#"{
            "meta": {
                "schema_version": 3,
                "tool_version": "0.3.0",
                "root": "/tmp/x",
                "built_at": "2026-10-01T00:00:00Z",
                "builder_pid": 1,
                "file_count": 0,
                "symbol_count": 0,
                "edge_count": 0,
                "section_count": 2
            },
            "nodes": [],
            "edges": []
        }"#;
        let graph: SerializableGraph = serde_json::from_str(v3_json).expect("schema 3 parses");
        let meta = graph.meta.expect("meta");
        assert_eq!(meta.section_count, 2);
        assert_eq!(meta.git_commit, None);
        assert_eq!(meta.git_dirty, None);
        assert_eq!(meta.git_branch, None);
        assert!(graph.imports.is_empty(), "missing side table must default");

        let mut g = ProjectGraph::new();
        g.meta = Some(IndexMeta {
            schema_version: 3,
            ..Default::default()
        });
        assert!(g.is_stale());
        assert!(!g.lacks_docs(), "schema 3 keeps its doc data");
        g.meta.as_mut().unwrap().schema_version = 2;
        assert!(g.lacks_docs());
        g.meta.as_mut().unwrap().schema_version = SCHEMA_VERSION;
        assert!(!g.is_stale());
    }

    #[test]
    fn test_forward_compatibility_0_3_0_reader_with_v4_index() {
        // Copy of the 0.3.0 meta and graph: no Git fields, no imports table.
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct V030IndexMeta {
            schema_version: u32,
            tool_version: String,
            root: String,
            built_at: String,
            builder_pid: u32,
            file_count: usize,
            symbol_count: usize,
            edge_count: usize,
            #[serde(default)]
            doc_file_count: usize,
            #[serde(default)]
            section_count: usize,
            #[serde(default)]
            reference_count: usize,
            #[serde(default)]
            unresolved_mention_count: usize,
            #[serde(default)]
            ambiguous_mention_count: usize,
            #[serde(default)]
            api_operation_count: usize,
        }
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct V030SerializableGraph {
            #[serde(default)]
            meta: Option<V030IndexMeta>,
            nodes: Vec<NodeData>,
            edges: Vec<(usize, usize, EdgeType)>,
            #[serde(default)]
            doc_meta: BTreeMap<String, DocMeta>,
            #[serde(default)]
            doc_terms: BTreeMap<String, Vec<DocTerm>>,
        }

        use tempfile::tempdir;
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("index.json");
        sample_v4_graph().save(&path).expect("save");

        let json = fs::read_to_string(&path).unwrap();
        assert!(json.contains("\"git_commit\""));
        assert!(json.contains("\"imports\""));
        let old_reader: V030SerializableGraph =
            serde_json::from_str(&json).expect("0.3.0 reader must parse a v4 index");
        assert_eq!(old_reader.meta.unwrap().schema_version, SCHEMA_VERSION);
        assert_eq!(old_reader.nodes.len(), 2);
        assert_eq!(old_reader.doc_meta.len(), 1);
    }
}
