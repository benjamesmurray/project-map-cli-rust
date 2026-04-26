use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Serialize, Deserialize};
use std::fs;
use std::path::Path;
use crate::error::Result;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum NodeType {
    File,
    Symbol,
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
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum EdgeType {
    Contains,
    Imports,
    Calls,
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
        let serializable: SerializableGraph = serde_json::from_str(&json)?;
        
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
