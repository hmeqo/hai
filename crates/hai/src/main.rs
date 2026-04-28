use clap::Parser;
use hai::cli::Cli;

#[tokio::main]
async fn main() {
    if let Err(err) = Cli::parse().execute().await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
