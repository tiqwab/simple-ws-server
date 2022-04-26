use crate::http::request::Request;
use anyhow::Result;
use async_trait::async_trait;
use std::net::SocketAddr;
use tokio::net::TcpStream;

pub mod echo;
pub mod websocket;

#[async_trait]
pub trait Handler {
    /// Return true if the handler target this request.
    fn accepts(&self, request: &Request, client_addr: SocketAddr) -> bool;

    async fn handle(
        &self,
        request: Request,
        mut stream: TcpStream,
        client_addr: SocketAddr,
    ) -> Result<()>;
}
