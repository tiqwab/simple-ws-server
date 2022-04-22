use crate::http::common::HTTPVersion;
use crate::http::request::{Request, RequestLine};
use crate::http::response::{Response, ResponseBody, ResponseHeaders, StatusLine};
use anyhow::{Context, Result};
use futures::TryFutureExt;
use log::{debug, error};
use serde::Serialize;
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

#[derive(Serialize)]
struct EchoResponse {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    data: String,
}

impl EchoResponse {
    fn new(
        method: String,
        path: String,
        headers: HashMap<String, String>,
        data: String,
    ) -> EchoResponse {
        EchoResponse {
            method,
            path,
            headers,
            data,
        }
    }
}

async fn handle_request(mut stream: TcpStream, _client_addr: SocketAddr) -> Result<()> {
    let request = Request::parse(&mut stream)
        .await
        .context("Failed to parse request")?;
    debug!("Accepted request: {:?}", request);

    let echo_response = EchoResponse::new(
        request.get_method().to_string(),
        request.get_path().to_owned(),
        request
            .get_headers()
            .iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
        String::from_utf8_lossy(request.get_body()).to_string(),
    );
    let response_body =
        ResponseBody::new(serde_json::to_string(&echo_response)?.as_bytes().to_owned());

    let response = Response::new(
        StatusLine::new(HTTPVersion::V1_1, 200, "OK".to_string()),
        ResponseHeaders::from([
            ("Content-Type", "application/json".to_string()),
            ("Content-Length", response_body.len().to_string()),
        ]),
        response_body,
    );

    stream
        .write(&response.encode())
        .await
        .context("Failed to write response")?;

    Ok(())
}
