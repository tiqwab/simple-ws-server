use crate::http::common::HTTPVersion;
use crate::http::request::{Request, RequestLine, RequestParseError};
use crate::http::response::{Response, ResponseBody, ResponseHeaders, ResponseStatus, StatusLine};
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

async fn handle_request(mut stream: TcpStream, client_addr: SocketAddr) -> Result<()> {
    async fn do_handle_request(stream: &mut TcpStream) -> Result<Response, RequestParseError> {
        let request = Request::parse(stream).await?;
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
        let response_body = ResponseBody::new(
            serde_json::to_string(&echo_response)
                .map_err(|_err| {
                    RequestParseError::new(
                        ResponseStatus::InternalServerError,
                        "Failed to create response body",
                    )
                })?
                .as_bytes()
                .to_owned(),
        );

        let response = Response::new(
            StatusLine::new(HTTPVersion::V1_1, ResponseStatus::Ok),
            ResponseHeaders::from([
                ("Content-Type", "application/json".to_string()),
                ("Content-Length", response_body.len().to_string()),
            ]),
            response_body,
        );

        Ok(response)
    }

    let response = do_handle_request(&mut stream).await.unwrap_or_else(|err| {
        if err.get_status().is_server_error() {
            error!(
                "Error occurred while handling request from {}: {:?}",
                client_addr, err
            );
        }
        Response::new(
            StatusLine::new(HTTPVersion::V1_1, err.get_status().clone()),
            ResponseHeaders::from([("Connection", "close"), ("Content-Length", "0")]),
            ResponseBody::new(vec![]),
        )
    });

    stream
        .write(&response.encode())
        .await
        .context("Failed to write response")?;

    Ok(())
}
