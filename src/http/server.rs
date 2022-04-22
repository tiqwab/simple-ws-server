use crate::http::common::HTTPVersion;
use crate::http::request::{Request, RequestLine};
use crate::http::response::{Response, ResponseBody, ResponseHeaders, StatusLine};
use anyhow::{Context, Result};
use futures::TryFutureExt;
use log::{debug, error};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

async fn handle_request(mut stream: TcpStream, _client_addr: SocketAddr) -> Result<()> {
    let request = Request::parse(&mut stream)
        .await
        .context("Failed to parse request")?;
    debug!("Accepted request: {:?}", request);

    let response = Response::new(
        StatusLine::new(HTTPVersion::V1_1, 200, "OK".to_string()),
        ResponseHeaders::from([("Content-Type", "text/plain"), ("Content-Length", "0")]),
        ResponseBody::new(vec![]),
    );

    stream
        .write(&response.encode())
        .await
        .context("Failed to write response")?;

    Ok(())
}
