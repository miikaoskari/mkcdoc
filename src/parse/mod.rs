use anyhow::{Context, Result};
use std::path::Path;
use tree_sitter::{Node, Parser};

use crate::doxycomment;
use crate::model::{Enum, Field, Function, Macro, SourceFile, Struct, Typedef, Variant};

/// Parse a single C source/header file and extract its documented declarations.
pub fn parse_file(path: &Path) -> Result<SourceFile> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    parse_source(&source, path).with_context(|| format!("parsing {}", path.display()))
}

fn parse_source(source: &str, path: &Path) -> Result<SourceFile> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .context("loading tree-sitter-c grammar")?;
    let tree = parser.parse(source, None).context("tree-sitter parse")?;

    let mut out = SourceFile::default();
    collect_items(tree.root_node(), source, path, &mut out);
    Ok(out)
}

/// Recursively walk the tree looking for top-level declarations. Descends into wrapper nodes
/// (preprocessor conditionals, `extern "C"` blocks, a plain `declaration` that turns out not to
/// be a function, ...) but never into an item it has already extracted, so e.g. local variable
/// declarations inside a function body are never mistaken for top-level items.
fn collect_items(node: Node, source: &str, path: &Path, out: &mut SourceFile) {
    match node.kind() {
        "function_definition" => {
            if let Some(f) = function_from(node, source, path) {
                out.functions.push(f);
            }
            return;
        }
        "declaration" => {
            if let Some(f) = function_from(node, source, path) {
                out.functions.push(f);
                return;
            }
            // Not a function (e.g. a plain variable, or `struct Foo {...} instance;`) — fall
            // through so a nested struct/union/enum specifier still gets extracted below.
        }
        "type_definition" => {
            type_definition_from(node, source, path, out);
            return;
        }
        "struct_specifier" | "union_specifier" => {
            if let Some(s) = struct_from(node, source, path) {
                out.structs.push(s);
            }
            return;
        }
        "enum_specifier" => {
            if let Some(e) = enum_from(node, source, path) {
                out.enums.push(e);
            }
            return;
        }
        "preproc_def" => {
            if !is_include_guard(node, source) {
                out.macros.push(object_macro_from(node, source, path));
            }
            return;
        }
        "preproc_function_def" => {
            out.macros.push(function_macro_from(node, source, path));
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_items(child, source, path, out);
    }
}

// --- functions --------------------------------------------------------------------------

fn function_from(node: Node, source: &str, path: &Path) -> Option<Function> {
    let function_declarator = find_descendant(node, "function_declarator")?;
    let name = declarator_name(function_declarator.child_by_field_name("declarator")?, source)?;
    Some(Function {
        name,
        signature: signature_text(node, source),
        doc: preceding_doc_comment(node, source).map(|raw| doxycomment::parse(&raw)),
        file: path.to_path_buf(),
        line: node.start_position().row + 1,
    })
}

/// The declaration/definition's signature, i.e. everything up to (not including) the body or
/// trailing semicolon.
fn signature_text(node: Node, source: &str) -> String {
    let full = node.utf8_text(source.as_bytes()).unwrap_or_default();
    match full.find('{') {
        Some(idx) => full[..idx].trim_end().to_string(),
        None => full.trim_end_matches(';').trim_end().to_string(),
    }
}

// --- typedefs, and the `typedef struct {...} Name;` idiom -------------------------------

fn type_definition_from(node: Node, source: &str, path: &Path, out: &mut SourceFile) {
    let Some(declarator) = node.child_by_field_name("declarator") else {
        return;
    };
    let Some(name) = declarator_name(declarator, source) else {
        return;
    };
    let doc = preceding_doc_comment(node, source).map(|raw| doxycomment::parse(&raw));
    let line = node.start_position().row + 1;
    let file = path.to_path_buf();

    let Some(type_node) = node.child_by_field_name("type") else {
        return;
    };
    match type_node.kind() {
        "struct_specifier" | "union_specifier" => out.structs.push(Struct {
            name,
            fields: extract_fields(type_node, source),
            doc,
            file,
            line,
        }),
        "enum_specifier" => out.enums.push(Enum {
            name,
            variants: extract_variants(type_node, source),
            doc,
            file,
            line,
        }),
        _ => out.typedefs.push(Typedef {
            name,
            underlying: signature_text(node, source),
            doc,
            file,
            line,
        }),
    }
}

// --- standalone structs/unions/enums (not behind a typedef) ------------------------------

fn struct_from(node: Node, source: &str, path: &Path) -> Option<Struct> {
    let name_node = node.child_by_field_name("name")?;
    // A bare forward declaration (`struct Foo;`) has no body and nothing to document yet.
    node.child_by_field_name("body")?;
    Some(Struct {
        name: name_node.utf8_text(source.as_bytes()).ok()?.to_string(),
        fields: extract_fields(node, source),
        doc: preceding_doc_comment(node, source).map(|raw| doxycomment::parse(&raw)),
        file: path.to_path_buf(),
        line: node.start_position().row + 1,
    })
}

fn enum_from(node: Node, source: &str, path: &Path) -> Option<Enum> {
    let name_node = node.child_by_field_name("name")?;
    node.child_by_field_name("body")?;
    Some(Enum {
        name: name_node.utf8_text(source.as_bytes()).ok()?.to_string(),
        variants: extract_variants(node, source),
        doc: preceding_doc_comment(node, source).map(|raw| doxycomment::parse(&raw)),
        file: path.to_path_buf(),
        line: node.start_position().row + 1,
    })
}

fn extract_fields(spec: Node, source: &str) -> Vec<Field> {
    let Some(body) = spec.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if child.kind() != "field_declaration" {
            continue;
        }
        let Some(declarator) = child.child_by_field_name("declarator") else {
            continue;
        };
        let Some(name) = declarator_name(declarator, source) else {
            continue;
        };
        fields.push(Field {
            name,
            type_text: signature_text(child, source),
            doc: member_doc_comment(child, source),
        });
    }
    fields
}

