use std::path::Path;

pub fn path_to_fqn(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let mut parts = Vec::new();
    
    for component in rel.components() {
        let part = component.as_os_str().to_string_lossy();
        if part == "__init__.py" || part == "mod.rs" || part == "lib.rs" || part == "index.ts" || part == "index.tsx" || part == "init.lua" {
            continue;
        }
        let clean_part = part.trim_end_matches(".py")
            .trim_end_matches(".rs")
            .trim_end_matches(".tsx")
            .trim_end_matches(".ts")
            .trim_end_matches(".kt")
            .trim_end_matches(".lua")
            .trim_end_matches(".php")
            .trim_end_matches(".tf")
            .trim_end_matches(".gd")
            .trim_end_matches(".sql")
            .trim_end_matches(".vue")
            .trim_end_matches(".md");
            
        if !clean_part.is_empty() {
            parts.push(clean_part.to_string());
        }
    }
    
    parts.join(".")
}

pub fn resolve_import_path(current_file: &str, import_specifier: &str) -> String {
    if !import_specifier.starts_with('.') {
        return import_specifier.to_string();
    }

    let current_path = Path::new(current_file);
    let current_dir = current_path.parent().unwrap_or_else(|| Path::new(""));
    let mut resolved = current_dir.to_path_buf();
    
    for part in import_specifier.split('/') {
        if part == "." {
            continue;
        } else if part == ".." {
            resolved.pop();
        } else {
            resolved.push(part);
        }
    }

    resolved.to_string_lossy().to_string()
}

pub fn render_tree(paths: &[String], max_depth: usize) -> String {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct TreeNode {
        children: BTreeMap<String, TreeNode>,
    }

    let mut root = TreeNode::default();
    for path_str in paths {
        let path = Path::new(path_str);
        let mut current = &mut root;
        for component in path.components() {
            let name = component.as_os_str().to_string_lossy().into_owned();
            current = current.children.entry(name).or_default();
        }
    }

    fn render_node(node: &TreeNode, name: &str, prefix: &str, is_last: bool, depth: usize, max_depth: usize) -> String {
        if depth > max_depth {
            return "".to_string();
        }

        let mut output = String::new();
        if !name.is_empty() {
            let marker = if is_last { "└── " } else { "├── " };
            output.push_str(&format!("{}{}{}\n", prefix, marker, name));
        }

        let new_prefix = if name.is_empty() {
            "".to_string()
        } else {
            format!("{}{}", prefix, if is_last { "    " } else { "│   " })
        };

        let child_count = node.children.len();
        for (i, (child_name, child_node)) in node.children.iter().enumerate() {
            let is_child_last = i == child_count - 1;
            output.push_str(&render_node(child_node, child_name, &new_prefix, is_child_last, depth + 1, max_depth));
        }

        output
    }

    render_node(&root, "", "", true, 0, max_depth)
}

pub fn get_active_features(root: &Path) -> Vec<String> {
    let mut active = Vec::new();
    let active_dir = root.join("projects/active");
    if let Ok(entries) = std::fs::read_dir(active_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let has_spec = path.join("Specification.md").exists()
                    || path.join("Tasks.json").exists()
                    || path.join(".deliver_meta.json").exists();
                if has_spec {
                    active.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
    }
    active.sort();
    active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_to_fqn() {
        let root = Path::new("/project");
        let path = Path::new("/project/src/core/utils.py");
        assert_eq!(path_to_fqn(root, path), "src.core.utils");
        
        let path2 = Path::new("/project/src/main.rs");
        assert_eq!(path_to_fqn(root, path2), "src.main");

        let path3 = Path::new("/project/src/core/__init__.py");
        assert_eq!(path_to_fqn(root, path3), "src.core");

        let path4 = Path::new("/project/tests/integration/test_main.py");
        assert_eq!(path_to_fqn(root, path4), "tests.integration.test_main");

        let path5 = Path::new("/project/src/components/Button.tsx");
        assert_eq!(path_to_fqn(root, path5), "src.components.Button");

        let path6 = Path::new("/project/src/components/index.ts");
        assert_eq!(path_to_fqn(root, path6), "src.components");
    }

    #[test]
    fn test_resolve_import_path() {
        assert_eq!(resolve_import_path("src/main.ts", "./utils"), "src/utils");
        assert_eq!(resolve_import_path("src/core/parser.ts", "../utils"), "src/utils");
        assert_eq!(resolve_import_path("src/index.ts", "lodash"), "lodash");
    }

    #[test]
    fn test_get_active_features() {
        let temp_dir = std::env::temp_dir().join(format!(
            "test_active_features_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let active_dir = temp_dir.join("projects").join("active");
        std::fs::create_dir_all(&active_dir).unwrap();

        // 1. Empty feature directory (should be ignored)
        std::fs::create_dir_all(active_dir.join("empty_feature")).unwrap();

        // 2. Directory with unrelated file (should be ignored)
        let other_dir = active_dir.join("unrelated_feature");
        std::fs::create_dir_all(&other_dir).unwrap();
        std::fs::write(other_dir.join("random.txt"), "hello").unwrap();

        // 3. Valid feature with Specification.md
        let spec_dir = active_dir.join("feature_with_spec");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("Specification.md"), "# Spec").unwrap();

        // 4. Valid feature with Tasks.json
        let tasks_dir = active_dir.join("feature_with_tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(tasks_dir.join("Tasks.json"), "{}").unwrap();

        // 5. Valid feature with .deliver_meta.json
        let meta_dir = active_dir.join("feature_with_meta");
        std::fs::create_dir_all(&meta_dir).unwrap();
        std::fs::write(meta_dir.join(".deliver_meta.json"), "{}").unwrap();

        // 6. Regular file in projects/active (should be ignored)
        std::fs::write(active_dir.join("notes.txt"), "some notes").unwrap();

        let features = get_active_features(&temp_dir);
        assert_eq!(
            features,
            vec!["feature_with_meta", "feature_with_spec", "feature_with_tasks"]
        );

        // Clean up
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_get_active_features_nonexistent_dir() {
        let nonexistent = Path::new("/path/that/definitely/does/not/exist/ever");
        let features = get_active_features(nonexistent);
        assert!(features.is_empty());
    }
}
