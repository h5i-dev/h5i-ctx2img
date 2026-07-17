//! Symbol and import extraction via tree-sitter, with a language-agnostic
//! identifier-bag fallback for everything else.

use crate::hazard;
use crate::tokens;
use crate::types::{Lang, ParsedFile, Symbol, SymbolKind};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

/// Parse one file. Never fails: on grammar/query trouble it degrades to
/// the identifier bag (which is all the relevance machinery strictly needs).
pub fn parse_file(lang: Lang, content: &str) -> ParsedFile {
    let mut out = ParsedFile {
        idents: tokens::ident_bag(content),
        hazards: hazard::scan(content),
        ..Default::default()
    };
    if let Some(spec) = lang_spec(lang) {
        extract_ts(spec, content, &mut out);
    }
    out
}

struct LangSpec {
    language: Language,
    query: &'static OnceLock<Option<Query>>,
    query_src: &'static str,
}

fn lang_spec(lang: Lang) -> Option<LangSpec> {
    static RUST_Q: OnceLock<Option<Query>> = OnceLock::new();
    static PY_Q: OnceLock<Option<Query>> = OnceLock::new();
    static JS_Q: OnceLock<Option<Query>> = OnceLock::new();
    static TS_Q: OnceLock<Option<Query>> = OnceLock::new();
    static GO_Q: OnceLock<Option<Query>> = OnceLock::new();
    static JAVA_Q: OnceLock<Option<Query>> = OnceLock::new();

    match lang {
        Lang::Rust => Some(LangSpec {
            language: tree_sitter_rust::LANGUAGE.into(),
            query: &RUST_Q,
            query_src: RUST_QUERY,
        }),
        Lang::Python => Some(LangSpec {
            language: tree_sitter_python::LANGUAGE.into(),
            query: &PY_Q,
            query_src: PYTHON_QUERY,
        }),
        Lang::JavaScript => Some(LangSpec {
            language: tree_sitter_javascript::LANGUAGE.into(),
            query: &JS_Q,
            query_src: JS_QUERY,
        }),
        Lang::TypeScript => Some(LangSpec {
            language: tree_sitter_typescript::LANGUAGE_TSX.into(),
            query: &TS_Q,
            query_src: TS_QUERY,
        }),
        Lang::Go => Some(LangSpec {
            language: tree_sitter_go::LANGUAGE.into(),
            query: &GO_Q,
            query_src: GO_QUERY,
        }),
        Lang::Java => Some(LangSpec {
            language: tree_sitter_java::LANGUAGE.into(),
            query: &JAVA_Q,
            query_src: JAVA_QUERY,
        }),
        _ => None,
    }
}

fn extract_ts(spec: LangSpec, content: &str, out: &mut ParsedFile) {
    let query = match spec
        .query
        .get_or_init(|| Query::new(&spec.language, spec.query_src).ok())
    {
        Some(q) => q,
        None => return, // query failed to compile against this grammar version
    };

    thread_local! {
        static PARSERS: RefCell<HashMap<usize, Parser>> = RefCell::new(HashMap::new());
    }
    let tree = PARSERS.with(|cell| {
        let mut parsers = cell.borrow_mut();
        // Language has no Hash; key parsers by the query's address (one per lang).
        let parser = parsers
            .entry(query as *const Query as usize)
            .or_insert_with(|| {
                let mut p = Parser::new();
                let _ = p.set_language(&spec.language);
                p
            });
        parser.parse(content, None)
    });
    let tree = match tree {
        Some(t) => t,
        None => return,
    };

    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), content.as_bytes());
    while let Some(m) = matches.next() {
        let mut name: Option<&str> = None;
        let mut def: Option<(&str, tree_sitter::Node)> = None;
        let mut import: Option<tree_sitter::Node> = None;
        for cap in m.captures {
            let cap_name = names[cap.index as usize];
            match cap_name {
                "name" => name = cap.node.utf8_text(content.as_bytes()).ok(),
                "import" => import = Some(cap.node),
                _ => {
                    if let Some(kind) = cap_name.strip_prefix("def.") {
                        def = Some((kind, cap.node));
                    }
                }
            }
        }
        if let (Some(name), Some((kind, node))) = (name, def) {
            if let Some(kind) = symbol_kind(kind) {
                out.symbols.push(Symbol {
                    name: name.to_string(),
                    kind,
                    line: node.start_position().row as u32 + 1,
                    line_end: node.end_position().row as u32 + 1,
                });
            }
        }
        if let Some(node) = import {
            if let Ok(text) = node.utf8_text(content.as_bytes()) {
                let t = text.trim().trim_end_matches(';').to_string();
                if !t.is_empty() && t.len() < 400 {
                    out.imports.push(t);
                }
            }
        }
    }
    out.symbols.sort_by_key(|s| (s.line, s.name.clone()));
    out.imports.dedup();
}

