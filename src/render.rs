use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tera::Tera;

use crate::config::{Config, NavEntry};
use crate::model::{DocComment, Document, Enum, Function, Macro, Struct, Typedef};

const BASE_TEMPLATE: &str = include_str!("templates/base.html.tera");
const PAGE_TEMPLATE: &str = include_str!("templates/page.html.tera");
const API_TEMPLATE: &str = include_str!("templates/api.html.tera");

#[derive(Serialize)]
struct FunctionView {
    name: String,
    signature: String,
    line: usize,
    brief: Option<String>,
    description: Option<String>,
    params: Vec<ParamView>,
    returns: Option<String>,
}

#[derive(Serialize)]
struct ParamView {
    name: String,
    description: String,
}

#[derive(Serialize)]
struct StructView {
    name: String,
    anchor: String,
    line: usize,
    brief: Option<String>,
    description: Option<String>,
    fields: Vec<MemberView>,
}

#[derive(Serialize)]
struct EnumView {
    name: String,
    anchor: String,
    line: usize,
    brief: Option<String>,
    description: Option<String>,
    variants: Vec<MemberView>,
}

/// A struct field or enum variant: `type_text`/`value` are mutually exclusive depending on
/// which one this represents.
#[derive(Serialize)]
struct MemberView {
    name: String,
    type_text: Option<String>,
    value: Option<String>,
    brief: Option<String>,
}

#[derive(Serialize)]
struct TypedefView {
    name: String,
    anchor: String,
    line: usize,
    underlying: String,
    brief: Option<String>,
    description: Option<String>,
}

#[derive(Serialize)]
struct MacroView {
    name: String,
    line: usize,
    params: Option<Vec<String>>,
    value: Option<String>,
    brief: Option<String>,
    description: Option<String>,
}

/// Everything extracted from one source file, grouped for the API reference page so readers can
/// see at a glance which header/`.c` file a function, struct, etc. belongs to — rather than
/// having to notice a small per-item file label buried in each entry.
#[derive(Serialize, Default)]
struct FileGroupView {
    file: String,
    anchor: String,
    functions: Vec<FunctionView>,
    structs: Vec<StructView>,
    enums: Vec<EnumView>,
    typedefs: Vec<TypedefView>,
    macros: Vec<MacroView>,
}

/// An HTML-id-safe anchor for a file path, e.g. `src/mathutil.h` -> `src-mathutil-h`.
fn anchor_for(file: &str) -> String {
    file.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

fn group_by_file(document: &Document) -> Vec<FileGroupView> {
    let symbols = build_symbol_table(document);
    let mut groups: std::collections::BTreeMap<String, FileGroupView> =
        std::collections::BTreeMap::new();

    macro_rules! group_of {
        ($file:expr) => {{
            let file = $file;
            groups.entry(file.clone()).or_insert_with(|| FileGroupView {
                file: file.clone(),
                anchor: anchor_for(&file),
                ..Default::default()
            })
        }};
    }

    for f in &document.functions {
        group_of!(f.file.display().to_string()).functions.push(function_view(f, &symbols));
    }
    for s in &document.structs {
        group_of!(s.file.display().to_string()).structs.push(struct_view(s, &symbols));
    }
    for e in &document.enums {
        group_of!(e.file.display().to_string()).enums.push(enum_view(e));
    }
    for t in &document.typedefs {
        group_of!(t.file.display().to_string()).typedefs.push(typedef_view(t, &symbols));
    }
    for m in &document.macros {
        group_of!(m.file.display().to_string()).macros.push(macro_view(m));
    }

    groups.into_values().collect()
}

/// Maps a type-like symbol's name (struct/enum/typedef) to the anchor id its own entry is
/// rendered with, so [`linkify`] can turn a plain mention of that name — e.g. `Point` inside the
/// text `const Point *p` — into a link to its definition on the same page.
///
/// Function and macro names are deliberately not included: this links *type* references, not
/// arbitrary identifier mentions. On a same-name collision across categories (legal in C, since
/// struct/enum tags and typedef names are technically separate namespaces, though rare in
/// practice) the first one found wins — structs, then enums, then typedefs.
type SymbolTable = HashMap<String, String>;

fn build_symbol_table(document: &Document) -> SymbolTable {
    let mut table = SymbolTable::new();
    for s in &document.structs {
        table.entry(s.name.clone()).or_insert_with(|| type_anchor(&s.name));
    }
    for e in &document.enums {
        table.entry(e.name.clone()).or_insert_with(|| type_anchor(&e.name));
    }
    for t in &document.typedefs {
        table.entry(t.name.clone()).or_insert_with(|| type_anchor(&t.name));
    }
    table
}

/// An HTML-id-safe anchor for a type-like symbol. C identifiers are already id-safe
/// (`[A-Za-z_][A-Za-z0-9_]*`), so no sanitizing is needed beyond the `type-` prefix.
fn type_anchor(name: &str) -> String {
    format!("type-{name}")
}

/// Re-emit `text` (a C signature, field type, or typedef's underlying type — never
/// user-controlled prose) as HTML, turning any whole-word mention of a name in `symbols` into a
/// link to that symbol's anchor. `exclude` skips linking a symbol's own name within its own
/// defining text — e.g. `typedef void (*PointVisitor)(...)` would otherwise link `PointVisitor`
/// to itself, right at the line that defines it. Must be paired with `| safe` in the template,
/// since the output already contains real `<a>` markup.
fn linkify(text: &str, symbols: &SymbolTable, exclude: Option<&str>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();

    while let Some(&(start, c)) = chars.peek() {
        if c.is_ascii_alphabetic() || c == '_' {
            let mut end = start + c.len_utf8();
            chars.next();
            while let Some(&(idx, ch)) = chars.peek() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    end = idx + ch.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let token = &text[start..end];
            if Some(token) == exclude {
                out.push_str(token);
            } else {
                match symbols.get(token) {
                    Some(anchor) => out.push_str(&format!("<a href=\"#{anchor}\">{token}</a>")),
                    None => out.push_str(token),
                }
            }
        } else {
            escape_html_char(c, &mut out);
            chars.next();
        }
    }
    out
}

fn escape_html_char(c: char, out: &mut String) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        other => out.push(other),
    }
}

