use std::collections::BTreeMap;
use std::path::Path;

use crate::model::{Function, SourceFile};

/// Merge per-file function declarations into one entry per public (non-`static`) function name,
/// combining a header's prototype/doc with a `.c` file's definition/doc when both exist.
///
/// Doxygen convention: a function declared in a header and defined in a `.c` file is one logical
/// entity, not two — this reproduces that. `static` functions have internal linkage, so the same
/// name reused in two unrelated `.c` files must NOT be merged into a single entry.
pub fn merge_functions(files: &[SourceFile]) -> Vec<Function> {
    let mut public: BTreeMap<String, Function> = BTreeMap::new();
    let mut file_local: Vec<Function> = Vec::new();

    for file in files {
        for func in &file.functions {
            if is_static(&func.signature) {
                file_local.push(func.clone());
                continue;
            }
            public
                .entry(func.name.clone())
                .and_modify(|existing| merge_into(existing, func))
                .or_insert_with(|| func.clone());
        }
    }

    let mut result: Vec<Function> = public.into_values().collect();
    result.extend(file_local);
    result.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.file.cmp(&b.file)));
    result
}

fn is_static(signature: &str) -> bool {
    signature.trim_start().starts_with("static ")
}

/// Fold `incoming` into `existing`: a header's signature is preferred as the canonical one when
/// present, and whichever side has a doc comment wins (header wins if both do).
fn merge_into(existing: &mut Function, incoming: &Function) {
    let existing_is_header = is_header(&existing.file);
    let incoming_is_header = is_header(&incoming.file);

    if incoming_is_header && !existing_is_header {
        existing.signature = incoming.signature.clone();
        existing.file = incoming.file.clone();
        existing.line = incoming.line;
    }

    match (&existing.doc, &incoming.doc) {
        (None, Some(_)) => existing.doc = incoming.doc.clone(),
        (Some(_), Some(_)) if incoming_is_header && !existing_is_header => {
            existing.doc = incoming.doc.clone();
        }
        _ => {}
    }
}

fn is_header(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("h")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DocComment;
    use std::path::PathBuf;

    fn func(name: &str, file: &str, signature: &str, doc: Option<&str>) -> Function {
        Function {
            name: name.to_string(),
            signature: signature.to_string(),
            doc: doc.map(|brief| DocComment {
                brief: Some(brief.to_string()),
                ..Default::default()
            }),
            file: PathBuf::from(file),
            line: 1,
        }
    }

    #[test]
    fn merges_header_declaration_with_c_definition() {
        let files = vec![
            SourceFile {
                functions: vec![func(
                    "mu_add",
                    "mathutil.h",
                    "int mu_add(int a, int b)",
                    Some("Adds two integers."),
                )],
                ..Default::default()
            },
            SourceFile {
                functions: vec![func(
                    "mu_add",
                    "mathutil.c",
                    "int mu_add(int a, int b)",
                    None,
                )],
                ..Default::default()
            },
        ];

        let merged = merge_functions(&files);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].file, PathBuf::from("mathutil.h"));
        assert_eq!(merged[0].doc.as_ref().unwrap().brief.as_deref(), Some("Adds two integers."));
    }

    #[test]
    fn does_not_merge_static_functions_across_files() {
        let files = vec![
            SourceFile {
                functions: vec![func("helper", "a.c", "static int helper(void)", None)],
                ..Default::default()
            },
            SourceFile {
                functions: vec![func("helper", "b.c", "static int helper(void)", None)],
                ..Default::default()
            },
        ];

        let merged = merge_functions(&files);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn header_doc_wins_when_both_are_documented() {
        let files = vec![
            SourceFile {
                functions: vec![func(
                    "mu_add",
                    "mathutil.c",
                    "int mu_add(int a, int b)",
                    Some("Definition-site brief."),
                )],
                ..Default::default()
            },
            SourceFile {
                functions: vec![func(
                    "mu_add",
                    "mathutil.h",
                    "int mu_add(int a, int b)",
                    Some("Header brief."),
                )],
                ..Default::default()
            },
        ];

        let merged = merge_functions(&files);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].doc.as_ref().unwrap().brief.as_deref(), Some("Header brief."));
    }
}
