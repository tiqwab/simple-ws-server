use anyhow::{Context, Result};
use log::{debug, warn};
use simple_ws_server::http::server::Server;
use simple_ws_server::settings::Settings;
use std::net::SocketAddr;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let settings = Settings::load().unwrap_or_else(|err| {
        warn!("Failed to load settings, use default values: {:?}", err);
        Settings::default()
    });

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