fn brief_of(doc: &Option<DocComment>) -> Option<String> {
    doc.as_ref().and_then(|d| d.brief.clone())
}

fn description_of(doc: &Option<DocComment>) -> Option<String> {
    doc.as_ref().and_then(|d| d.description.clone())
}

/// A nav entry as seen by the templates: `Section.items` recurses, but the top navbar itself only
/// renders two levels deep (top-level entries, and one level of dropdown items) — deeper nesting
/// is accepted by the config but won't show up in the built-in theme yet.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NavView {
    Page { title: String, href: String },
    Section { title: String, items: Vec<NavView> },
}

/// Flatten a (possibly nested) source-relative path into a single output filename, e.g.
/// `guide/install.md` -> `guide-install.html`. Every generated page lives directly under the
/// site root, so nav links are always plain filenames — no directory-relative path math needed.
fn page_slug(rel_path: &Path) -> String {
    let flat = rel_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("-");
    let mut slug = PathBuf::from(flat);
    slug.set_extension("html");
    slug.to_string_lossy().into_owned()
}

fn build_nav(entries: &[NavEntry]) -> Vec<NavView> {
    entries
        .iter()
        .map(|entry| match entry {
            NavEntry::Page { title, path } => NavView::Page {
                title: title.clone(),
                href: page_slug(path),
            },
            NavEntry::ApiReference { title, path } => NavView::Page {
                title: title.clone(),
                href: page_slug(path),
            },
            NavEntry::Section { title, items } => NavView::Section {
                title: title.clone(),
                items: build_nav(items),
            },
        })
        .collect()
}

/// Render the whole site: every `nav` entry becomes a page under `site.output_dir`, sharing the
/// same top navbar. `Page` entries are hand-written Markdown; `ApiReference` renders `document`.
pub fn render_site(cfg: &Config, document: &Document) -> Result<()> {
    let mut tera = Tera::default();
    tera.add_raw_templates(vec![
        ("base.html", BASE_TEMPLATE),
        ("page.html", PAGE_TEMPLATE),
        ("api.html", API_TEMPLATE),
    ])
    .context("compiling built-in templates")?;

    std::fs::create_dir_all(&cfg.site.output_dir)
        .with_context(|| format!("creating output dir {}", cfg.site.output_dir.display()))?;
    crate::content::copy_assets(&cfg.content.dir, &cfg.site.output_dir)?;

    let nav = build_nav(&cfg.nav);
    render_nav_entries(&cfg.nav, &tera, &nav, cfg, document)
}

fn render_nav_entries(
    entries: &[NavEntry],
    tera: &Tera,
    nav: &[NavView],
    cfg: &Config,
    document: &Document,
) -> Result<()> {
    for entry in entries {
        match entry {
            NavEntry::Page { title, path } => render_content_page(tera, nav, cfg, title, path)?,
            NavEntry::ApiReference { title, path } => {
                render_api_page(tera, nav, cfg, document, title, path)?
            }
            NavEntry::Section { items, .. } => {
                render_nav_entries(items, tera, nav, cfg, document)?
            }
        }
    }
    Ok(())
}

fn render_content_page(
    tera: &Tera,
    nav: &[NavView],
    cfg: &Config,
    title: &str,
    rel_path: &Path,
) -> Result<()> {
    let body_html = crate::content::render_markdown_file(&cfg.content.dir.join(rel_path))?;

    let mut ctx = tera::Context::new();
    ctx.insert("site_name", &cfg.site.name);
    ctx.insert("nav", nav);
    ctx.insert("page_title", title);
    ctx.insert("body_html", &body_html);

    write_page(tera, "page.html", &ctx, &cfg.site.output_dir, &page_slug(rel_path))
}

