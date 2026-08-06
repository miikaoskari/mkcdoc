use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::{merge, model, parse, render};

/// Load `config_path` and run a full build. Shared by the `build` and `serve` subcommands so
/// they can never drift apart.
pub fn build(config_path: &Path) -> Result<Config> {
    let cfg = Config::load(config_path)?;
    build_with_config(&cfg)?;
    Ok(cfg)
}

pub fn build_with_config(cfg: &Config) -> Result<()> {
    let mut source_files = Vec::new();
    for dir in &cfg.source.dirs {
        for path in walk_c_files(dir)? {
            source_files.push(parse::parse_file(&path)?);
        }
    }

    let raw_count: usize = source_files.iter().map(|f| f.functions.len()).sum();
    let functions = merge::merge_functions(&source_files);

    // Structs/enums/typedefs/macros are each scanned from exactly one file (unlike functions,
    // they don't have a separate header-declaration/source-definition split), so no merge step
    // is needed for them — just flatten across files.
    let document = model::Document {
        functions,
        structs: source_files.iter().flat_map(|f| f.structs.iter().cloned()).collect(),
        enums: source_files.iter().flat_map(|f| f.enums.iter().cloned()).collect(),
        typedefs: source_files.iter().flat_map(|f| f.typedefs.iter().cloned()).collect(),
        macros: source_files.iter().flat_map(|f| f.macros.iter().cloned()).collect(),
    };

    println!(
        "Parsed {raw_count} function declaration(s) from {} file(s), merged into {} function(s); \
         {} struct(s), {} enum(s), {} typedef(s), {} macro(s)",
        source_files.len(),
        document.functions.len(),
        document.structs.len(),
        document.enums.len(),
        document.typedefs.len(),
        document.macros.len(),
    );

    render::render_site(cfg, &document)?;
    println!(
        "Wrote {} page(s) to {}",
        count_nav_pages(&cfg.nav),
        cfg.site.output_dir.display()
    );

    Ok(())
}

fn count_nav_pages(entries: &[config::NavEntry]) -> usize {
    entries
        .iter()
        .map(|entry| match entry {
            config::NavEntry::Section { items, .. } => count_nav_pages(items),
            _ => 1,
        })
        .sum()
}

fn walk_c_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("reading directory {}", current.display()))?;
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(path.extension().and_then(|e| e.to_str()), Some("c") | Some("h"))
            {
                files.push(path);
            }
        }
    }
    Ok(files)
}
