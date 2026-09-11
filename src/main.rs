mod config;
mod content;
mod doxycomment;
mod merge;
mod model;
mod parse;
mod pipeline;
mod render;
mod serve;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mkcdoc",
    version,
    about = "Generate a static docs site from Doxygen-style C comments and hand-written Markdown"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the static site
    Build {
        /// Project directory containing the config file (defaults to the current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long, default_value = "mkcdoc.toml")]
        config: PathBuf,
    },
    /// Build, then serve the site locally with live reload on source/content/config changes
    Serve {
        /// Project directory containing the config file (defaults to the current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long, default_value = "mkcdoc.toml")]
        config: PathBuf,
        #[arg(short, long, default_value_t = 8000)]
        port: u16,
    },
}

/// Move the process into `path` so every path in the config (`source.dirs`, `content.dir`,
/// `site.output_dir`) and `--config` itself resolve relative to the target project, regardless
/// of where `mkcdoc` was invoked from.
fn enter_project_dir(path: &PathBuf) -> Result<()> {
    std::env::set_current_dir(path)
        .with_context(|| format!("changing directory to {}", path.display()))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { path, config } => {
            enter_project_dir(&path)?;
            pipeline::build(&config).map(|_| ())
        }
        Command::Serve { path, config, port } => {
            enter_project_dir(&path)?;
            serve::run(&config, port)
        }
    }
}
