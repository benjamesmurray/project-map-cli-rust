use std::path::Path;

pub fn path_to_fqn(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let mut parts = Vec::new();
    
    for component in rel.components() {
        let part = component.as_os_str().to_string_lossy();
        if part == "__init__.py" || part == "mod.rs" || part == "lib.rs" || part == "index.ts" || part == "index.tsx" {
            continue;
        }
        let clean_part = part.trim_end_matches(".py")
            .trim_end_matches(".rs")
            .trim_end_matches(".tsx")
            .trim_end_matches(".ts")
            .trim_end_matches(".kt")
            .trim_end_matches(".sql")
            .trim_end_matches(".vue");
            
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
}
