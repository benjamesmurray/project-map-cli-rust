//! Resolution of Rust `use` paths to indexed files and symbols.
//!
//! Only `crate::`, `self::`, `super::` and the crate's own name resolve; any other first
//! segment is an external crate (or `std`) and stays unresolved.

/// What a `use` leaf points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustTarget {
    /// The module file itself.
    File(String),
    /// An item that should be a symbol named `name` in `file`; the caller falls back to the file.
    Symbol { file: String, name: String },
}

/// The crate containing the importing file.
#[derive(Debug, Clone)]
pub struct CrateInfo {
    /// Directory holding Cargo.toml, relative to the project root; "" for the root itself.
    pub dir: String,
    /// `[package] name` with `-` replaced by `_`.
    pub name: Option<String>,
}

impl CrateInfo {
    fn src(&self) -> String {
        join(&self.dir, "src")
    }
}

/// `[package] name` of a Cargo.toml, with `-` as `_`.
pub fn crate_name(cargo_toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("name") {
            let value = rest.trim_start().strip_prefix('=')?.trim();
            let value = value.trim_matches(|c| c == '"' || c == '\'');
            if !value.is_empty() {
                return Some(value.replace('-', "_"));
            }
        }
    }
    None
}

/// Expands a use tree into its leaf paths: `a::{b, c::d, self}` gives `a::b`, `a::c::d`, `a`.
/// Aliases (`as x`) are dropped and a glob (`a::*`) gives `a`.
pub fn expand_use(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    expand("", text.trim().trim_start_matches("::"), &mut out);
    out
}

fn expand(prefix: &str, tree: &str, out: &mut Vec<String>) {
    let tree = tree.trim();
    if tree.is_empty() {
        return;
    }
    if let Some(open) = tree.find('{') {
        let head = format!("{prefix}{}", &tree[..open]);
        let body_end = matching_brace(tree, open).unwrap_or(tree.len());
        let body = &tree[open + 1..body_end.min(tree.len())];
        for item in split_top_level(body) {
            expand(&head, item, out);
        }
        return;
    }
    let leaf = tree.split(" as ").next().unwrap_or(tree).trim();
    let full = format!("{prefix}{leaf}");
    let full = if leaf == "self" || leaf == "*" {
        prefix.trim_end_matches("::").to_string()
    } else {
        full.strip_suffix("::*").unwrap_or(&full).to_string()
    };
    if !full.is_empty() {
        out.push(full);
    }
}

