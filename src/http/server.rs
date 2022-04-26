use crate::http::handler::echo::EchoHandler;
use crate::http::handler::websocket::WebSocketHandler;
use crate::http::handler::Handler;
use crate::http::request::Request;
use anyhow::{bail, Context, Result};
use futures::TryFutureExt;
use log::{debug, error};
use once_cell::sync::Lazy;
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};

pub struct Server {
    addr: SocketAddr,
}

impl Server {
    pub fn new(addr: SocketAddr) -> Server {
        Server { addr }
    }

    pub async fn start(&self) -> Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        loop {
            let (stream, client_addr) = listener.accept().await?;
            tokio::task::spawn(
                handle_request(stream, client_addr).unwrap_or_else(move |err| {
                    error!("Error in handle_request from {}: {:?}", client_addr, err);
                }),
            );
        }
    }
}

const HANDLERS: Lazy<Arc<Vec<Box<dyn Handler + Send + Sync>>>> =
    Lazy::new(|| Arc::new(vec![Box::new(WebSocketHandler), Box::new(EchoHandler)]));

async fn handle_request(mut stream: TcpStream, client_addr: SocketAddr) -> Result<()> {
    let request = Request::parse(&mut stream).await?;
    debug!("Accepted request: {:?}", request);

    let handlers = Arc::clone(&HANDLERS);
    let handler = handlers
        .iter()
        .find(|handler| handler.accepts(&request, client_addr));
    match handler {
        Some(h) => h.handle(request, stream, client_addr).await,
        None => {
            bail!(
                "Unexpected error: couldn't find appropriate handler for the request: {:?}",
                request
            );
        }
    }
}
