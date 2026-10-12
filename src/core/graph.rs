use crate::error::Result;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexMeta {
    pub schema_version: u32,
    pub tool_version: String,
    pub root: String,
    pub built_at: String,
    pub builder_pid: u32,
    pub file_count: usize,
    pub symbol_count: usize,
    pub edge_count: usize,
}

#[derive(Serialize, Deserialize)]
struct SerializableGraph {
    #[serde(default)]
    meta: Option<IndexMeta>,
    nodes: Vec<NodeData>,
    edges: Vec<(usize, usize, EdgeType)>,
}

#[derive(Debug)]
pub struct ProjectGraph {
    pub graph: DiGraph<NodeData, EdgeType>,
    pub meta: Option<IndexMeta>,
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
        }
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
        };
        let json = serde_json::to_string_pretty(&serializable)?;
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
}
