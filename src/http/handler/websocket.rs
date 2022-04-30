use crate::http::common::{HTTPVersion, IMFDateTime};
use crate::http::handler::Handler;
use crate::http::request::{Request, RequestMethod, RequestParseError};
use crate::http::response::{Response, ResponseBody, ResponseHeaders, ResponseStatus, StatusLine};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use log::{debug, error};
use sha1::{Digest, Sha1};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const WS_VERSION: &str = "13";
const WS_ACCEPT_STR: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/*
     WebSocket Frame (from RFC 6455 5.2):

     0                   1                   2                   3
     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
    +-+-+-+-+-------+-+-------------+-------------------------------+
    |F|R|R|R| opcode|M| Payload len |    Extended payload length    |
    |I|S|S|S|  (4)  |A|     (7)     |             (16/64)           |
    |N|V|V|V|       |S|             |   (if payload len==126/127)   |
    | |1|2|3|       |K|             |                               |
    +-+-+-+-+-------+-+-------------+ - - - - - - - - - - - - - - - +
    |     Extended payload length continued, if payload len == 127  |
    + - - - - - - - - - - - - - - - +-------------------------------+
    |                               |Masking-key, if MASK set to 1  |
    +-------------------------------+-------------------------------+
    | Masking-key (continued)       |          Payload Data         |
    +-------------------------------- - - - - - - - - - - - - - - - +
    :                     Payload Data continued ...                :
    + - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - +
    |                     Payload Data continued ...                |
    +---------------------------------------------------------------+
*/

#[derive(Debug)]
pub enum Frame {
    // Text { data: String },
    Binary {
        data: Vec<u8>,
    },
    Close {
        status_code: Option<u16>,
        message: Vec<u8>,
    },
    Ping {
        data: Vec<u8>,
    },
    Pong {
        data: Vec<u8>,
    },
}

impl Frame {
    pub fn decode(raw_data: Vec<u8>) -> Result<Frame> {
        // TODO: Handle fragmentation

        let mut pos = 0;

        let metadata = raw_data[0];
        let _fin = (metadata & 0x80) != 0;
        let _rsv1 = (metadata & 0x40) != 0;
        let _rsv1 = (metadata & 0x20) != 0;
        let _rsv1 = (metadata & 0x10) != 0;
        pos += 1;

        let is_masked = (raw_data[1] & 0x80) != 0;
        let len = match raw_data[1] & 0x7f {
            l if l <= 0x7d => {
                let res = l as usize;
                pos += 1;
                res
            }
            l if l == 0x7e => {
                let res = usize::from_be_bytes((&raw_data[2..4]).try_into()?);
                pos += 3;
                res
            }
            l if l == 0x7f => {
                let res = usize::from_be_bytes((&raw_data[2..10]).try_into()?);
                pos += 9;
                res
            }
            _ => unreachable!(),
        };

        let mask_key_opt: Option<[u8; 4]> = if is_masked {
            let res = (&raw_data[pos..(pos + 4)]).try_into().ok();
            pos += 4;
            res
        } else {
            None
        };

        let mut data = if let Some(mask_key) = mask_key_opt {
            Self::unmask(raw_data[pos..(pos + len)].to_owned(), mask_key)
        } else {
            raw_data[pos..(pos + len)].to_owned()
        };
        pos += len;

        match metadata & 0x0f {
            0x2 => {
                // Binary
                Ok(Self::Binary { data })
            }
            0x8 => {
                // Close
                if data.len() < 2 {
                    // ignore body (assume that it has empty payload)
                    Ok(Self::Close {
                        status_code: None,
                        message: vec![],
                    })
                } else {
                    let status_code = u16::from_be_bytes(data.drain(..2).as_slice().try_into()?);
                    let message = data;
                    Ok(Self::Close {
                        status_code: Some(status_code),
                        message,
                    })
                }
            }
            0x9 => {
                // Ping
                Ok(Self::Ping { data })
            }
            0xa => {
                // Pong
                Ok(Self::Pong { data })
            }
            _ => {
                bail!("not yet implemented");
            }
        }
    }

