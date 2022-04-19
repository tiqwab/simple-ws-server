use crate::http::request::{Request, RequestLine};
use anyhow::{Context, Result};
use futures::TryFutureExt;
use log::{debug, error};
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
    // FIXME: The current implementation performs one read (not considering Content-Length)
    let mut buf = Vec::new();
    stream
        .read_buf(&mut buf)
        .await
        .context("Failed to read data")?;
    let s = String::from_utf8_lossy(&buf).to_string();

    let request_line = RequestLine::parse(s.split("\r\n").next().unwrap())?;
    debug!("Accepted request: {:?}", request_line);

    // let request = Request::parse(&s).context("Failed to parse request")?;
    // debug!("Accepted request: {:?}", request);

    stream
        .write(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 0\r\n\r\n".as_bytes(),
        )
        .await
        .context("Failed to write response")?;

    Ok(())
}
