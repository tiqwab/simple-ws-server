use anyhow::{Context, Result};
use log::debug;
use simple_ws_server::http::server::Server;
use std::net::SocketAddr;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let addr = SocketAddr::from_str("127.0.0.1:8888").context("Failed to parse server address")?;
    debug!("Server will listen at {}", addr);
    let server = Server::new(addr);
    server.start().await.context("Failed in running server")?;

    Ok(())
}