fn matching_brace(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices().skip_while(|(i, _)| *i < open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(body: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

/// Directory holding the child modules of the module defined by `from`.
fn module_dir(from: &str, src: &str) -> String {
    let dir = parent(from);
    let stem = from
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim_end_matches(".rs");
    if dir == src && (stem == "lib" || stem == "main") {
        src.to_string()
    } else if stem == "mod" {
        dir.to_string()
    } else {
        join(dir, stem)
    }
}

/// The file defining the module whose child directory is `dir`.
fn module_file(dir: &str, src: &str, has_file: &dyn Fn(&str) -> bool) -> Option<String> {
    let candidates = if dir == src {
        vec![join(dir, "lib.rs"), join(dir, "main.rs")]
    } else {
        vec![format!("{dir}.rs"), join(dir, "mod.rs")]
    };
    candidates.into_iter().find(|c| has_file(c))
}

/// Resolves one use leaf (`crate::a::b::Sym`) written in the file `from`.
pub fn resolve(
    path: &str,
    from: &str,
    krate: &CrateInfo,
    has_file: &dyn Fn(&str) -> bool,
) -> Option<RustTarget> {
    let src = krate.src();
    let mut segs = path.split("::").filter(|s| !s.is_empty());
    let first = segs.next()?;
    let (mut dir, mut file) = match first {
        "crate"
            if from.starts_with(&format!("{src}/"))
                && !from.starts_with(&format!("{src}/bin/")) =>
        {
            (src.clone(), module_file(&src, &src, has_file)?)
        }
        "self" => (module_dir(from, &src), from.to_string()),
        "super" => {
            let mut dir = module_dir(from, &src);
            if dir == src {
                return None;
            }
            dir = parent(&dir).to_string();
            (dir.clone(), module_file(&dir, &src, has_file)?)
        }
        name if krate.name.as_deref() == Some(name) => {
            (src.clone(), module_file(&src, &src, has_file)?)
        }
        _ => return None,
    };

    let mut rest: Vec<&str> = segs.collect();
    // Further `super` segments climb one module each.
    if first == "super" {
        while rest.first() == Some(&"super") {
            rest.remove(0);
            if dir == src {
                return None;
            }
            dir = parent(&dir).to_string();
            file = module_file(&dir, &src, has_file)?;
        }
    }

    let mut i = 0;
    while i < rest.len() {
        let seg = rest[i];
        let flat = format!("{}.rs", join(&dir, seg));
        let nested = join(&join(&dir, seg), "mod.rs");
        let found = if has_file(&flat) {
            flat
        } else if has_file(&nested) {
            nested
        } else {
            break;
        };
        dir = join(&dir, seg);
        file = found;
        i += 1;
    }

    match rest.get(i) {
        None => Some(RustTarget::File(file)),
        Some(name) => Some(RustTarget::Symbol {
            file,
            name: (*name).to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn files(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_crate_name_reads_the_package_table() {
        let toml = "[workspace]\nname = \"nope\"\n\n[package]\nversion = \"1\"\nname = \"my-crate\"\n\n[dependencies]\nname = \"x\"\n";
        assert_eq!(crate_name(toml).as_deref(), Some("my_crate"));
        assert_eq!(crate_name("[dependencies]\nname = \"x\"\n"), None);
        assert_eq!(
            crate_name("[package]\nname=\"a-b-c\"\n").as_deref(),
            Some("a_b_c")
        );
    }

    #[test]
    fn test_expand_use_trees() {
        assert_eq!(expand_use("a::b::C"), ["a::b::C"]);
        assert_eq!(
            expand_use("crate::a::{b, c}"),
            ["crate::a::b", "crate::a::c"]
        );
        assert_eq!(
            expand_use("std::{fs, io::{self, Read}, collections::HashMap as Map}"),
            [
                "std::fs",
                "std::io",
                "std::io::Read",
                "std::collections::HashMap"
            ]
        );
        assert_eq!(expand_use("a::b::*"), ["a::b"]);
        assert_eq!(expand_use("::serde::Serialize"), ["serde::Serialize"]);
        assert_eq!(expand_use("{a, b::c}"), ["a", "b::c"]);
        assert_eq!(expand_use("serde_json"), ["serde_json"]);
    }

    #[test]
    fn test_resolve_crate_self_super_and_own_name() {
        let set = files(&[
            "k/src/lib.rs",
            "k/src/a/mod.rs",
            "k/src/a/b.rs",
            "k/src/a/c.rs",
            "k/src/c/mod.rs",
            "k/src/user.rs",
            "k/tests/t.rs",
        ]);
        let has = |p: &str| set.contains(p);
        let krate = CrateInfo {
            dir: "k".into(),
            name: Some("kk".into()),
        };
        let sym = |file: &str, name: &str| {
            Some(RustTarget::Symbol {
                file: file.into(),
                name: name.into(),
            })
        };
        let file = |f: &str| Some(RustTarget::File(f.into()));

        assert_eq!(
            resolve("crate::a::b::Sym", "k/src/user.rs", &krate, &has),
            sym("k/src/a/b.rs", "Sym")
        );
        assert_eq!(
            resolve("crate::a::b", "k/src/user.rs", &krate, &has),
            file("k/src/a/b.rs")
        );
        assert_eq!(
            resolve("crate::c::Gamma", "k/src/a/b.rs", &krate, &has),
            sym("k/src/c/mod.rs", "Gamma")
        );
        assert_eq!(
            resolve("crate::Top", "k/src/user.rs", &krate, &has),
            sym("k/src/lib.rs", "Top")
        );
        assert_eq!(
            resolve("self::b::Sym", "k/src/a/mod.rs", &krate, &has),
            sym("k/src/a/b.rs", "Sym")
        );
        assert_eq!(
            resolve("super::c::Gamma", "k/src/a/mod.rs", &krate, &has),
            sym("k/src/c/mod.rs", "Gamma")
        );
        assert_eq!(
            resolve("super::c::Delta", "k/src/a/b.rs", &krate, &has),
            sym("k/src/a/c.rs", "Delta")
        );
        assert_eq!(
            resolve("super::Thing", "k/src/a/mod.rs", &krate, &has),
            sym("k/src/lib.rs", "Thing")
        );
        assert_eq!(
            resolve("kk::a::b::Sym", "k/tests/t.rs", &krate, &has),
            sym("k/src/a/b.rs", "Sym")
        );
    }

    #[test]
    fn test_resolve_leaves_external_and_impossible_paths_unresolved() {
        let set = files(&["k/src/lib.rs", "k/tests/t.rs", "k/src/bin/x.rs"]);
        let has = |p: &str| set.contains(p);
        let krate = CrateInfo {
            dir: "k".into(),
            name: Some("kk".into()),
        };
        assert_eq!(
            resolve("serde_json::Value", "k/src/lib.rs", &krate, &has),
            None
        );
        assert_eq!(resolve("std::fs", "k/src/lib.rs", &krate, &has), None);
        assert_eq!(
            resolve("crate::x", "k/tests/t.rs", &krate, &has),
            None,
            "crate:: in a test crate"
        );
        assert_eq!(resolve("crate::x", "k/src/bin/x.rs", &krate, &has), None);
        assert_eq!(
            resolve("super::x", "k/src/lib.rs", &krate, &has),
            None,
            "no parent of the crate root"
        );
        let no_root = |_: &str| false;
        assert_eq!(resolve("crate::x", "k/src/user.rs", &krate, &no_root), None);
    }
}
