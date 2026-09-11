use anyhow::{Context, Result};
use pulldown_cmark::{html, CodeBlockKind, Event, Parser, Tag, TagEnd};
use std::path::Path;
use tree_sitter_highlight::{Highlight, HighlightConfiguration, Highlighter, HtmlRenderer};

/// Highlight capture names recognized in `tree-sitter-c`'s bundled `highlights.scm`, in the
/// order their indices are looked up by [`highlight_c`]. Names not styled in CSS (e.g.
/// `operator`, `variable`) still get a `tok-*` class but render as plain text.
const HIGHLIGHT_NAMES: &[&str] = &[
    "variable",
    "constant",
    "keyword",
    "operator",
    "delimiter",
    "string",
    "number",
    "property",
    "label",
    "type",
    "function",
    "function.special",
    "comment",
];

/// Render a hand-written Markdown content page to an HTML fragment (no `<html>`/`<body>`
/// wrapper — that's supplied by the page template).
pub fn render_markdown_file(path: &Path) -> Result<String> {
    let markdown = std::fs::read_to_string(path)
        .with_context(|| format!("reading content page {}", path.display()))?;
    Ok(render_markdown(&markdown))
}

/// Copy every non-Markdown file (images, etc.) from `content_dir` into `output_dir`, preserving
/// their relative path. Unlike `.md` pages — which are flattened to a single output filename so
/// page-to-page links never need directory-relative math — assets keep their original nested
/// path, since every generated page lives at the output root and can reach e.g. `guide/foo.png`
/// with a single, page-independent relative link. So: reference images in Markdown using a path
/// relative to `content_dir`, not relative to the `.md` file itself.
pub fn copy_assets(content_dir: &Path, output_dir: &Path) -> Result<()> {
    if !content_dir.exists() {
        return Ok(());
    }
    let mut stack = vec![content_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .with_context(|| format!("reading directory {}", dir.display()))?;
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                continue;
            }
            let rel = path.strip_prefix(content_dir).unwrap_or(&path);
            let dest = output_dir.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating directory {}", parent.display()))?;
            }
            std::fs::copy(&path, &dest)
                .with_context(|| format!("copying {} to {}", path.display(), dest.display()))?;
        }
    }
    Ok(())
}

fn render_markdown(markdown: &str) -> String {
    let events = rewrite_code_blocks(Parser::new(markdown));
    let mut html_out = String::new();
    html::push_html(&mut html_out, events.into_iter());
    html_out
}

/// How a fenced code block should be rewritten, decided from its language tag.
enum FenceKind {
    /// ` ```mermaid ` -> `<pre class="mermaid">`, picked up by the client-side Mermaid renderer.
    Mermaid,
    /// ` ```c ` -> syntax-highlighted at build time via `tree-sitter-highlight`.
    C,
}

/// Rewrite fenced ` ```mermaid ` and ` ```c ` code blocks into raw HTML, replacing the inert
/// `<pre><code class="language-...">` pulldown-cmark would otherwise emit for them. Every other
/// fenced language passes through unchanged.
fn rewrite_code_blocks(parser: Parser<'_>) -> Vec<Event<'_>> {
    let mut out = Vec::new();
    let mut fence: Option<FenceKind> = None;
    let mut buffer = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => match &*lang {
                "mermaid" => {
                    fence = Some(FenceKind::Mermaid);
                    buffer.clear();
                }
                "c" => {
                    fence = Some(FenceKind::C);
                    buffer.clear();
                }
                _ => out.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang)))),
            },
            Event::End(TagEnd::CodeBlock) => match fence.take() {
                Some(FenceKind::Mermaid) => {
                    let diagram = format!("<pre class=\"mermaid\">{}</pre>", escape_html(&buffer));
                    out.push(Event::Html(diagram.into()));
                }
                Some(FenceKind::C) => {
                    let code = highlight_c(&buffer).unwrap_or_else(|| escape_html(&buffer));
                    let block = format!("<pre><code class=\"language-c\">{code}</code></pre>");
                    out.push(Event::Html(block.into()));
                }
                None => out.push(Event::End(TagEnd::CodeBlock)),
            },
            Event::Text(text) if fence.is_some() => buffer.push_str(&text),
            other => out.push(other),
        }
    }
    out
}

/// Syntax-highlight a `c` fenced code block into a string of `<span class="tok-...">` markup
/// (already HTML-escaped), using `tree-sitter-c`'s bundled highlight query — the same grammar
/// the API-reference parser uses, so no extra language dependency is needed. Returns `None` if
/// the code fails to highlight (e.g. it doesn't actually parse as C), leaving the caller to fall
/// back to plain escaped text.
fn highlight_c(code: &str) -> Option<String> {
    let mut config = HighlightConfiguration::new(
        tree_sitter_c::LANGUAGE.into(),
        "c",
        tree_sitter_c::HIGHLIGHT_QUERY,
        "",
        "",
    )
    .ok()?;
    config.configure(HIGHLIGHT_NAMES);

    let classes: Vec<String> = HIGHLIGHT_NAMES
        .iter()
        .map(|name| format!("class=\"tok-{}\"", name.replace('.', "-")))
        .collect();

    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(&config, code.as_bytes(), None, None, |_| None)
        .ok()?;

    let mut renderer = HtmlRenderer::new();
    renderer
        .render(events, code.as_bytes(), &|highlight: Highlight, out: &mut Vec<u8>| {
            out.extend_from_slice(classes[highlight.0].as_bytes());
        })
        .ok()?;

    String::from_utf8(renderer.html).ok()
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_mermaid_fence_as_pre_mermaid() {
        let html = render_markdown("```mermaid\ngraph TD;\n  A-->B;\n```");
        assert_eq!(
            html.trim(),
            "<pre class=\"mermaid\">graph TD;\n  A--&gt;B;\n</pre>"
        );
    }

    #[test]
    fn other_fenced_languages_render_normally() {
        let html = render_markdown("```python\nx = 1\n```");
        assert!(html.contains("<pre><code class=\"language-python\">"));
    }

    #[test]
    fn c_fence_is_syntax_highlighted() {
        let html = render_markdown("```c\nint add(int a, int b) {\n    return a + b;\n}\n```");
        assert!(html.starts_with("<pre><code class=\"language-c\">"));
        assert!(html.contains("<span class=\"tok-type\">int</span>"));
        assert!(html.contains("<span class=\"tok-keyword\">return</span>"));
        assert!(html.contains("<span class=\"tok-function\">add</span>"));
    }
}