fn extract_variants(spec: Node, source: &str) -> Vec<Variant> {
    let Some(body) = spec.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut variants = Vec::new();
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if child.kind() != "enumerator" {
            continue;
        }
        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else {
            continue;
        };
        let value = child
            .child_by_field_name("value")
            .and_then(|v| v.utf8_text(source.as_bytes()).ok())
            .map(str::to_string);
        variants.push(Variant {
            name: name.to_string(),
            value,
            doc: member_doc_comment(child, source),
        });
    }
    variants
}

// --- macros --------------------------------------------------------------------------------

/// Whether `node` (a `preproc_def`) is the `#define GUARD` half of a `#ifndef GUARD` / `#define
/// GUARD` header include guard: no value, and its name matches the identifier tested by the
/// `preproc_ifdef` it's directly nested in. These are never meant as documented API surface.
fn is_include_guard(node: Node, source: &str) -> bool {
    if node.child_by_field_name("value").is_some() {
        return false;
    }
    let Some(parent) = node.parent().filter(|p| p.kind() == "preproc_ifdef") else {
        return false;
    };
    let guard_name = text_of(parent, "name", source);
    let macro_name = text_of(node, "name", source);
    guard_name.is_some() && guard_name == macro_name
}

fn object_macro_from(node: Node, source: &str, path: &Path) -> Macro {
    Macro {
        name: text_of(node, "name", source).unwrap_or_default(),
        params: None,
        value: text_of(node, "value", source),
        doc: preceding_doc_comment(node, source).map(|raw| doxycomment::parse(&raw)),
        file: path.to_path_buf(),
        line: node.start_position().row + 1,
    }
}

fn function_macro_from(node: Node, source: &str, path: &Path) -> Macro {
    let params = node.child_by_field_name("parameters").map(|p| {
        let mut cursor = p.walk();
        p.children(&mut cursor)
            .filter(|c| c.kind() == "identifier")
            .filter_map(|c| c.utf8_text(source.as_bytes()).ok().map(str::to_string))
            .collect()
    });
    Macro {
        name: text_of(node, "name", source).unwrap_or_default(),
        params,
        value: text_of(node, "value", source),
        doc: preceding_doc_comment(node, source).map(|raw| doxycomment::parse(&raw)),
        file: path.to_path_buf(),
        line: node.start_position().row + 1,
    }
}

fn text_of(node: Node, field: &str, source: &str) -> Option<String> {
    node.child_by_field_name(field)
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .map(|s| s.trim().to_string())
}

// --- shared helpers --------------------------------------------------------------------------

/// Resolve a declarator down to its identifier, unwrapping pointer/array/function/parenthesized
/// wrappers (e.g. `char *foo(void)`, `int (*Callback)(int, int)`, `char name[32]`).
fn declarator_name(node: Node, source: &str) -> Option<String> {
    if matches!(node.kind(), "identifier" | "type_identifier" | "field_identifier") {
        return node.utf8_text(source.as_bytes()).ok().map(str::to_string);
    }
    if let Some(inner) = node.child_by_field_name("declarator") {
        if let Some(name) = declarator_name(inner, source) {
            return Some(name);
        }
    }
    // Wrapper nodes without a named `declarator` field, e.g. `parenthesized_declarator`.
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| declarator_name(child, source))
}

