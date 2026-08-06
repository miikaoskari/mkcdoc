use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct DocComment {
    pub brief: Option<String>,
    pub description: Option<String>,
    pub params: Vec<ParamDoc>,
    pub returns: Option<String>,
    pub see_also: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParamDoc {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub signature: String,
    pub doc: Option<DocComment>,
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    /// Full field text (type + declarator, e.g. `char *name`, `int count`), no trailing `;`.
    pub type_text: String,
    pub doc: Option<DocComment>,
}

#[derive(Debug, Clone)]
pub struct Struct {
    pub name: String,
    pub fields: Vec<Field>,
    pub doc: Option<DocComment>,
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub value: Option<String>,
    pub doc: Option<DocComment>,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub name: String,
    pub variants: Vec<Variant>,
    pub doc: Option<DocComment>,
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Typedef {
    pub name: String,
    /// Full `typedef ...;` text (minus the trailing `;`), e.g. `typedef int (*Callback)(int, int)`.
    pub underlying: String,
    pub doc: Option<DocComment>,
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Macro {
    pub name: String,
    /// `None` for an object-like macro (`#define FOO 1`), `Some` (possibly empty) for a
    /// function-like macro (`#define FOO(x) ...`).
    pub params: Option<Vec<String>>,
    pub value: Option<String>,
    pub doc: Option<DocComment>,
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SourceFile {
    pub functions: Vec<Function>,
    pub structs: Vec<Struct>,
    pub enums: Vec<Enum>,
    pub typedefs: Vec<Typedef>,
    pub macros: Vec<Macro>,
}

/// Everything mkcdoc extracted from the source tree, ready to render.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub functions: Vec<Function>,
    pub structs: Vec<Struct>,
    pub enums: Vec<Enum>,
    pub typedefs: Vec<Typedef>,
    pub macros: Vec<Macro>,
}
