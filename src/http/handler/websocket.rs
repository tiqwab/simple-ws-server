use crate::http::handler::Handler;
use crate::http::request::Request;
use async_trait::async_trait;
use std::net::SocketAddr;
use tokio::net::TcpStream;

pub struct WebSocketHandler;

#[async_trait]
impl Handler for WebSocketHandler {
    fn accepts(&self, request: &Request, client_addr: SocketAddr) -> bool {
        let upgrade = request.get_header("Upgrade");
        match upgrade {
            Some(s) if s.to_ascii_lowercase() == "websocket" => true,
            _ => false,
        }
    }

    async fn handle(
        &self,
        request: Request,
        stream: TcpStream,
        client_addr: SocketAddr,
    ) -> anyhow::Result<()> {
        todo!()
    }
}
