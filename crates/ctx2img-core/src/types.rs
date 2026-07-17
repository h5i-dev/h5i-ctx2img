use serde::{Deserialize, Serialize};

/// Index into [`crate::Analysis::files`]. Stable only within one analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FileId(pub u32);

impl FileId {
    pub fn idx(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    Markdown,
    Config,
    Shell,
    Other,
}

impl Lang {
    pub fn from_path(path: &str) -> Lang {
        let ext = path.rsplit('.').next().unwrap_or("");
        match ext {
            "rs" => Lang::Rust,
            "py" | "pyi" => Lang::Python,
            "js" | "jsx" | "mjs" | "cjs" => Lang::JavaScript,
            "ts" | "tsx" | "mts" | "cts" => Lang::TypeScript,
            "go" => Lang::Go,
            "java" => Lang::Java,
            "md" | "rst" | "txt" | "adoc" => Lang::Markdown,
            "toml" | "yaml" | "yml" | "json" | "ini" | "cfg" | "lock" | "xml" => Lang::Config,
            "sh" | "bash" | "zsh" => Lang::Shell,
            _ => Lang::Other,
        }
    }

    /// Short tag used in legends and rosters.
    pub fn tag(self) -> &'static str {
        match self {
            Lang::Rust => "rs",
            Lang::Python => "py",
            Lang::JavaScript => "js",
            Lang::TypeScript => "ts",
            Lang::Go => "go",
            Lang::Java => "java",
            Lang::Markdown => "doc",
            Lang::Config => "cfg",
            Lang::Shell => "sh",
            Lang::Other => "misc",
        }
    }
}

/// One ingested repository file (relative path, always `/`-separated).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub lang: Lang,
    pub size: u64,
    pub loc: u32,
    /// blake3 of content, hex.
    pub hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Type,
    Const,
}

impl SymbolKind {
    pub fn tag(self) -> &'static str {
        match self {
            SymbolKind::Function => "fn",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Interface => "iface",
            SymbolKind::Type => "type",
            SymbolKind::Const => "const",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// 1-based line of the definition.
    pub line: u32,
    pub line_end: u32,
}

/// Per-file parse output; cacheable by content hash.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedFile {
    pub symbols: Vec<Symbol>,
    /// Raw import strings as written in the source.
    pub imports: Vec<String>,
    /// Identifier bag: token -> occurrence count (sub-token split applied).
    pub idents: Vec<(String, u32)>,
    /// Bitflags from [`crate::hazard`].
    pub hazards: u8,
}

impl ParsedFile {
    pub fn ident_count(&self) -> u64 {
        self.idents.iter().map(|(_, c)| *c as u64).sum()
    }
}
