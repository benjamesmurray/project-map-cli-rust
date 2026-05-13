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
            "ts" => ("typescript", tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            "tsx" => ("typescript", tree_sitter_typescript::LANGUAGE_TSX.into()),
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
                         (import_from_statement module_name: (dotted_name) @import)
                         (expression_statement (string) @doc)",
            "rust" => "((struct_item name: (type_identifier) @name) @struct)
                       ((enum_item name: (type_identifier) @name) @enum)
                       ((function_item name: (identifier) @name) @function)
                       ((trait_item name: (type_identifier) @name) @trait)
                       ((impl_item type: (_) @name) @impl)
                       (line_doc_comment) @doc
                       (block_doc_comment) @doc",
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
                    let imp = capture.node.utf8_text(content.as_bytes())
                        .unwrap_or("")
                        .to_string();
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

        // Final filtering: remove noisy variables
        symbols.retain(|s| s.kind != "variable" || s.docstring.is_some());

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
