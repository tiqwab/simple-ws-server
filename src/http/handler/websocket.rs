use crate::http::common::{HTTPVersion, IMFDateTime};
use crate::http::handler::Handler;
use crate::http::request::{Request, RequestMethod, RequestParseError};
use crate::http::response::{Response, ResponseBody, ResponseHeaders, ResponseStatus, StatusLine};
use anyhow::{Context, Result};
use async_trait::async_trait;
use log::error;
use sha1::{Digest, Sha1};
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

const WS_VERSION: &str = "13";
const WS_ACCEPT_STR: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub struct WebSocketHandler;

impl WebSocketHandler {
    fn handshake(&self, request: &Request) -> Result<Response, RequestParseError> {
        fn check_header(request: &Request, key: &str, expected: &str) -> bool {
            matches!(request.get_header_lc(key).as_deref(), Some(s) if s == expected)
        }

        fn client_error(msg: &str) -> RequestParseError {
            RequestParseError::new(ResponseStatus::BadRequest, msg)
        }

        if !check_header(&request, "Upgrade", "websocket") {
            return Err(client_error("Illegal Upgrade header"));
        }
        if !check_header(&request, "Connection", "upgrade") {
            return Err(client_error("Illegal Connection header"));
        }
        if !check_header(&request, "Sec-WebSocket-Version", WS_VERSION) {
            return Err(client_error("Illegal WebSocket version"));
        }
        if request.get_method() != &RequestMethod::GET {
            return Err(client_error("Illegal request method"));
        }

        let sec_ws_key = request
            .get_header("Sec-WebSocket-Key")
            .ok_or(client_error("Missing Sec-WebSocket-Key header"))?;

        let bs: Vec<u8> = sec_ws_key.bytes().chain(WS_ACCEPT_STR.bytes()).collect();
        let mut hasher = Sha1::new();
        hasher.update(&bs);
        let hashed = hasher.finalize();

        let sec_ws_accept = base64::encode(hashed);

        let response = Response::new(
            StatusLine::new(HTTPVersion::V1_1, ResponseStatus::SwitchingProtocol),
            ResponseHeaders::from([
                ("Date", IMFDateTime::now().to_string()),
                ("Upgrade", "websocket".to_string()),
                ("Connection", "Upgrade".to_string()),
                ("Sec-WebSocket-Accept", sec_ws_accept),
            ]),
            ResponseBody::new(vec![]),
        );
        Ok(response)
    }
}

#[async_trait]
impl Handler for WebSocketHandler {
    fn accepts(&self, request: &Request, _client_addr: SocketAddr) -> bool {
        matches!(
            request.get_header_lc("Upgrade").as_deref(),
            Some("websocket")
        )
    }

    async fn handle(
        &self,
        request: Request,
        mut stream: TcpStream,
        client_addr: SocketAddr,
    ) -> Result<()> {
        match self.handshake(&request) {
            Ok(res) => {
                stream
                    .write(&res.encode())
                    .await
                    .context("Failed to write response")?;
            }
            Err(err) => {
                if err.get_status().is_server_error() {
                    error!(
                        "Error occurred while handling request from {}: {:?}",
                        client_addr, err
                    );
                }
                let res = Response::new(
                    StatusLine::new(HTTPVersion::V1_1, err.get_status().clone()),
                    ResponseHeaders::from([
                        ("Date", IMFDateTime::now().to_string().as_str()),
                        ("Connection", "close"),
                        ("Content-Length", "0"),
                    ]),
                    ResponseBody::new(vec![]),
                );
                stream
                    .write(&res.encode())
                    .await
                    .context("Failed to write response")?;
                return Ok(());
            }
        };

        // continue when handshake succeeded

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::request::{RequestBody, RequestHeaders, RequestLine};

    fn create_ws_request() -> Request {
        Request::new(
            RequestLine::new(RequestMethod::GET, "/", HTTPVersion::V1_1),
            RequestHeaders::from([
                ("Host", "localhost:80"),
                ("Upgrade", "websocket"),
                ("Connection", "Upgrade"),
                ("Sec-WebSocket-Version", "13"),
                // the value comes from RFC 6455 1.3
                ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ]),
            RequestBody::new(vec![]),
        )
    }

    #[test]
    fn test_websocket_handler_handshake() {
        let req = create_ws_request();
        let res = WebSocketHandler.handshake(&req);
        assert!(res.is_ok());
        let res = res.unwrap();
        assert_eq!(res.get_status(), &ResponseStatus::SwitchingProtocol);
        assert_eq!(res.get_header("Upgrade"), Some("websocket"));
        assert_eq!(res.get_header("Connection"), Some("Upgrade"));
        assert_eq!(
            res.get_header("Sec-WebSocket-Accept"),
            Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
        );
    }

    #[test]
    fn test_websocket_handler_handshake_for_missing_headers() {
        let original_req = create_ws_request();
        for header in [
            "Upgrade",
            "Connection",
            "Sec-WebSocket-Version",
            "Sec-WebSocket-Key",
        ] {
            let mut req = original_req.clone();
            req.remove_header(header);
            let res = WebSocketHandler.handshake(&req);
            assert!(res.is_err(), "Should require \"{}\" in header", header);
            assert_eq!(res.unwrap_err().get_status(), &ResponseStatus::BadRequest);
        }
    }

    #[test]
    fn test_websocket_handler_handshake_for_illegal_connection() {
        let mut req = create_ws_request();
        req.insert_header("Connection".to_string(), "foo".to_string());
        let res = WebSocketHandler.handshake(&req);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().get_status(), &ResponseStatus::BadRequest);
    }
}
