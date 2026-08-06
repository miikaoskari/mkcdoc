mod config;
mod content;
mod doxycomment;
mod merge;
mod model;
mod parse;
mod pipeline;
mod render;
mod serve;

use anyhow::Result;
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
        #[arg(short, long, default_value = "mkcdoc.toml")]
        config: PathBuf,
    },
    /// Build, then serve the site locally with live reload on source/content/config changes
    Serve {
        #[arg(short, long, default_value = "mkcdoc.toml")]
        config: PathBuf,
        #[arg(short, long, default_value_t = 8000)]
        port: u16,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { config } => pipeline::build(&config).map(|_| ()),
        Command::Serve { config, port } => serve::run(&config, port),
    }
}