fn find_descendant<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find_map(|child| find_descendant(child, kind))
}

/// A Doxygen-style comment immediately preceding `node`, documenting what follows it.
fn preceding_doc_comment(node: Node, source: &str) -> Option<String> {
    let sibling = node.prev_sibling()?;
    if sibling.kind() != "comment" {
        return None;
    }
    let text = sibling.utf8_text(source.as_bytes()).ok()?;
    doxygen_leading(text).then(|| text.to_string())
}

fn doxygen_leading(text: &str) -> bool {
    doxycomment::is_doc_comment(text) && !doxycomment::is_trailing_marker(text)
}

/// The doc comment for a struct field / enum variant, which may either precede it in the usual
/// way, or trail it on the same line using the `/**<`-style marker (documenting what came
/// before, e.g. `int x; /**< X coordinate. */`) — the idiom typically used for these members.
fn member_doc_comment(node: Node, source: &str) -> Option<crate::model::DocComment> {
    // `next_named_sibling` (not `next_sibling`) so a `,` between an enumerator and its trailing
    // doc comment (`RED, /**< Red. */`) doesn't shadow the comment.
    if let Some(next) = node.next_named_sibling() {
        if next.kind() == "comment" && next.start_position().row == node.end_position().row {
            if let Ok(text) = next.utf8_text(source.as_bytes()) {
                if doxycomment::is_trailing_marker(text) {
                    return Some(doxycomment::parse(text));
                }
            }
        }
    }
    preceding_doc_comment(node, source).map(|raw| doxycomment::parse(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> SourceFile {
        super::parse_source(source, Path::new("test.c")).unwrap()
    }

    #[test]
    fn extracts_typedef_struct_with_trailing_field_docs() {
        let sf = parse(
            r#"
/** A 2D point. */
typedef struct {
    int x; /**< X coordinate. */
    int y; /**< Y coordinate. */
} Point;
"#,
        );
        assert_eq!(sf.structs.len(), 1);
        let s = &sf.structs[0];
        assert_eq!(s.name, "Point");
        assert_eq!(s.doc.as_ref().unwrap().brief.as_deref(), Some("A 2D point."));
        assert_eq!(s.fields.len(), 2);
        assert_eq!(s.fields[0].name, "x");
        assert_eq!(
            s.fields[0].doc.as_ref().unwrap().brief.as_deref(),
            Some("X coordinate.")
        );
    }

    #[test]
    fn extracts_standalone_struct_and_enum() {
        let sf = parse(
            r#"
struct Named {
    int a;
};

/** Color channel. */
typedef enum {
    RED,   /**< Red. */
    GREEN, /**< Green. */
    BLUE = 5
} Color;
"#,
        );
        assert_eq!(sf.structs.len(), 1);
        assert_eq!(sf.structs[0].name, "Named");
        assert_eq!(sf.enums.len(), 1);
        let e = &sf.enums[0];
        assert_eq!(e.name, "Color");
        assert_eq!(e.variants.len(), 3);
        assert_eq!(e.variants[0].name, "RED");
        assert_eq!(e.variants[0].doc.as_ref().unwrap().brief.as_deref(), Some("Red."));
        assert_eq!(e.variants[2].name, "BLUE");
        assert_eq!(e.variants[2].value.as_deref(), Some("5"));
    }

    #[test]
    fn extracts_macros() {
        let sf = parse(
            r#"
/** Max buffer size. */
#define MAX_SIZE 128

/** Squares x. */
#define SQUARE(x) ((x) * (x))
"#,
        );
        assert_eq!(sf.macros.len(), 2);
        assert_eq!(sf.macros[0].name, "MAX_SIZE");
        assert_eq!(sf.macros[0].params, None);
        assert_eq!(sf.macros[0].value.as_deref(), Some("128"));
        assert_eq!(sf.macros[1].name, "SQUARE");
        assert_eq!(sf.macros[1].params.as_deref(), Some(&["x".to_string()][..]));
    }

    #[test]
    fn skips_header_include_guard() {
        let sf = parse(
            r#"
#ifndef POINT_H
#define POINT_H

/** Max coordinate. */
#define POINT_MAX_COORD 10000

#endif
"#,
        );
        assert_eq!(sf.macros.len(), 1);
        assert_eq!(sf.macros[0].name, "POINT_MAX_COORD");
    }

    #[test]
    fn extracts_function_pointer_typedef() {
        let sf = parse("typedef int (*Callback)(int, int);");
        assert_eq!(sf.typedefs.len(), 1);
        assert_eq!(sf.typedefs[0].name, "Callback");
        assert_eq!(sf.typedefs[0].underlying, "typedef int (*Callback)(int, int)");
    }
}