fn symbol_kind(tag: &str) -> Option<SymbolKind> {
    Some(match tag {
        "function" => SymbolKind::Function,
        "method" => SymbolKind::Method,
        "class" => SymbolKind::Class,
        "struct" => SymbolKind::Struct,
        "enum" => SymbolKind::Enum,
        "trait" => SymbolKind::Trait,
        "interface" => SymbolKind::Interface,
        "type" => SymbolKind::Type,
        "const" => SymbolKind::Const,
        _ => return None,
    })
}

const RUST_QUERY: &str = r#"
(function_item name: (identifier) @name) @def.function
(struct_item name: (type_identifier) @name) @def.struct
(enum_item name: (type_identifier) @name) @def.enum
(trait_item name: (type_identifier) @name) @def.trait
(use_declaration) @import
"#;

const PYTHON_QUERY: &str = r#"
(function_definition name: (identifier) @name) @def.function
(class_definition name: (identifier) @name) @def.class
(import_statement) @import
(import_from_statement) @import
"#;

const JS_QUERY: &str = r#"
(function_declaration name: (identifier) @name) @def.function
(class_declaration name: (identifier) @name) @def.class
(method_definition name: (property_identifier) @name) @def.method
(variable_declarator name: (identifier) @name value: (arrow_function)) @def.function
(import_statement) @import
"#;

const TS_QUERY: &str = r#"
(function_declaration name: (identifier) @name) @def.function
(class_declaration name: (type_identifier) @name) @def.class
(method_definition name: (property_identifier) @name) @def.method
(variable_declarator name: (identifier) @name value: (arrow_function)) @def.function
(interface_declaration name: (type_identifier) @name) @def.interface
(type_alias_declaration name: (type_identifier) @name) @def.type
(enum_declaration name: (identifier) @name) @def.enum
(import_statement) @import
"#;

const GO_QUERY: &str = r#"
(function_declaration name: (identifier) @name) @def.function
(method_declaration name: (field_identifier) @name) @def.method
(type_declaration (type_spec name: (type_identifier) @name)) @def.type
(import_declaration) @import
"#;

const JAVA_QUERY: &str = r#"
(class_declaration name: (identifier) @name) @def.class
(interface_declaration name: (identifier) @name) @def.interface
(enum_declaration name: (identifier) @name) @def.enum
(method_declaration name: (identifier) @name) @def.method
(import_declaration) @import
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard against grammar-version drift: every query must compile.
    #[test]
    fn all_queries_compile() {
        for lang in [
            Lang::Rust,
            Lang::Python,
            Lang::JavaScript,
            Lang::TypeScript,
            Lang::Go,
            Lang::Java,
        ] {
            let spec = lang_spec(lang).unwrap();
            Query::new(&spec.language, spec.query_src)
                .unwrap_or_else(|e| panic!("{lang:?} query failed: {e}"));
        }
    }

    #[test]
    fn rust_symbols_and_imports() {
        let src = "use crate::auth::session;\npub fn check_expiry(s: &Session) -> bool { true }\npub struct Session { id: u64 }\n";
        let p = parse_file(Lang::Rust, src);
        let names: Vec<(&str, SymbolKind)> = p
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s.kind))
            .collect();
        assert!(names.contains(&("check_expiry", SymbolKind::Function)));
        assert!(names.contains(&("Session", SymbolKind::Struct)));
        assert_eq!(p.imports, vec!["use crate::auth::session"]);
    }

    #[test]
    fn python_symbols() {
        let src = "import os\nfrom auth.session import Session\n\nclass Manager:\n    def expire(self):\n        pass\n";
        let p = parse_file(Lang::Python, src);
        assert!(p
            .symbols
            .iter()
            .any(|s| s.name == "Manager" && s.kind == SymbolKind::Class));
        assert!(p.symbols.iter().any(|s| s.name == "expire"));
        assert_eq!(p.imports.len(), 2);
    }

    #[test]
    fn typescript_symbols() {
        let src = "import { x } from './util';\nexport interface Session { id: string }\nexport const check = (s: Session) => s.id !== '';\nclass Store { get(k: string) { return k; } }\n";
        let p = parse_file(Lang::TypeScript, src);
        assert!(p
            .symbols
            .iter()
            .any(|s| s.name == "Session" && s.kind == SymbolKind::Interface));
        assert!(p
            .symbols
            .iter()
            .any(|s| s.name == "check" && s.kind == SymbolKind::Function));
        assert!(p
            .symbols
            .iter()
            .any(|s| s.name == "get" && s.kind == SymbolKind::Method));
        assert_eq!(p.imports.len(), 1);
    }
}
