use crate::core::graph::{ProjectGraph, NodeData, NodeType};
use crate::error::Result;
use petgraph::visit::Dfs;
use std::path::Path;

pub struct QueryEngine {
    graph: ProjectGraph,
}

impl QueryEngine {
    pub fn load(path: &Path) -> Result<Self> {
        let graph = ProjectGraph::load(path)?;
        Ok(Self { graph })
    }

    pub fn find_symbols(&self, query: &str) -> Vec<NodeData> {
        let query_lower = query.to_lowercase();
        self.graph.graph.node_weights()
            .filter(|n| n.node_type == NodeType::Symbol && n.name.to_lowercase().contains(&query_lower))
            .cloned()
            .collect()
    }

    pub fn get_file_outline(&self, path: &str) -> Vec<NodeData> {
        let file_node = self.graph.graph.node_indices()
            .find(|i| self.graph.graph[*i].node_type == NodeType::File && self.graph.graph[*i].path == path);

        if let Some(idx) = file_node {
            self.graph.graph.neighbors_directed(idx, petgraph::Direction::Outgoing)
                .map(|n| self.graph.graph[n].clone())
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn analyze_impact(&self, name: &str) -> Vec<NodeData> {
        let node_idx = self.graph.graph.node_indices()
            .find(|i| self.graph.graph[*i].name == name);
        
        if let Some(start_node) = node_idx {
            // Impact: who do I depend on? (Outgoing edges)
            let mut dfs = Dfs::new(&self.graph.graph, start_node);
            let mut results = Vec::new();
            while let Some(nx) = dfs.next(&self.graph.graph) {
                if nx != start_node {
                    results.push(self.graph.graph[nx].clone());
                }
            }
            results
        } else {
            Vec::new()
        }
    }

    pub fn check_blast_radius(&self, path: &str, symbol: &str) -> Vec<NodeData> {
        let node_idx = self.graph.graph.node_indices()
            .find(|i| {
                let node = &self.graph.graph[*i];
                node.path == path && node.name == symbol && node.node_type == NodeType::Symbol
            });
        
        let start_node = if let Some(idx) = node_idx {
            idx
        } else {
            // Try matching just by file path if symbol not found
            let file_node = self.graph.graph.node_indices()
                .find(|i| {
                    let node = &self.graph.graph[*i];
                    node.path == path && node.node_type == NodeType::File
                });
            if let Some(idx) = file_node { idx } else { return Vec::new(); }
        };

        // Blast Radius: who depends on me? (Incoming edges)
        // We need to use a graph traversal that follows edges backwards.
        // petgraph's Dfs follows outgoing edges. To follow incoming, we can use a custom traversal or reverse the graph.
        // Alternatively, we can use neighbors_directed with Incoming in a loop.
        
        let mut results = Vec::new();
        let mut stack = vec![start_node];
        let mut visited = std::collections::HashSet::new();
        visited.insert(start_node);

        while let Some(current) = stack.pop() {
            for neighbor in self.graph.graph.neighbors_directed(current, petgraph::Direction::Incoming) {
                if visited.insert(neighbor) {
                    results.push(self.graph.graph[neighbor].clone());
                    stack.push(neighbor);
                }
            }
        }
        
        results
    }

    pub fn find_symbol_in_path(&self, path: &str, name: &str) -> Option<NodeData> {
        self.graph.graph.node_weights()
            .find(|n| n.node_type == NodeType::Symbol && n.path == path && n.name == name)
            .cloned()
    }
}
