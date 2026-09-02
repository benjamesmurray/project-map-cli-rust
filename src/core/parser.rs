use tree_sitter::{Parser, Query, QueryCursor};
use streaming_iterator::StreamingIterator;
use std::fs;
use std::path::Path;
use crate::error::{AppError, Result};
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub docstring: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileOutline {
    pub path: String,
    pub language: String,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<String>,
}

pub struct CodeParser {
    parser: Parser,
}

impl CodeParser {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
        }
    }

    pub fn parse_file(&mut self, path: &Path) -> Result<FileOutline> {
        let file_name = path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let extension = path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if file_name.starts_with(".env") || extension == "env" {
            return self.parse_env_file(path);
        }

        if extension == "yaml" || extension == "yml" {
            return self.parse_yaml_file(path);
        }

        if extension == "sql" {
            return self.parse_sql_file(path);
        }

        let (language, ts_language) = match extension {
            "py" => ("python", tree_sitter_python::LANGUAGE.into()),
            "rs" => ("rust", tree_sitter_rust::LANGUAGE.into()),
            "ts" => ("typescript", tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            "tsx" => ("typescript", tree_sitter_typescript::LANGUAGE_TSX.into()),
            "kt" => ("kotlin", tree_sitter_kotlin_ng::LANGUAGE.into()),
            "lua" => ("lua", tree_sitter_lua::LANGUAGE.into()),
            "php" => ("php", tree_sitter_php::LANGUAGE_PHP.into()),
            "tf" => ("hcl", tree_sitter_hcl::LANGUAGE.into()),
            "gd" => ("gdscript", tree_sitter_gdscript::LANGUAGE.into()),
            "vue" => ("vue", tree_sitter_vue_updated::language().into()),
            "md" | "json" | "toml" => {
                let lang = match extension {
                    "md" => "markdown",
                    "json" => "json",
                    "toml" => "toml",
                    _ => "text",
                };
                return Ok(FileOutline {
                    path: path.to_string_lossy().to_string(),
                    language: lang.to_string(),
                    symbols: Vec::new(),
                    imports: Vec::new(),
                });
            }
            _ => return Err(AppError::Parser(format!("Unsupported extension: {}", extension))),
        };

        self.parser.set_language(&ts_language)
            .map_err(|e| AppError::Parser(format!("Failed to set language: {}", e)))?;

        let content = fs::read_to_string(path)?;
        let tree = self.parser.parse(&content, None)
            .ok_or_else(|| AppError::Parser("Failed to parse file".to_string()))?;

        let query_str = match language {
            "python" => "((class_definition name: (identifier) @name) @class)
                         ((function_definition name: (identifier) @name) @function)
                         (import_statement (dotted_name) @import)
                         (import_from_statement module_name: (dotted_name) @import)
                         (expression_statement (string) @doc)",
            "rust" => "((struct_item name: (type_identifier) @name) @struct)
                       ((enum_item name: (type_identifier) @name) @enum)
                       ((function_item name: (identifier) @name) @function)
                       ((trait_item name: (type_identifier) @name) @trait)
                       ((impl_item type: (_) @name) @impl)
                       ((line_comment) @doc (#match? @doc \"^///\"))
                       ((block_comment) @doc (#match? @doc \"^/\\\\*\\\\*\"))",
            "typescript" => "((class_declaration name: (type_identifier) @name) @class)
                             ((function_declaration name: (identifier) @name) @function)
                             ((generator_function_declaration name: (identifier) @name) @function)
                             ((interface_declaration name: (type_identifier) @name) @interface)
                             ((type_alias_declaration name: (type_identifier) @name) @type)
                             ((enum_declaration name: (identifier) @name) @enum)
                             ((method_definition name: (property_identifier) @name) @function)
                             ((variable_declarator name: (identifier) @name value: (arrow_function)) @function)
                             ((variable_declarator name: (identifier) @name value: (function_expression)) @function)
                             ((variable_declarator name: (identifier) @name) @variable)
                             (internal_module name: (identifier) @name) @module
                             (import_statement source: (string (string_fragment) @import))
                             (export_statement source: (string (string_fragment) @import))
                             (export_statement (export_clause (export_specifier name: (identifier) @name)) @export)
                             (comment) @doc",
            "kotlin" => "((class_declaration name: (identifier) @name) @class)
                         ((object_declaration name: (identifier) @name) @class)
                         ((companion_object name: (identifier) @name) @class)
                         ((function_declaration name: (identifier) @name) @function)
                         (import (qualified_identifier) @import)
                         (line_comment) @doc
                         (block_comment) @doc",
            "lua" => "((function_declaration name: (_) @name) @function)
                         (function_call name: (identifier) @func_name arguments: (arguments (string content: (_) @import)) (#eq? @func_name \"require\"))
                         (comment) @doc",
            "php" => "((class_declaration name: (name) @name) @class)
                         ((interface_declaration name: (name) @name) @interface)
                         ((trait_declaration name: (name) @name) @trait)
                         ((method_declaration name: (name) @name) @function)
                         ((function_definition name: (name) @name) @function)
                         (namespace_use_clause [(name) (qualified_name)] @import)
                         (comment) @doc
                         (attribute_list) @doc",
            "hcl" => "(block 
                         (identifier) @kind (#match? @kind \"^(resource|data)$\")
                         (string_lit (template_literal) @name1)
                         (string_lit (template_literal) @name2)
                      ) @symbol
                      (block 
                         (identifier) @kind (#match? @kind \"^(module|variable|output)$\")
                         (string_lit (template_literal) @name1)
                      ) @symbol
                      (block 
                         (identifier) @kind (#eq? @kind \"module\")
                         (body 
                            (attribute 
                               (identifier) @attr (#eq? @attr \"source\")
                               (expression (literal_value (string_lit (template_literal) @import)))
                            )
                         )
                      )
                      (comment) @doc",
            "gdscript" => "((class_definition name: (name) @name) @class)
                         ((class_name_statement name: (name) @name) @class)
                         ((function_definition name: (name) @name) @function)
                         ((signal_statement name: (name) @name) @signal)
                         (extends_statement (string) @import)
                         (call
                            (identifier) @func (#match? @func \"^(preload|load)$\")
                            arguments: (arguments (string) @import)
                         )
                         (comment) @doc",
            "vue" => "((tag_name) @name) @component",
            _ => unreachable!(),
        };

        let query = Query::new(&ts_language, query_str)
            .map_err(|e| AppError::Parser(format!("Failed to create query: {}", e)))?;
        
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let mut raw_docs = Vec::new();

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut kind = String::new();
            let mut line = 0;
            let mut start_byte = 0;
            let mut end_byte = 0;
            let mut is_import = false;
            let mut is_doc = false;

            for capture in m.captures {
                let capture_name = query.capture_names()[capture.index as usize].to_string();
                if capture_name == "import" {
                    let mut imp = capture.node.utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if language == "php" {
                        imp = imp.replace("\\", ".");
                    }
                    if language == "gdscript" {
                        imp = imp.replace("res://", "");
                        imp = imp.trim_matches('"').trim_matches('\'').to_string();
                    }
                    if !imp.is_empty() {
                        imports.push(imp);
                    }
                    is_import = true;
                    break;
                } else if capture_name == "doc" {
                    let text = capture.node.utf8_text(content.as_bytes()).unwrap_or("");
                    // For Python, only keep if it's a docstring (this is a heuristic)
                    if language == "python" && !(text.starts_with("\"\"\"") || text.starts_with("'''")) {
                        continue;
                    }
                    
                    raw_docs.push((capture.node.start_position().row + 1, capture.node.start_byte(), capture.node.end_byte(), text.to_string()));
                    is_doc = true;
                    break;
                } else if capture_name.starts_with("name") {
                    let part = capture.node.utf8_text(content.as_bytes())
                        .unwrap_or("unknown")
                        .to_string();
                    if name.is_empty() {
                        name = part;
                    } else {
                        name = format!("{}.{}", name, part);
                    }
                } else {
                    if capture_name == "kind" {
                        kind = capture.node.utf8_text(content.as_bytes())
                            .unwrap_or("unknown")
                            .to_string();
                    } else {
                        kind = capture_name;
                    }
                    line = capture.node.start_position().row + 1;
                    start_byte = capture.node.start_byte();
                    end_byte = capture.node.end_byte();
                }
            }
            
            if !is_import && !is_doc && !name.is_empty() && !kind.is_empty() {
                let mut clean_name = name.replace("\n", " ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                
                if clean_name.chars().count() > 100 {
                    clean_name = format!("{}...", clean_name.chars().take(97).collect::<String>());
                }

                symbols.push(Symbol {
                    name: clean_name,
                    kind,
                    line,
                    start_byte,
                    end_byte,
                    docstring: None,
                    role: None,
                });
            }
        }

        // Second pass: Associate docstrings with symbols
        for symbol in &mut symbols {
            let mut attached_docs = Vec::new();
            for (doc_line, doc_start, doc_end, doc_text) in &raw_docs {
                // Case 1: Docstring is immediately before the symbol (within 2 lines)
                if *doc_line < symbol.line && *doc_line >= symbol.line.saturating_sub(2) {
                    attached_docs.push(doc_text.clone());
                }
                // Case 2: Docstring is inside the symbol's byte range
                else if *doc_start >= symbol.start_byte && *doc_end <= symbol.end_byte {
                    attached_docs.push(doc_text.clone());
                }
            }
            if !attached_docs.is_empty() {
                symbol.docstring = Some(attached_docs.join("\n\n"));
            }
        }

        // Final filtering: remove noisy variables (except for HCL where they are top-level constructs)
        symbols.retain(|s| (s.kind != "variable" || s.docstring.is_some()) || language == "hcl");

        // For Vue, always add a component symbol based on the filename
        if language == "vue" {
            let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("Component");
            symbols.push(Symbol {
                name: file_name.trim_end_matches(".vue").to_string(),
                kind: "component".to_string(),
                line: 1,
                start_byte: 0,
                end_byte: content.len(),
                docstring: None,
                role: None,
            });
        }

        // Scan for Kafka topics, environment lookups, and data contracts in source files
        Self::scan_code_entities(&content, language, &mut symbols);

        symbols.sort_by(|a, b| a.line.cmp(&b.line).then_with(|| a.name.cmp(&b.name)));
        symbols.dedup_by(|a, b| a.name == b.name && a.line == b.line && a.kind == b.kind);

        Ok(FileOutline {
            path: path.to_string_lossy().to_string(),
            language: language.to_string(),
            symbols,
            imports,
        })
    }

    pub fn parse_env_file(&self, path: &Path) -> Result<FileOutline> {
        let content = fs::read_to_string(path)?;
        let mut symbols = Vec::new();
        let mut current_byte = 0;

        for (line_idx, line_str) in content.lines().enumerate() {
            let line_num = line_idx + 1;
            let line_len = line_str.len();
            let trimmed = line_str.trim();

            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                let line_without_export = if trimmed.starts_with("export ") {
                    trimmed[7..].trim()
                } else {
                    trimmed
                };

                if let Some(idx) = line_without_export.find('=') {
                    let key = line_without_export[..idx].trim();
                    let val = line_without_export[idx + 1..].trim().trim_matches(|c| c == '\'' || c == '"');
                    if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        let start_byte = current_byte + line_str.find(key).unwrap_or(0);
                        let end_byte = start_byte + key.len();

                        symbols.push(Symbol {
                            name: key.to_string(),
                            kind: "config_env_var".to_string(),
                            line: line_num,
                            start_byte,
                            end_byte,
                            docstring: None,
                            role: Some("Configuration".to_string()),
                        });

                        if is_kafka_topic_string(val) || key.to_lowercase().contains("topic") {
                            let role = classify_kafka_role(key);
                            let val_start = current_byte + line_str.find(val).unwrap_or(0);
                            let val_end = val_start + val.len();
                            symbols.push(Symbol {
                                name: val.to_string(),
                                kind: "kafka_topic".to_string(),
                                line: line_num,
                                start_byte: val_start,
                                end_byte: val_end,
                                docstring: None,
                                role: Some(role),
                            });
                        }
                    }
                }
            }
            current_byte += line_len + 1;
        }

        Ok(FileOutline {
            path: path.to_string_lossy().to_string(),
            language: "env".to_string(),
            symbols,
            imports: Vec::new(),
        })
    }

    pub fn parse_yaml_file(&self, path: &Path) -> Result<FileOutline> {
        let content = fs::read_to_string(path)?;
        let mut symbols = Vec::new();
        let file_lines: Vec<&str> = content.lines().collect();

        let find_line_and_bytes = |target: &str| -> (usize, usize, usize) {
            for (idx, line) in file_lines.iter().enumerate() {
                if let Some(col) = line.find(target) {
                    let start_byte: usize = file_lines[..idx].iter().map(|l| l.len() + 1).sum::<usize>() + col;
                    let end_byte = start_byte + target.len();
                    return (idx + 1, start_byte, end_byte);
                }
            }
            (1, 0, 0)
        };

        if let Ok(yaml_val) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
            Self::extract_yaml_symbols(&yaml_val, None, &mut symbols, &find_line_and_bytes);
        }

        symbols.sort_by(|a, b| a.line.cmp(&b.line).then_with(|| a.name.cmp(&b.name)));
        symbols.dedup_by(|a, b| a.name == b.name && a.line == b.line);

        Ok(FileOutline {
            path: path.to_string_lossy().to_string(),
            language: "yaml".to_string(),
            symbols,
            imports: Vec::new(),
        })
    }

    fn extract_yaml_symbols<F>(
        val: &serde_yaml::Value,
        parent_key: Option<&str>,
        symbols: &mut Vec<Symbol>,
        finder: &F,
    ) where
        F: Fn(&str) -> (usize, usize, usize),
    {
        match val {
            serde_yaml::Value::Mapping(map) => {
                for (k, v) in map {
                    if let Some(k_str) = k.as_str() {
                        let k_lower = k_str.to_lowercase();

                        if k_lower == "environment" {
                            match v {
                                serde_yaml::Value::Mapping(env_map) => {
                                    for (env_k, env_v) in env_map {
                                        if let Some(env_k_str) = env_k.as_str() {
                                            let (line, start_byte, end_byte) = finder(env_k_str);
                                            symbols.push(Symbol {
                                                name: env_k_str.to_string(),
                                                kind: "config_env_var".to_string(),
                                                line,
                                                start_byte,
                                                end_byte,
                                                docstring: None,
                                                role: Some("Configuration".to_string()),
                                            });

                                            if let Some(env_v_str) = env_v.as_str() {
                                                if is_kafka_topic_string(env_v_str) || env_k_str.to_lowercase().contains("topic") {
                                                    let role = classify_kafka_role(env_k_str);
                                                    let (t_line, t_start, t_end) = finder(env_v_str);
                                                    symbols.push(Symbol {
                                                        name: env_v_str.to_string(),
                                                        kind: "kafka_topic".to_string(),
                                                        line: t_line,
                                                        start_byte: t_start,
                                                        end_byte: t_end,
                                                        docstring: None,
                                                        role: Some(role),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                                serde_yaml::Value::Sequence(env_seq) => {
                                    for item in env_seq {
                                        if let Some(item_str) = item.as_str() {
                                            if let Some(eq_idx) = item_str.find('=') {
                                                let env_k = item_str[..eq_idx].trim();
                                                let env_v = item_str[eq_idx + 1..].trim();
                                                let (line, start_byte, end_byte) = finder(env_k);
                                                symbols.push(Symbol {
                                                    name: env_k.to_string(),
                                                    kind: "config_env_var".to_string(),
                                                    line,
                                                    start_byte,
                                                    end_byte,
                                                    docstring: None,
                                                    role: Some("Configuration".to_string()),
                                                });

                                                if is_kafka_topic_string(env_v) || env_k.to_lowercase().contains("topic") {
                                                    let role = classify_kafka_role(env_k);
                                                    let (t_line, t_start, t_end) = finder(env_v);
                                                    symbols.push(Symbol {
                                                        name: env_v.to_string(),
                                                        kind: "kafka_topic".to_string(),
                                                        line: t_line,
                                                        start_byte: t_start,
                                                        end_byte: t_end,
                                                        docstring: None,
                                                        role: Some(role),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        } else if k_lower.contains("topic") {
                            let role = classify_kafka_role(k_str);
                            match v {
                                serde_yaml::Value::String(s) => {
                                    let (line, start_byte, end_byte) = finder(s);
                                    symbols.push(Symbol {
                                        name: s.clone(),
                                        kind: "kafka_topic".to_string(),
                                        line,
                                        start_byte,
                                        end_byte,
                                        docstring: None,
                                        role: Some(role),
                                    });
                                }
                                serde_yaml::Value::Sequence(seq) => {
                                    for item in seq {
                                        if let Some(s) = item.as_str() {
                                            let (line, start_byte, end_byte) = finder(s);
                                            symbols.push(Symbol {
                                                name: s.to_string(),
                                                kind: "kafka_topic".to_string(),
                                                line,
                                                start_byte,
                                                end_byte,
                                                docstring: None,
                                                role: Some(role.clone()),
                                            });
                                        }
                                    }
                                }
                                _ => {
                                    Self::extract_yaml_symbols(v, Some(k_str), symbols, finder);
                                }
                            }
                        } else {
                            let (line, start_byte, end_byte) = finder(k_str);
                            symbols.push(Symbol {
                                name: k_str.to_string(),
                                kind: "config".to_string(),
                                line,
                                start_byte,
                                end_byte,
                                docstring: None,
                                role: None,
                            });
                            Self::extract_yaml_symbols(v, Some(k_str), symbols, finder);
                        }
                    }
                }
            }
            serde_yaml::Value::Sequence(seq) => {
                for item in seq {
                    Self::extract_yaml_symbols(item, parent_key, symbols, finder);
                }
            }
            serde_yaml::Value::String(s) => {
                if is_kafka_topic_string(s) {
                    let role = parent_key.map(classify_kafka_role).unwrap_or_else(|| "Configuration".to_string());
                    let (line, start_byte, end_byte) = finder(s);
                    symbols.push(Symbol {
                        name: s.clone(),
                        kind: "kafka_topic".to_string(),
                        line,
                        start_byte,
                        end_byte,
                        docstring: None,
                        role: Some(role),
                    });
                }
            }
            _ => {}
        }
    }

    pub fn parse_sql_file(&self, path: &Path) -> Result<FileOutline> {
        let content = fs::read_to_string(path)?;
        let mut symbols = Vec::new();
        let file_lines: Vec<&str> = content.lines().collect();

        for (line_idx, line) in file_lines.iter().enumerate() {
            let line_num = line_idx + 1;
            let trimmed = line.trim();
            let upper = trimmed.to_uppercase();

            if upper.starts_with("CREATE TABLE") {
                let rest = trimmed["CREATE TABLE".len()..].trim_start();
                let rest_upper = rest.to_uppercase();
                let table_part = if rest_upper.starts_with("IF NOT EXISTS") {
                    rest["IF NOT EXISTS".len()..].trim_start()
                } else {
                    rest
                };
                let table_name = table_part
                    .split(|c: char| c.is_whitespace() || c == '(' || c == ';')
                    .next()
                    .unwrap_or("")
                    .trim_matches(|c| c == '"' || c == '`' || c == '\'');

                if !table_name.is_empty() {
                    let start_col = line.find(table_name).unwrap_or(0);
                    let start_byte: usize = file_lines[..line_idx].iter().map(|l| l.len() + 1).sum::<usize>() + start_col;
                    let end_byte = start_byte + table_name.len();

                    symbols.push(Symbol {
                        name: table_name.to_string(),
                        kind: "database_table".to_string(),
                        line: line_num,
                        start_byte,
                        end_byte,
                        docstring: None,
                        role: Some("Definition".to_string()),
                    });
                }
            } else if upper.starts_with("ALTER TABLE") {
                let rest = trimmed["ALTER TABLE".len()..].trim_start();
                let rest_upper = rest.to_uppercase();
                let table_part = if rest_upper.starts_with("IF EXISTS") {
                    rest["IF EXISTS".len()..].trim_start()
                } else {
                    rest
                };
                let table_name = table_part
                    .split(|c: char| c.is_whitespace() || c == ';')
                    .next()
                    .unwrap_or("")
                    .trim_matches(|c| c == '"' || c == '`' || c == '\'');

                if !table_name.is_empty() {
                    let start_col = line.find(table_name).unwrap_or(0);
                    let start_byte: usize = file_lines[..line_idx].iter().map(|l| l.len() + 1).sum::<usize>() + start_col;
                    let end_byte = start_byte + table_name.len();

                    symbols.push(Symbol {
                        name: table_name.to_string(),
                        kind: "database_table".to_string(),
                        line: line_num,
                        start_byte,
                        end_byte,
                        docstring: None,
                        role: Some("Configuration".to_string()),
                    });
                }
            } else if upper.starts_with("CREATE VIEW") || upper.starts_with("CREATE OR REPLACE VIEW") {
                let prefix_len = if upper.starts_with("CREATE OR REPLACE VIEW") {
                    "CREATE OR REPLACE VIEW".len()
                } else {
                    "CREATE VIEW".len()
                };
                let rest = trimmed[prefix_len..].trim_start();
                let view_name = rest
                    .split(|c: char| c.is_whitespace() || c == '(' || c == ';' || c == '\n')
                    .next()
                    .unwrap_or("")
                    .trim_matches(|c| c == '"' || c == '`' || c == '\'');

                if !view_name.is_empty() {
                    let start_col = line.find(view_name).unwrap_or(0);
                    let start_byte: usize = file_lines[..line_idx].iter().map(|l| l.len() + 1).sum::<usize>() + start_col;
                    let end_byte = start_byte + view_name.len();

                    symbols.push(Symbol {
                        name: view_name.to_string(),
                        kind: "database_table".to_string(),
                        line: line_num,
                        start_byte,
                        end_byte,
                        docstring: None,
                        role: Some("Definition".to_string()),
                    });
                }
            } else if upper.starts_with("CREATE FUNCTION") || upper.starts_with("CREATE OR REPLACE FUNCTION") {
                let prefix_len = if upper.starts_with("CREATE OR REPLACE FUNCTION") {
                    "CREATE OR REPLACE FUNCTION".len()
                } else {
                    "CREATE FUNCTION".len()
                };
                let rest = trimmed[prefix_len..].trim_start();
                let func_name = rest
                    .split(|c: char| c.is_whitespace() || c == '(' || c == ';' || c == '\n')
                    .next()
                    .unwrap_or("")
                    .trim_matches(|c| c == '"' || c == '`' || c == '\'');

                if !func_name.is_empty() {
                    let start_col = line.find(func_name).unwrap_or(0);
                    let start_byte: usize = file_lines[..line_idx].iter().map(|l| l.len() + 1).sum::<usize>() + start_col;
                    let end_byte = start_byte + func_name.len();

                    symbols.push(Symbol {
                        name: func_name.to_string(),
                        kind: "function".to_string(),
                        line: line_num,
                        start_byte,
                        end_byte,
                        docstring: None,
                        role: Some("Definition".to_string()),
                    });
                }
            }
        }

        Ok(FileOutline {
            path: path.to_string_lossy().to_string(),
            language: "sql".to_string(),
            symbols,
            imports: Vec::new(),
        })
    }

    pub fn scan_code_entities(content: &str, _language: &str, symbols: &mut Vec<Symbol>) {
        let file_lines: Vec<&str> = content.lines().collect();
        let mut current_byte = 0;

        for (line_idx, line) in file_lines.iter().enumerate() {
            let line_num = line_idx + 1;
            let line_len = line.len();

            // 1. Scan for Kafka topics in quoted strings
            let mut search_from = 0;
            while search_from < line.len() {
                let quote_start = line[search_from..].find(|c| c == '"' || c == '\'');
                if let Some(rel_start) = quote_start {
                    let start_idx = search_from + rel_start;
                    let quote_char = line.as_bytes()[start_idx] as char;
                    if let Some(rel_end) = line[start_idx + 1..].find(quote_char) {
                        let end_idx = start_idx + 1 + rel_end;
                        let literal = &line[start_idx + 1..end_idx];

                        if is_kafka_topic_string(literal) {
                            let role = classify_kafka_code_role(line);
                            let start_byte = current_byte + start_idx + 1;
                            let end_byte = current_byte + end_idx;
                            symbols.push(Symbol {
                                name: literal.to_string(),
                                kind: "kafka_topic".to_string(),
                                line: line_num,
                                start_byte,
                                end_byte,
                                docstring: None,
                                role: Some(role),
                            });
                        }
                        search_from = end_idx + 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            // 2. Scan for environment variable lookups
            // Kotlin/Java: System.getenv("VAR")
            if line.contains("System.getenv") {
                if let Some(var_name) = extract_between_quotes(line, "System.getenv") {
                    if is_env_var_name(&var_name) {
                        let col = line.find(&var_name).unwrap_or(0);
                        let start_byte = current_byte + col;
                        symbols.push(Symbol {
                            name: var_name.clone(),
                            kind: "config_env_var".to_string(),
                            line: line_num,
                            start_byte,
                            end_byte: start_byte + var_name.len(),
                            docstring: None,
                            role: Some("Reference".to_string()),
                        });
                    }
                }
            }

            // Python: os.environ["VAR"], os.environ.get("VAR"), os.getenv("VAR")
            if line.contains("os.environ") || line.contains("os.getenv") {
                let trigger = if line.contains("os.environ") { "os.environ" } else { "os.getenv" };
                if let Some(var_name) = extract_between_quotes(line, trigger) {
                    if is_env_var_name(&var_name) {
                        let col = line.find(&var_name).unwrap_or(0);
                        let start_byte = current_byte + col;
                        symbols.push(Symbol {
                            name: var_name.clone(),
                            kind: "config_env_var".to_string(),
                            line: line_num,
                            start_byte,
                            end_byte: start_byte + var_name.len(),
                            docstring: None,
                            role: Some("Reference".to_string()),
                        });
                    }
                }
            }

            // Rust: env::var("VAR")
            if line.contains("env::var") {
                if let Some(var_name) = extract_between_quotes(line, "env::var") {
                    if is_env_var_name(&var_name) {
                        let col = line.find(&var_name).unwrap_or(0);
                        let start_byte = current_byte + col;
                        symbols.push(Symbol {
                            name: var_name.clone(),
                            kind: "config_env_var".to_string(),
                            line: line_num,
                            start_byte,
                            end_byte: start_byte + var_name.len(),
                            docstring: None,
                            role: Some("Reference".to_string()),
                        });
                    }
                }
            }

            // TypeScript / Node: process.env.VAR or process.env["VAR"]
            if line.contains("process.env") {
                if let Some(idx) = line.find("process.env") {
                    let rest = &line[idx + "process.env".len()..];
                    if rest.starts_with('.') {
                        let var_name = rest[1..]
                            .split(|c: char| !c.is_alphanumeric() && c != '_')
                            .next()
                            .unwrap_or("");
                        if is_env_var_name(var_name) {
                            let col = line.find(var_name).unwrap_or(0);
                            let start_byte = current_byte + col;
                            symbols.push(Symbol {
                                name: var_name.to_string(),
                                kind: "config_env_var".to_string(),
                                line: line_num,
                                start_byte,
                                end_byte: start_byte + var_name.len(),
                                docstring: None,
                                role: Some("Reference".to_string()),
                            });
                        }
                    } else if let Some(var_name) = extract_between_quotes(line, "process.env") {
                        if is_env_var_name(&var_name) {
                            let col = line.find(&var_name).unwrap_or(0);
                            let start_byte = current_byte + col;
                            symbols.push(Symbol {
                                name: var_name.clone(),
                                kind: "config_env_var".to_string(),
                                line: line_num,
                                start_byte,
                                end_byte: start_byte + var_name.len(),
                                docstring: None,
                                role: Some("Reference".to_string()),
                            });
                        }
                    }
                }
            }

            current_byte += line_len + 1;
        }
    }
}

pub fn is_env_var_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && s.chars().any(|c| c.is_ascii_uppercase())
}

pub fn extract_between_quotes(line: &str, after_prefix: &str) -> Option<String> {
    if let Some(idx) = line.find(after_prefix) {
        let rest = &line[idx + after_prefix.len()..];
        if let Some(q_start) = rest.find(|c| c == '"' || c == '\'') {
            let q_char = rest.as_bytes()[q_start] as char;
            if let Some(q_end) = rest[q_start + 1..].find(q_char) {
                return Some(rest[q_start + 1..q_start + 1 + q_end].to_string());
            }
        }
    }
    None
}

pub fn classify_kafka_code_role(line: &str) -> String {
    let lower = line.to_lowercase();
    if lower.contains(".send(")
        || lower.contains(".produce(")
        || lower.contains(".to(")
        || lower.contains("output-topic")
        || lower.contains("output_topic")
        || lower.contains("outputtopic")
        || lower.contains("sink")
        || lower.contains("producer")
    {
        "Producer".to_string()
    } else if lower.contains(".subscribe(")
        || lower.contains(".from(")
        || lower.contains(".consume(")
        || lower.contains("globalktable(")
        || lower.contains("input-topics")
        || lower.contains("input_topics")
        || lower.contains("inputtopic")
        || lower.contains("input_topic")
        || lower.contains("source")
        || lower.contains("consumer")
    {
        "Consumer".to_string()
    } else {
        "Reference".to_string()
    }
}

pub fn is_kafka_topic_string(s: &str) -> bool {
    let s = s.trim().trim_matches(|c| c == '\'' || c == '"');
    if s.len() < 3 || s.contains(' ') || s.contains('/') || s.contains('\\') || s.contains(':') || s.contains('{') || s.contains('$') {
        return false;
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() >= 2 {
        let all_valid = parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
        if all_valid {
            if parts.last().map(|last| last.starts_with('v') && last.len() > 1 && last[1..].chars().all(|c| c.is_ascii_digit())).unwrap_or(false) {
                return true;
            }
            if parts.len() >= 3 {
                return true;
            }
        }
    }
    false
}

pub fn classify_kafka_role(context: &str) -> String {
    let lower = context.to_lowercase();
    if lower.contains("output-topic")
        || lower.contains("output_topic")
        || lower.contains("outputtopic")
        || lower.contains("sink")
        || lower.contains("producer")
        || lower.contains("produce")
        || lower.contains(".send(")
        || lower.contains(".to(")
    {
        "Producer".to_string()
    } else if lower.contains("input-topics")
        || lower.contains("input_topics")
        || lower.contains("input-topic")
        || lower.contains("input_topic")
        || lower.contains("inputtopic")
        || lower.contains("source")
        || lower.contains("consumer")
        || lower.contains("consume")
        || lower.contains(".subscribe(")
        || lower.contains(".from(")
        || lower.contains("globalktable(")
    {
        "Consumer".to_string()
    } else if lower.contains(".yml") || lower.contains(".yaml") || lower.contains("docker-compose") || lower.contains("config") {
        "Configuration".to_string()
    } else {
        "Reference".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_kafka_topic_string() {
        assert!(is_kafka_topic_string("wde.labels.phase.v1"));
        assert!(is_kafka_topic_string("wde.bars.raw.v5"));
        assert!(is_kafka_topic_string("orders.v1"));
        assert!(is_kafka_topic_string("com.example.service"));
        assert!(!is_kafka_topic_string("simple_word"));
        assert!(!is_kafka_topic_string("path/to/file.kt"));
        assert!(!is_kafka_topic_string("foo.rs"));
    }

    #[test]
    fn test_classify_kafka_role() {
        assert_eq!(classify_kafka_role("output-topic"), "Producer");
        assert_eq!(classify_kafka_role("KAFKA_SINK_TOPIC"), "Producer");
        assert_eq!(classify_kafka_role("input-topics"), "Consumer");
        assert_eq!(classify_kafka_role("consumer_topic"), "Consumer");
        assert_eq!(classify_kafka_role("docker-compose.yml"), "Configuration");
        assert_eq!(classify_kafka_role("some_random_context"), "Reference");
    }

    #[test]
    fn test_parse_env_file() {
        let temp_path = std::env::temp_dir().join("test_pm_env.env");
        let env_content = "# Comment line\nDATABASE_URL=postgres://localhost/db\nexport KAFKA_OUTPUT_TOPIC=wde.labels.phase.v1\nKAFKA_INPUT_TOPIC=\"wde.bars.raw.v5\"\n";
        std::fs::write(&temp_path, env_content).unwrap();

        let parser = CodeParser::new();
        let outline = parser.parse_env_file(&temp_path).unwrap();
        std::fs::remove_file(&temp_path).ok();

        let env_vars: Vec<&str> = outline.symbols.iter().filter(|s| s.kind == "config_env_var").map(|s| s.name.as_str()).collect();
        assert!(env_vars.contains(&"DATABASE_URL"));
        assert!(env_vars.contains(&"KAFKA_OUTPUT_TOPIC"));
        assert!(env_vars.contains(&"KAFKA_INPUT_TOPIC"));

        let topics: Vec<&Symbol> = outline.symbols.iter().filter(|s| s.kind == "kafka_topic").collect();
        assert_eq!(topics.len(), 2);
        let out_topic = topics.iter().find(|s| s.name == "wde.labels.phase.v1").unwrap();
        assert_eq!(out_topic.role.as_deref(), Some("Producer"));
        let in_topic = topics.iter().find(|s| s.name == "wde.bars.raw.v5").unwrap();
        assert_eq!(in_topic.role.as_deref(), Some("Consumer"));
    }

    #[test]
    fn test_parse_yaml_file() {
        let temp_path = std::env::temp_dir().join("test_pm_compose.yml");
        let yaml_content = r#"
version: '3.8'
services:
  app:
    environment:
      - KAFKA_BOOTSTRAP=localhost:9092
      - OUTPUT_TOPIC=wde.labels.phase.v1
    input-topics:
      - wde.bars.raw.v5
"#;
        std::fs::write(&temp_path, yaml_content).unwrap();

        let parser = CodeParser::new();
        let outline = parser.parse_yaml_file(&temp_path).unwrap();
        std::fs::remove_file(&temp_path).ok();

        assert!(outline.symbols.iter().any(|s| s.name == "KAFKA_BOOTSTRAP" && s.kind == "config_env_var"));
        assert!(outline.symbols.iter().any(|s| s.name == "OUTPUT_TOPIC" && s.kind == "config_env_var"));

        let out_topic = outline.symbols.iter().find(|s| s.name == "wde.labels.phase.v1").unwrap();
        assert_eq!(out_topic.kind, "kafka_topic");
        assert_eq!(out_topic.role.as_deref(), Some("Producer"));

        let in_topic = outline.symbols.iter().find(|s| s.name == "wde.bars.raw.v5").unwrap();
        assert_eq!(in_topic.kind, "kafka_topic");
        assert_eq!(in_topic.role.as_deref(), Some("Consumer"));
    }

    #[test]
    fn test_parse_sql_file() {
        let temp_path = std::env::temp_dir().join("test_pm_schema.sql");
        let sql_content = r#"
CREATE TABLE IF NOT EXISTS public.levels_history (
    id UUID PRIMARY KEY,
    price NUMERIC NOT NULL
);

ALTER TABLE public.levels_history ADD COLUMN volume NUMERIC;

CREATE VIEW active_levels AS SELECT * FROM public.levels_history;

CREATE FUNCTION calculate_level() RETURNS void AS $$ BEGIN END; $$ LANGUAGE plpgsql;
"#;
        std::fs::write(&temp_path, sql_content).unwrap();

        let parser = CodeParser::new();
        let outline = parser.parse_sql_file(&temp_path).unwrap();
        std::fs::remove_file(&temp_path).ok();

        let tables: Vec<&Symbol> = outline.symbols.iter().filter(|s| s.kind == "database_table").collect();
        assert!(tables.iter().any(|s| s.name == "public.levels_history" && s.role.as_deref() == Some("Definition")));
        assert!(tables.iter().any(|s| s.name == "public.levels_history" && s.role.as_deref() == Some("Configuration")));
        assert!(tables.iter().any(|s| s.name == "active_levels" && s.role.as_deref() == Some("Definition")));

        let fns: Vec<&Symbol> = outline.symbols.iter().filter(|s| s.kind == "function").collect();
        assert!(fns.iter().any(|s| s.name == "calculate_level"));
    }

    #[test]
    fn test_scan_code_entities_kotlin_and_python() {
        let mut symbols = Vec::new();
        let kt_code = r#"
package com.example

fun configure() {
    val topic = "wde.labels.phase.v1"
    builder.stream("wde.bars.raw.v5").to("wde.labels.phase.v1")
    val envKey = System.getenv("KAFKA_BROKERS")
}
"#;
        CodeParser::scan_code_entities(kt_code, "kotlin", &mut symbols);

        let topic_names: Vec<&str> = symbols.iter().filter(|s| s.kind == "kafka_topic").map(|s| s.name.as_str()).collect();
        assert!(topic_names.contains(&"wde.labels.phase.v1"));
        assert!(topic_names.contains(&"wde.bars.raw.v5"));

        let env_vars: Vec<&str> = symbols.iter().filter(|s| s.kind == "config_env_var").map(|s| s.name.as_str()).collect();
        assert!(env_vars.contains(&"KAFKA_BROKERS"));

        let mut py_symbols = Vec::new();
        let py_code = r#"
import os

def send_events():
    producer.send("wde.labels.phase.v1", value=b"data")
    db_pass = os.environ["DB_PASSWORD"]
    api_key = os.getenv("API_KEY")
"#;
        CodeParser::scan_code_entities(py_code, "python", &mut py_symbols);

        let out_topic = py_symbols.iter().find(|s| s.name == "wde.labels.phase.v1").unwrap();
        assert_eq!(out_topic.kind, "kafka_topic");
        assert_eq!(out_topic.role.as_deref(), Some("Producer"));

        let py_env_vars: Vec<&str> = py_symbols.iter().filter(|s| s.kind == "config_env_var").map(|s| s.name.as_str()).collect();
        assert!(py_env_vars.contains(&"DB_PASSWORD"));
        assert!(py_env_vars.contains(&"API_KEY"));
    }
}

