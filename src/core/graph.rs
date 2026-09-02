use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Serialize, Deserialize};
use std::fs;
use std::path::Path;
use crate::error::Result;

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

#[derive(Serialize, Deserialize)]
struct SerializableGraph {
    nodes: Vec<NodeData>,
    edges: Vec<(usize, usize, EdgeType)>,
}

pub struct ProjectGraph {
    pub graph: DiGraph<NodeData, EdgeType>,
}

impl ProjectGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
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

        let serializable = SerializableGraph { nodes, edges };
        let json = serde_json::to_string_pretty(&serializable)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let json = fs::read_to_string(path)?;
        let serializable: SerializableGraph = match serde_json::from_str(&json) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "Warning: Schema mismatch or corrupted index at {:?} ({}). Triggering clean re-index...",
                    path, e
                );
                let root = path
                    .parent()
                    .and_then(|p| p.parent())
                    .unwrap_or_else(|| Path::new("."));
                let root = if root.as_os_str().is_empty() { Path::new(".") } else { root };

                let mut orch = crate::core::orchestrator::Orchestrator::new();
                let _ = orch.scaffold_if_empty(root);
                orch.build_index(root)?;
                let index_dir = path.parent().and_then(|p| p.parent()).unwrap_or_else(|| Path::new(".project-map"));
                orch.save_index_versioned(index_dir)?;

                let fresh_json = fs::read_to_string(path)?;
                serde_json::from_str(&fresh_json)?
            }
        };

        let mut graph = DiGraph::new();
        for node in serializable.nodes {
            graph.add_node(node);
        }
        for (from, to, edge_type) in serializable.edges {
            graph.add_edge(NodeIndex::new(from), NodeIndex::new(to), edge_type);
        }

        Ok(Self { graph })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type_deserialization_backwards_compatibility() {
        // PascalCase existing
        assert_eq!(serde_json::from_str::<NodeType>("\"File\"").unwrap(), NodeType::File);
        assert_eq!(serde_json::from_str::<NodeType>("\"Symbol\"").unwrap(), NodeType::Symbol);

        // New variants
        assert_eq!(serde_json::from_str::<NodeType>("\"KafkaTopic\"").unwrap(), NodeType::KafkaTopic);
        assert_eq!(serde_json::from_str::<NodeType>("\"DatabaseTable\"").unwrap(), NodeType::DatabaseTable);
        assert_eq!(serde_json::from_str::<NodeType>("\"ConfigEnvVar\"").unwrap(), NodeType::ConfigEnvVar);

        // Snake_case aliases
        assert_eq!(serde_json::from_str::<NodeType>("\"file\"").unwrap(), NodeType::File);
        assert_eq!(serde_json::from_str::<NodeType>("\"symbol\"").unwrap(), NodeType::Symbol);
        assert_eq!(serde_json::from_str::<NodeType>("\"kafka_topic\"").unwrap(), NodeType::KafkaTopic);
        assert_eq!(serde_json::from_str::<NodeType>("\"database_table\"").unwrap(), NodeType::DatabaseTable);
        assert_eq!(serde_json::from_str::<NodeType>("\"config_env_var\"").unwrap(), NodeType::ConfigEnvVar);

        // Unknown fallback
        assert_eq!(serde_json::from_str::<NodeType>("\"FutureNewType\"").unwrap(), NodeType::Unknown);
        assert_eq!(serde_json::from_str::<NodeType>("\"something_else\"").unwrap(), NodeType::Unknown);
    }

    #[test]
    fn test_edge_type_deserialization_backwards_compatibility() {
        // PascalCase existing
        assert_eq!(serde_json::from_str::<EdgeType>("\"Contains\"").unwrap(), EdgeType::Contains);
        assert_eq!(serde_json::from_str::<EdgeType>("\"Imports\"").unwrap(), EdgeType::Imports);
        assert_eq!(serde_json::from_str::<EdgeType>("\"Calls\"").unwrap(), EdgeType::Calls);

        // New variants
        assert_eq!(serde_json::from_str::<EdgeType>("\"Produces\"").unwrap(), EdgeType::Produces);
        assert_eq!(serde_json::from_str::<EdgeType>("\"Consumes\"").unwrap(), EdgeType::Consumes);
        assert_eq!(serde_json::from_str::<EdgeType>("\"Configures\"").unwrap(), EdgeType::Configures);
        assert_eq!(serde_json::from_str::<EdgeType>("\"References\"").unwrap(), EdgeType::References);

        // Snake_case aliases
        assert_eq!(serde_json::from_str::<EdgeType>("\"produces\"").unwrap(), EdgeType::Produces);
        assert_eq!(serde_json::from_str::<EdgeType>("\"consumes\"").unwrap(), EdgeType::Consumes);

        // Unknown fallback
        assert_eq!(serde_json::from_str::<EdgeType>("\"SubscribesTo\"").unwrap(), EdgeType::Unknown);
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
}
