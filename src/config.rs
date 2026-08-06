use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub site: SiteConfig,
    pub source: SourceConfig,
    #[serde(default)]
    pub content: ContentConfig,
    #[serde(default)]
    pub nav: Vec<NavEntry>,
}

#[derive(Debug, Deserialize)]
pub struct SiteConfig {
    pub name: String,
    pub output_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct SourceConfig {
    /// Directories to scan (recursively) for `.c`/`.h` files.
    pub dirs: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct ContentConfig {
    /// Directory hand-written Markdown pages are resolved against.
    #[serde(default = "default_content_dir")]
    pub dir: PathBuf,
}

impl Default for ContentConfig {
    fn default() -> Self {
        Self {
            dir: default_content_dir(),
        }
    }
}

fn default_content_dir() -> PathBuf {
    PathBuf::from("content")
}

/// One entry of the explicit `nav` tree. A `Page` is a hand-written Markdown file; `Section`
/// groups entries under a heading; `ApiReference` marks where the generated API docs go.
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NavEntry {
    Page {
        title: String,
        /// Path to a Markdown file, relative to `content.dir`.
        path: PathBuf,
    },
    Section {
        title: String,
        items: Vec<NavEntry>,
    },
    ApiReference {
        title: String,
        #[serde(default = "default_api_path")]
        path: PathBuf,
    },
}

fn default_api_path() -> PathBuf {
    PathBuf::from("api.html")
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing config file {}", path.display()))
    }
}