fn render_api_page(
    tera: &Tera,
    nav: &[NavView],
    cfg: &Config,
    document: &Document,
    title: &str,
    rel_path: &Path,
) -> Result<()> {
    let mut ctx = tera::Context::new();
    ctx.insert("site_name", &cfg.site.name);
    ctx.insert("nav", nav);
    ctx.insert("page_title", title);
    ctx.insert("file_groups", &group_by_file(document));

    write_page(tera, "api.html", &ctx, &cfg.site.output_dir, &page_slug(rel_path))
}

fn write_page(
    tera: &Tera,
    template: &str,
    ctx: &tera::Context,
    output_dir: &Path,
    slug: &str,
) -> Result<()> {
    let html = tera
        .render(template, ctx)
        .with_context(|| format!("rendering {template} -> {slug}"))?;
    let out_path = output_dir.join(slug);
    std::fs::write(&out_path, html).with_context(|| format!("writing {}", out_path.display()))?;
    Ok(())
}

fn function_view(f: &Function, symbols: &SymbolTable) -> FunctionView {
    FunctionView {
        name: f.name.clone(),
        signature: linkify(&f.signature, symbols, None),
        line: f.line,
        brief: brief_of(&f.doc),
        description: description_of(&f.doc),
        params: f
            .doc
            .as_ref()
            .map(|d| {
                d.params
                    .iter()
                    .map(|p| ParamView {
                        name: p.name.clone(),
                        description: p.description.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        returns: f.doc.as_ref().and_then(|d| d.returns.clone()),
    }
}

fn struct_view(s: &Struct, symbols: &SymbolTable) -> StructView {
    StructView {
        name: s.name.clone(),
        anchor: type_anchor(&s.name),
        line: s.line,
        brief: brief_of(&s.doc),
        description: description_of(&s.doc),
        fields: s
            .fields
            .iter()
            .map(|f| MemberView {
                name: f.name.clone(),
                type_text: Some(linkify(&f.type_text, symbols, None)),
                value: None,
                brief: brief_of(&f.doc),
            })
            .collect(),
    }
}

fn enum_view(e: &Enum) -> EnumView {
    EnumView {
        name: e.name.clone(),
        anchor: type_anchor(&e.name),
        line: e.line,
        brief: brief_of(&e.doc),
        description: description_of(&e.doc),
        variants: e
            .variants
            .iter()
            .map(|v| MemberView {
                name: v.name.clone(),
                type_text: None,
                value: v.value.clone(),
                brief: brief_of(&v.doc),
            })
            .collect(),
    }
}

fn typedef_view(t: &Typedef, symbols: &SymbolTable) -> TypedefView {
    TypedefView {
        name: t.name.clone(),
        anchor: type_anchor(&t.name),
        line: t.line,
        underlying: linkify(&t.underlying, symbols, Some(&t.name)),
        brief: brief_of(&t.doc),
        description: description_of(&t.doc),
    }
}

fn macro_view(m: &Macro) -> MacroView {
    MacroView {
        name: m.name.clone(),
        line: m.line,
        params: m.params.clone(),
        value: m.value.clone(),
        brief: brief_of(&m.doc),
        description: description_of(&m.doc),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linkifies_whole_word_matches_only() {
        let mut symbols = SymbolTable::new();
        symbols.insert("Point".to_string(), "type-Point".to_string());

        // Whole-word match gets linked.
        assert_eq!(
            linkify("void f(const Point *p)", &symbols, None),
            "void f(const <a href=\"#type-Point\">Point</a> *p)"
        );

        // Substring inside a longer identifier must NOT be linked.
        assert_eq!(linkify("PointStyle s", &symbols, None), "PointStyle s");

        // No match at all: text passes through unchanged.
        assert_eq!(linkify("int x", &symbols, None), "int x");
    }

    #[test]
    fn linkify_excludes_a_symbols_own_defining_occurrence() {
        let mut symbols = SymbolTable::new();
        symbols.insert("PointVisitor".to_string(), "type-PointVisitor".to_string());
        symbols.insert("Point".to_string(), "type-Point".to_string());

        let out = linkify(
            "typedef void (*PointVisitor)(const Point *p)",
            &symbols,
            Some("PointVisitor"),
        );
        assert_eq!(
            out,
            "typedef void (*PointVisitor)(const <a href=\"#type-Point\">Point</a> *p)"
        );
    }

    #[test]
    fn linkify_escapes_html_special_characters() {
        let symbols = SymbolTable::new();
        assert_eq!(linkify("a < b && b > c", &symbols, None), "a &lt; b &amp;&amp; b &gt; c");
    }
}
