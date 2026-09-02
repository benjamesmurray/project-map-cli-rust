use crate::core::graph::{NodeData, NodeType, EntityMatch};
use std::collections::{HashMap, HashSet};

pub struct ToonFormatter;

impl ToonFormatter {
    pub fn format_symbols(query: &str, matches: &[NodeData]) -> String {
        let mut output = format!("Resource: Symbols | Query: {}\n", query);
        output.push_str(&format!("Matches Found: {}\n", matches.len()));
        
        for m in matches.iter().take(10) {
            output.push_str(&format!("- {} ({}) [line {}]\n", m.path, m.name, m.line));
        }
        
        if matches.len() > 10 {
            output.push_str(&format!("... and {} more.\n", matches.len() - 10));
        }

        if let Some(first) = matches.first() {
            output.push_str(&format!("\nNext Step: `project-map impact --fqn {}`", first.name));
        }

        output
    }

    pub fn format_entity_matches(query: &str, matches: &[EntityMatch]) -> String {
        let mut output = format!("Resource: Symbols | Query: {}\n", query);
        output.push_str(&format!("Matches Found: {}\n", matches.len()));

        let is_kafka_query = matches.iter().any(|m| m.node_type == NodeType::KafkaTopic);

        if is_kafka_query {
            let topic_name = matches.iter().find(|m| m.node_type == NodeType::KafkaTopic).map(|m| m.name.as_str()).unwrap_or(query);
            output.push_str(&format!("\nSymbol: {} [KafkaTopic]\n", topic_name));

            let producers: Vec<_> = matches.iter().filter(|m| m.role.as_deref() == Some("Producer")).collect();
            let consumers: Vec<_> = matches.iter().filter(|m| m.role.as_deref() == Some("Consumer")).collect();
            let configs: Vec<_> = matches.iter().filter(|m| m.role.as_deref() == Some("Configuration") || m.role.as_deref() == Some("Definition")).collect();
            let references: Vec<_> = matches.iter().filter(|m| m.role.as_deref() == Some("Reference") || m.role.is_none()).collect();

            if !producers.is_empty() {
                output.push_str("\nProducers:\n");
                for p in producers {
                    output.push_str(&format!("  • {}:{}\n", p.path, p.line));
                    if let Some(prev) = &p.preview {
                        output.push_str(&format!("{}\n", prev.formatted));
                    }
                }
            }

            if !consumers.is_empty() {
                output.push_str("\nConsumers:\n");
                for c in consumers {
                    output.push_str(&format!("  • {}:{}\n", c.path, c.line));
                    if let Some(prev) = &c.preview {
                        output.push_str(&format!("{}\n", prev.formatted));
                    }
                }
            }

            if !configs.is_empty() {
                output.push_str("\nDefinitions & Configuration:\n");
                for cfg in configs {
                    output.push_str(&format!("  • {}:{}\n", cfg.path, cfg.line));
                    if let Some(prev) = &cfg.preview {
                        output.push_str(&format!("{}\n", prev.formatted));
                    }
                }
            }

            if !references.is_empty() {
                output.push_str("\nReferences:\n");
                for r in references {
                    output.push_str(&format!("  • {}:{}\n", r.path, r.line));
                    if let Some(prev) = &r.preview {
                        output.push_str(&format!("{}\n", prev.formatted));
                    }
                }
            }
        } else {
            for m in matches.iter().take(10) {
                output.push_str(&format!("\nMatch: {} [{:?}]\n", m.name, m.node_type));
                output.push_str(&format!("File: {}:{}\n", m.path, m.line));
                if let Some(role) = &m.role {
                    output.push_str(&format!("Role: {}\n", role));
                }
                if let Some(prev) = &m.preview {
                    output.push_str("Context:\n");
                    output.push_str(&format!("{}\n", prev.formatted));
                }
            }

            if matches.len() > 10 {
                output.push_str(&format!("\n... and {} more matches.\n", matches.len() - 10));
            }
        }

        if let Some(first) = matches.first() {
            output.push_str(&format!("\nNext Step: `project-map impact --fqn {}`", first.name));
        }

        output
    }

    pub fn format_file_context(path: &str, symbols: &[NodeData]) -> String {
        let mut output = format!("Resource: FileContext | Path: {}\n", path);
        output.push_str("\n--- File Outline ---\n");
        
        if symbols.is_empty() {
            output.push_str("- (No symbols detected or file not indexed)\n");
        } else {
            for s in symbols {
                output.push_str(&format!("- {} {} (line: {})\n", s.kind, s.name, s.line));
            }
        }

        output.push_str(&format!("\nNext Step: `project-map fetch --path {} --symbol <symbol_name>`", path));
        output
    }

