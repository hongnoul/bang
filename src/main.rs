mod config;
mod search;
mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Search from the terminal with engine-native !bangs.
///
/// Queries pass through to the search engine verbatim, so bangs like
/// `!gh`, `!w`, `!yt` behave exactly as they do in a browser address bar.
/// Navigational bangs print (or open) the destination URL; everything else
/// prints ranked results. There is deliberately no client-side bang or
/// alias table: the bang vocabulary is owned by the engine, and shell
/// aliases/functions are the supported customization layer.
#[derive(Parser)]
#[command(name = "bang", version, about, max_term_width = 100)]
struct Cli {
    /// Search query. Bangs pass through: `bang '!gh ripgrep'`.
    #[arg(trailing_var_arg = true)]
    query: Vec<String>,

    /// Engine backend: duckduckgo (default) or searxng.
    #[arg(long, short)]
    engine: Option<String>,

    /// Max results to print.
    #[arg(long, short, default_value_t = 8)]
    num: usize,

    /// Open the top result (or bang destination) in the browser.
    #[arg(long, short)]
    open: bool,

    /// Print results as JSON (for scripting/rofi/fzf pipelines).
    #[arg(long)]
    json: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Update bang to the latest release.
    Update {
        /// Check only; do not install.
        #[arg(long)]
        check: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(Command::Update { check }) = cli.command {
        return update::run(check);
    }

    let query = cli.query.join(" ");
    let query = query.trim();
    if query.is_empty() {
        anyhow::bail!("usage: bang <query>   (try: bang '!w rust lifetimes')");
    }

    let config = config::load();
    let engine = cli
        .engine
        .as_deref()
        .unwrap_or(&config.engine)
        .to_ascii_lowercase();

    let outcome = search::run(&engine, query, cli.num, &config)?;
    match outcome {
        search::Outcome::Destination(url) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({ "kind": "destination", "url": url })
                );
            } else {
                println!("{url}");
            }
            if cli.open {
                open_url(&url);
            }
        }
        search::Outcome::Results(results) => {
            if results.is_empty() {
                anyhow::bail!("no results for: {query}");
            }
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                for (i, r) in results.iter().enumerate() {
                    println!("{}. {}\n   {}", i + 1, r.title, r.url);
                    if !r.snippet.is_empty() {
                        println!("   {}", r.snippet);
                    }
                    println!();
                }
            }
            if cli.open {
                if let Some(top) = results.first() {
                    open_url(&top.url);
                }
            }
        }
    }

    // Non-blocking, best-effort background update notice (never fails a search).
    update::maybe_print_update_notice();
    Ok(())
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";
    let _ = std::process::Command::new(opener)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
