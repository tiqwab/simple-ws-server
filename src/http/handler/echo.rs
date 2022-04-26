use crate::http::common::{HTTPVersion, IMFDateTime};
use crate::http::handler::Handler;
use crate::http::request::{Request, RequestParseError};
use crate::http::response::{Response, ResponseBody, ResponseHeaders, ResponseStatus, StatusLine};
use anyhow::{Context, Result};
use async_trait::async_trait;
use log::error;
use serde::Serialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

pub struct EchoHandler;

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

#[async_trait]
impl Handler for EchoHandler {
    fn accepts(&self, request: &Request, client_addr: SocketAddr) -> bool {
        true
    }

    async fn handle(
        &self,
        request: Request,
        mut stream: TcpStream,
        client_addr: SocketAddr,
    ) -> Result<()> {
        fn prepare_response(request: Request) -> Result<Response, RequestParseError> {
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
                    ("Date", IMFDateTime::now().to_string()),
                    ("Content-Type", "application/json".to_string()),
                    ("Content-Length", response_body.len().to_string()),
                ]),
                response_body,
            );

            Ok(response)
        }

        let response = prepare_response(request).unwrap_or_else(|err| {
            if err.get_status().is_server_error() {
                error!(
                    "Error occurred while handling request from {}: {:?}",
                    client_addr, err
                );
            }
            Response::new(
                StatusLine::new(HTTPVersion::V1_1, err.get_status().clone()),
                ResponseHeaders::from([
                    ("Date", IMFDateTime::now().to_string().as_str()),
                    ("Connection", "close"),
                    ("Content-Length", "0"),
                ]),
                ResponseBody::new(vec![]),
            )
        });

        stream
            .write(&response.encode())
            .await
            .context("Failed to write response")?;

        Ok(())
    }
}