    pub fn format_impact_analysis(fqn: &str, impact: &[NodeData]) -> String {
        let mut output = format!("Resource: Impact Analysis | Target: {}\n", fqn);
        output.push_str(&format!("Nodes Impacted: {}\n", impact.len()));
        
        for node in impact.iter().take(10) {
            output.push_str(&format!("- {:?}: {} ({})\n", node.node_type, node.name, node.path));
        }

        if impact.len() > 10 {
            output.push_str(&format!("... and {} more.\n", impact.len() - 10));
        }

        if let Some(first) = impact.first() {
            output.push_str(&format!("\nNext Step: `project-map blast --path {} --symbol {}` to see what this impacts.", first.path, first.name));
        }

        output
    }

    pub fn format_blast_radius(path: &str, symbol: &str, results: &[NodeData]) -> String {
        let mut output = format!("Resource: Blast Radius | Symbol: {} in {}\n", symbol, path);
        
        if results.is_empty() {
            output.push_str("No dependent components found.\n");
        } else {
            let mut dir_counts: HashMap<String, usize> = HashMap::new();
            let mut unique_files: HashSet<String> = HashSet::new();

            for r in results {
                unique_files.insert(r.path.clone());
                let dir = std::path::Path::new(&r.path)
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or("root")
                    .to_string();
                *dir_counts.entry(dir).or_insert(0) += 1;
            }

            output.push_str("Summary:\n");
            output.push_str(&format!("- Total Impacted Nodes: {}\n", results.len()));
            output.push_str(&format!("- Unique Files Affected: {}\n", unique_files.len()));
            output.push_str("- Affected Modules/Packages:\n");

            let mut sorted_dirs: Vec<_> = dir_counts.into_iter().collect();
            sorted_dirs.sort_by(|a, b| b.1.cmp(&a.1));

            for (dir, count) in sorted_dirs.iter().take(5) {
                output.push_str(&format!("  * {}: {} nodes\n", dir, count));
            }

            if sorted_dirs.len() > 5 {
                output.push_str(&format!("  * ... and {} more directories.\n", sorted_dirs.len() - 5));
            }

            output.push_str("\nTop Direct Dependents:\n");
            for r in results.iter().take(10) {
                output.push_str(&format!("- {} (ln: {}) -> {}\n", r.path, r.line, r.name));
            }

            if results.len() > 10 {
                output.push_str(&format!("... and {} more nodes omitted for brevity.\n", results.len() - 10));
            }

            if let Some(first) = results.first() {
                output.push_str(&format!("\nNext Step: `project-map fetch --path {} --symbol {}` to view source.", first.path, first.name));
            }
        }

        output
    }

    pub fn format_status(is_ready: bool, index_path: Option<&str>, pulse_tree: Option<&str>, active_features: &[String]) -> String {
        let mut output = "Project Map CLI - Status\n".to_string();
        if is_ready {
            output.push_str("Phase: Ready\n");
            if let Some(path) = index_path {
                output.push_str(&format!("Index: Found ({})\n", path));
            }

            if let Some(tree) = pulse_tree {
                output.push_str("\n--- Project Pulse ---\n");
                output.push_str(tree);
            }

            if !active_features.is_empty() {
                output.push_str("\n--- Active Features ---\n");
                for feature in active_features {
                    output.push_str(&format!("- projects/active/{}\n", feature));
                }
            }
        } else {
            output.push_str("Phase: Discovery (No index found)\n");
            output.push_str("Next Step: Run `project-map build` to generate the index.\n");
        }
        output
    }

    pub fn format_file_matches(query: &str, matches: &[NodeData]) -> String {
        let mut output = format!("Resource: Files | Query: {}\n", query);
        output.push_str(&format!("Matches Found: {}\n", matches.len()));
        
        for m in matches.iter().take(10) {
            output.push_str(&format!("- {}\n", m.path));
        }
        
        if matches.len() > 10 {
            output.push_str(&format!("... and {} more.\n", matches.len() - 10));
        }

        if let Some(first) = matches.first() {
            output.push_str(&format!("\nNext Step: `project-map context --path {}` to see the file outline.", first.path));
        }

        output
    }

    pub fn format_fetch_result(path: &str, symbol: &str, content: Option<&str>) -> String {
        if let Some(c) = content {
            format!("Resource: Fetch | Path: {} | Symbol: {}\n---\n{}\n---", path, symbol, c)
        } else {
            format!("Resource: Fetch | Status: Symbol not found: {} in {}", symbol, path)
        }
    }
}
