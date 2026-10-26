use clap::Parser;
use robostickies::cli::{self, Cli};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let json = cli.json();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    if let Err(error) = cli::run(cli).await {
        if json {
            eprintln!("{}", serde_json::json!({"error": error}));
        } else {
            eprintln!("{error}");
        }
        std::process::exit(error.exit_code());
    }
}
