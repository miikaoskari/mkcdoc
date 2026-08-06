use crate::model::{DocComment, ParamDoc};

const DOC_MARKERS: [&str; 4] = ["/**", "/*!", "///", "//!"];
const TRAILING_DOC_MARKERS: [&str; 4] = ["/**<", "/*!<", "///<", "//!<"];

/// Whether `text` is a Doxygen-style comment (either the usual form documenting whatever
/// follows it, or the trailing `/**<`-style form documenting whatever precedes it).
pub fn is_doc_comment(text: &str) -> bool {
    DOC_MARKERS.iter().any(|m| text.starts_with(m))
}

/// Whether `text` uses the trailing marker form (`/**<`, `/*!<`, `///<`, `//!<`) that documents
/// the item immediately *before* the comment rather than the one after it — the idiom used for
/// inline struct field and enum variant docs, e.g. `int x; /**< X coordinate. */`.
pub fn is_trailing_marker(text: &str) -> bool {
    TRAILING_DOC_MARKERS.iter().any(|m| text.starts_with(m))
}

/// Strip comment delimiters (`/** */` or `///`, including the trailing-marker `<` variants) and
/// per-line leading `*` decoration.
fn strip_markers(raw: &str) -> String {
    let raw = raw.trim();
    let raw = TRAILING_DOC_MARKERS
        .iter()
        .chain(DOC_MARKERS.iter())
        .find_map(|m| raw.strip_prefix(m))
        .unwrap_or(raw);
    let raw = raw.trim_end_matches("*/");

    raw.lines()
        .map(|line| {
            let line = line.trim();
            line.strip_prefix('*').map(str::trim).unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse a raw Doxygen-style comment into a structured [`DocComment`].
///
/// Untagged text before the first `@tag` becomes the brief (first block) and
/// description (remaining blocks); recognized tags are `@brief`, `@param`,
/// `@return`/`@returns`, and `@see`/`@sa`. Unknown tags are ignored.
pub fn parse(raw: &str) -> DocComment {
    let text = strip_markers(raw);

    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut tag = String::new();
    let mut body = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('@') {
            blocks.push((std::mem::take(&mut tag), std::mem::take(&mut body)));
            let mut parts = rest.splitn(2, char::is_whitespace);
            tag = parts.next().unwrap_or("").to_string();
            body = parts.next().unwrap_or("").trim().to_string();
        } else if trimmed.is_empty() {
            // A blank line ends the current tag's paragraph (Doxygen semantics: only the
            // paragraph immediately after `@tag` belongs to it); anything after reverts to
            // untagged description text.
            if !body.is_empty() {
                blocks.push((std::mem::take(&mut tag), std::mem::take(&mut body)));
            }
            tag.clear();
        } else {
            if !body.is_empty() {
                body.push(' ');
            }
            body.push_str(trimmed);
        }
    }
    blocks.push((tag, body));

    let mut doc = DocComment::default();
    for (tag, content) in blocks {
        if content.is_empty() && tag != "param" {
            continue;
        }
        match tag.as_str() {
            "" => {
                if doc.brief.is_none() {
                    doc.brief = Some(content);
                } else {
                    doc.description = Some(match doc.description.take() {
                        Some(existing) => format!("{existing}\n\n{content}"),
                        None => content,
                    });
                }
            }
            "brief" => doc.brief = Some(content),
            "param" => {
                // Skip an optional Doxygen direction marker, e.g. "@param[in] name ...".
                let content = match content.strip_prefix('[') {
                    Some(rest) => rest.split_once(']').map_or(rest, |(_, after)| after),
                    None => content.as_str(),
                };
                let mut parts = content.trim().splitn(2, char::is_whitespace);
                let name = parts.next().unwrap_or("").to_string();
                let description = parts.next().unwrap_or("").trim().to_string();
                doc.params.push(ParamDoc { name, description });
            }
            "return" | "returns" => doc.returns = Some(content),
            "see" | "sa" => doc.see_also.push(content),
            _ => {}
        }
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_brief_params_and_return() {
        let raw = r#"/**
 * @brief Adds two integers.
 *
 * Performs signed 32-bit addition without overflow checking.
 *
 * @param a First addend.
 * @param b Second addend.
 * @return The sum of a and b.
 */"#;
        let doc = parse(raw);
        assert_eq!(doc.brief.as_deref(), Some("Adds two integers."));
        assert_eq!(
            doc.description.as_deref(),
            Some("Performs signed 32-bit addition without overflow checking.")
        );
        assert_eq!(doc.params.len(), 2);
        assert_eq!(doc.params[0].name, "a");
        assert_eq!(doc.params[0].description, "First addend.");
        assert_eq!(doc.returns.as_deref(), Some("The sum of a and b."));
    }

    #[test]
    fn untagged_first_line_becomes_brief() {
        let raw = "/** Autobrief style summary. */";
        let doc = parse(raw);
        assert_eq!(doc.brief.as_deref(), Some("Autobrief style summary."));
    }
}