    pub fn get_data(&self) -> Vec<u8> {
        match self {
            Self::Binary { data } => data.clone(),
            Self::Close {
                status_code,
                message,
            } => {
                let mut res = vec![];
                if let Some(code) = status_code {
                    res.extend(u16::to_be_bytes(*code))
                }
                res.extend(message);
                res
            }
            Self::Ping { data } => data.clone(),
            Self::Pong { data } => data.clone(),
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        // TODO: Handle fragmentation

        let mut res = vec![];

        let opcode = match self {
            Self::Binary { .. } => 0x2u8,
            Self::Close { .. } => 0x8u8,
            Self::Ping { .. } => 0x9u8,
            Self::Pong { .. } => 0xau8,
        };
        // always FIN for now
        res.push(0x80 | opcode);

        let data = self.get_data();

        match data.len() {
            l if l < (1 << 8) => res.push(l as u8),
            l if l < (1 << 16) => {
                res.push(0x7e);
                res.extend((l as u16).to_be_bytes());
            }
            l if l < (1 << 63) => {
                res.push(0x7f);
                res.extend((l as u64).to_be_bytes());
            }
            _ => {
                bail!("Too big payload");
            }
        }

        res.extend(data.iter());

        Ok(res)
    }

    fn unmask(data: Vec<u8>, mask_key: [u8; 4]) -> Vec<u8> {
        // RFC 6455 5.3
        data.into_iter()
            .enumerate()
            .map(|(i, b)| b ^ mask_key[i % 4])
            .collect()
    }
}

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
        loop {
            let mut buf = vec![];
            stream.read_buf(&mut buf).await?;
            let request_frame = Frame::decode(buf)?;
            debug!("Decode websocket frame: {:?}", request_frame);

            match request_frame {
                Frame::Binary { .. } => {
                    // do nothing for now
                }
                Frame::Ping { data } => {
                    let response_frame = Frame::Pong { data };
                    stream.write_all(&response_frame.encode()?).await?;
                }
                Frame::Pong { .. } => {}
                frame @ Frame::Close { .. } => {
                    // send back Close to show we accept it
                    stream.write_all(&frame.encode()?).await?;
                    break;
                }
            }
        }

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

    #[test]
    fn test_decode_ping_frame() {
        // ping frame with "hello" payload
        let raw_data = vec![
            0x89, 0x85, 0x78, 0xaf, 0x8c, 0x35, 0x10, 0xca, 0xe0, 0x59, 0x17,
        ];
        let frame = Frame::decode(raw_data).unwrap();
        assert!(matches!(
            frame,
            Frame::Ping {
                data,
            } if data == vec![b'h', b'e', b'l', b'l', b'o']
        ))
    }

    #[test]
    fn test_encode_pong_frame() {
        let frame = Frame::Pong {
            data: vec![b'h', b'e', b'l', b'l', b'o'],
        };
        let expected = vec![0x8a, 0x05, b'h', b'e', b'l', b'l', b'o'];
        assert_eq!(frame.encode().unwrap(), expected);
    }

    #[test]
    fn test_decode_close_frame() {
        let raw_data = vec![0x88, 0x80, 0x1e, 0x04, 0x7d, 0x84];
        let frame = Frame::decode(raw_data).unwrap();
        assert!(matches!(
            frame,
            Frame::Close { status_code: None, message } if message.is_empty()
        ));
    }

    #[test]
    fn test_decode_close_frame_with_payload() {
        // status_code: 1002, message: hi
        let raw_data = vec![0x88, 0x84, 0x1e, 0x04, 0x7d, 0x84, 0x1d, 0xee, 0x15, 0xed];
        let frame = Frame::decode(raw_data).unwrap();
        assert!(matches!(
            frame,
            Frame::Close { status_code: Some(1002), message } if message == vec![b'h', b'i']
        ));
    }

    #[test]
    fn test_encode_close_frame() {
        let frame = Frame::Close {
            status_code: None,
            message: vec![],
        };
        let expected = vec![0x88, 0x00];
        assert_eq!(frame.encode().unwrap(), expected);
    }

    #[test]
    fn test_encode_close_frame_with_payload() {
        let frame = Frame::Close {
            status_code: Some(1000),
            message: vec![b'h', b'i'],
        };
        let expected = vec![0x88, 0x04, 0x03, 0xe8, b'h', b'i'];
        assert_eq!(frame.encode().unwrap(), expected);
    }

    #[test]
    fn test_decode_binary_frame() {
        let raw_data = vec![0x82, 0x83, 0xec, 0xf6, 0xd7, 0x1c, 0xed, 0xf4, 0xd4];
        let frame = Frame::decode(raw_data).unwrap();
        assert!(matches!(
            frame,
            Frame::Binary {
                data
            } if data == vec![0x1, 0x2, 0x3]
        ))
    }

    #[test]
    fn test_encode_binary_frame() {
        let frame = Frame::Binary {
            data: vec![0x1, 0x2, 0x3],
        };
        let expected = vec![0x82, 0x03, 0x01, 0x02, 0x03];
        assert_eq!(frame.encode().unwrap(), expected);
    }

    // TODO
    // #[test]
    // fn test_decode_data_frame_with_long_data() {
    //     unimplemented!()
    // }

    // TODO
    // #[test]
    // fn test_encode_data_frame_with_long_data() {
    //     unimplemented!()
    // }
}
