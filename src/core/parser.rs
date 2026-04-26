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
        let extension = path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let (language, ts_language) = match extension {
            "py" => ("python", tree_sitter_python::LANGUAGE.into()),
            "rs" => ("rust", tree_sitter_rust::LANGUAGE.into()),
            "ts" | "tsx" => ("typescript", tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            "kt" => ("kotlin", tree_sitter_kotlin_ng::LANGUAGE.into()),
            "sql" => ("sql", tree_sitter_sequel::LANGUAGE.into()),
            "vue" => ("vue", tree_sitter_vue_updated::language().into()),
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
                         (import_from_statement module_name: (dotted_name) @import)",
            "rust" => "((struct_item name: (type_identifier) @name) @struct)
                       ((enum_item name: (type_identifier) @name) @enum)
                       ((function_item name: (identifier) @name) @function)
                       ((trait_item name: (type_identifier) @name) @trait)
                       ((impl_item type: (_) @name) @impl)",
            "typescript" => "((class_declaration name: (type_identifier) @name) @class)
                             ((function_declaration name: (identifier) @name) @function)
                             ((interface_declaration name: (type_identifier) @name) @interface)
                             ((type_alias_declaration name: (type_identifier) @name) @type)
                             ((method_definition name: (property_identifier) @name) @function)
                             (import_statement source: (string (string_fragment) @import))",
            "kotlin" => "((class_declaration) @name) @class
                         ((function_declaration) @name) @function
                         ((_) @import)",
            "sql" => "((identifier) @name) @symbol",
            "vue" => "((tag_name) @name) @component",
            _ => unreachable!(),
        };

        let query = Query::new(&ts_language, query_str)
            .map_err(|e| AppError::Parser(format!("Failed to create query: {}", e)))?;
        
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut kind = String::new();
            let mut line = 0;
            let mut start_byte = 0;
            let mut end_byte = 0;
            let mut is_import = false;

            for capture in m.captures {
                let capture_name = query.capture_names()[capture.index as usize].to_string();
                if capture_name == "import" {
                    let imp = capture.node.utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
                    if !imp.is_empty() {
                        imports.push(imp);
                    }
                    is_import = true;
                    break;
                } else if capture_name == "name" {
                    name = capture.node.utf8_text(content.as_bytes())
                        .unwrap_or("unknown")
                        .to_string();
                } else {
                    kind = capture_name;
                    line = capture.node.start_position().row + 1;
                    start_byte = capture.node.start_byte();
                    end_byte = capture.node.end_byte();
                }
            }
            
            if !is_import && !name.is_empty() && !kind.is_empty() {
                // Clean up name: remove excessive whitespace and truncate
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
                });
            }
        }

        // For Vue, always add a component symbol based on the filename
        if language == "vue" {
            let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("Component");
            symbols.push(Symbol {
                name: file_name.trim_end_matches(".vue").to_string(),
                kind: "component".to_string(),
                line: 1,
                start_byte: 0,
                end_byte: content.len(),
            });
        }

        Ok(FileOutline {
            path: path.to_string_lossy().to_string(),
            language: language.to_string(),
            symbols,
            imports,
        })
    }
}
