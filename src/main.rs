use anyhow::{Context, Result};
use clap::Parser;
use log::{debug, warn};
use simple_ws_server::http::server::Server;
use simple_ws_server::settings::Settings;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;

/// Simple HTTP and WebSocket server
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Path to settings
    #[clap(short, long, default_value = "settings-default.toml")]
    config_path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    let settings = Settings::load(args.config_path).context("Failed to load settings")?;

    let addr = SocketAddr::from_str(&format!(
        "{}:{}",
        settings.http().addr(),
        settings.http().port()
    ))
    .context("Failed to parse server address")?;

    debug!("Server will listen at {}", addr);
    let server = Server::new(addr, settings);
    server.start().await.context("Failed in running server")?;

    Ok(())
}
