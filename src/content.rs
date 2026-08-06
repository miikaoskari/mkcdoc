use anyhow::{Context, Result};
use pulldown_cmark::{html, CodeBlockKind, Event, Parser, Tag, TagEnd};
use std::path::Path;

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
    let events = rewrite_mermaid_blocks(Parser::new(markdown));
    let mut html_out = String::new();
    html::push_html(&mut html_out, events.into_iter());
    html_out
}

/// Rewrite fenced ` ```mermaid ` code blocks into `<pre class="mermaid">` elements (emitted as
/// raw HTML) so the client-side Mermaid renderer picks them up, instead of letting them render
/// as an inert `<pre><code class="language-mermaid">` block like any other fenced language.
fn rewrite_mermaid_blocks(parser: Parser<'_>) -> Vec<Event<'_>> {
    let mut out = Vec::new();
    let mut in_mermaid = false;
    let mut buffer = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) if &*lang == "mermaid" => {
                in_mermaid = true;
                buffer.clear();
            }
            Event::End(TagEnd::CodeBlock) if in_mermaid => {
                in_mermaid = false;
                let diagram = format!("<pre class=\"mermaid\">{}</pre>", escape_html(&buffer));
                out.push(Event::Html(diagram.into()));
            }
            Event::Text(text) if in_mermaid => buffer.push_str(&text),
            other => out.push(other),
        }
    }
    out
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
        let html = render_markdown("```c\nint x = 1;\n```");
        assert!(html.contains("<pre><code class=\"language-c\">"));
    }
}
