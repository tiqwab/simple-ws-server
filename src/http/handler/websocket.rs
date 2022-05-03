use crate::http::common::{HTTPVersion, IMFDateTime};
use crate::http::handler::Handler;
use crate::http::request::{Request, RequestMethod, RequestParseError};
use crate::http::response::{Response, ResponseBody, ResponseHeaders, ResponseStatus, StatusLine};
use crate::settings::Settings;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use log::{debug, error};
use sha1::{Digest, Sha1};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
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
    Text {
        message: String,
    },
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
    pub async fn decode<T: AsyncRead + Unpin>(
        reader: &mut T,
        max_payload_size: usize,
    ) -> Result<Frame> {
        // TODO: Handle fragmentation

        let metadata = reader
            .read_u8()
            .await
            .context("Failed to read the first byte of frame")?;
        let _fin = (metadata & 0x80) != 0;
        let _rsv1 = (metadata & 0x40) != 0;
        let _rsv1 = (metadata & 0x20) != 0;
        let _rsv1 = (metadata & 0x10) != 0;

        let first_len_byte = reader
            .read_u8()
            .await
            .context("Failed to read first byte of length")?;
        let is_masked = (first_len_byte & 0x80) != 0;
        let len = match first_len_byte & 0x7f {
            l if l <= 0x7d => l as usize,
            l if l == 0x7e => reader
                .read_u16()
                .await
                .context("Failed to read 16-bit length")? as usize,
            l if l == 0x7f => reader
                .read_u64()
                .await
                .context("Failed to read 64-bit length")? as usize,
            _ => unreachable!(),
        };
        if len > max_payload_size {
            bail!("Payload is too big");
        }

        let mask_key_opt: Option<[u8; 4]> = if is_masked {
            reader
                .read_u32()
                .await
                .context("Failed to read mask key")?
                .to_be_bytes()
                .try_into()
                .ok()
        } else {
            None
        };

        let mut data = if let Some(mask_key) = mask_key_opt {
            let mut buf = vec![0u8; len];
            reader
                .read_exact(&mut buf)
                .await
                .context("Failed to read paylod")?;
            Self::unmask(buf, mask_key)
        } else {
            let mut buf = vec![0u8; len];
            reader
                .read_exact(&mut buf)
                .await
                .context("Failed to read payload")?;
            buf
        };

        match metadata & 0x0f {
            0x1 => {
                // Text
                Ok(Self::Text {
                    message: String::from_utf8(data)
                        .context("Received text frame but cannot interpret as UTF-8 string")?,
                })
            }
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
                    let status_code = u16::from_be_bytes(
                        data.drain(..2)
                            .as_slice()
                            .try_into()
                            .context("Failed to read status_code in Close frame")?,
                    );
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
            opcode => {
                bail!("Unknown opcode: 0x{:02x}", opcode);
            }
        }
    }

    pub fn get_data(&self) -> Vec<u8> {
        match self {
            Self::Text { message } => message.as_bytes().to_owned(),
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
            Self::Text { .. } => 0x1u8,
            Self::Binary { .. } => 0x2u8,
            Self::Close { .. } => 0x8u8,
            Self::Ping { .. } => 0x9u8,
            Self::Pong { .. } => 0xau8,
        };
        // always FIN for now
        res.push(0x80 | opcode);

        let data = self.get_data();

        match data.len() {
            l if l < (1 << 7) => res.push(l as u8),
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
    fn accepts(
        &self,
        request: &Request,
        _client_addr: SocketAddr,
        _settings: Arc<Settings>,
    ) -> bool {
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
        settings: Arc<Settings>,
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
        let max_payload_size: usize = settings.as_ref().ws().max_payload_size().to_bytes() as usize;
        loop {
            let request_frame = Frame::decode(&mut stream, max_payload_size)
                .await
                .context("Failed to decode frame")?;
            debug!("Decode websocket frame: {:?}", request_frame);

            match request_frame {
                frame @ Frame::Text { .. } => {
                    // echo back
                    stream.write_all(&frame.encode()?).await?;
                }
                frame @ Frame::Binary { .. } => {
                    // echo back
                    stream.write_all(&frame.encode()?).await?;
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

    #[tokio::test]
    async fn test_decode_ping_frame() {
        // ping frame with "hello" payload
        let raw_data = vec![
            0x89, 0x85, 0x78, 0xaf, 0x8c, 0x35, 0x10, 0xca, 0xe0, 0x59, 0x17,
        ];
        let frame = Frame::decode(&mut raw_data.as_slice(), 1024).await.unwrap();
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

    #[tokio::test]
    async fn test_decode_close_frame() {
        let raw_data = vec![0x88, 0x80, 0x1e, 0x04, 0x7d, 0x84];
        let frame = Frame::decode(&mut raw_data.as_slice(), 1024).await.unwrap();
        assert!(matches!(
            frame,
            Frame::Close { status_code: None, message } if message.is_empty()
        ));
    }

    #[tokio::test]
    async fn test_decode_close_frame_with_payload() {
        // status_code: 1002, message: hi
        let raw_data = vec![0x88, 0x84, 0x1e, 0x04, 0x7d, 0x84, 0x1d, 0xee, 0x15, 0xed];
        let frame = Frame::decode(&mut raw_data.as_slice(), 1024).await.unwrap();
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

    #[tokio::test]
    async fn test_decode_binary_frame() {
        let raw_data = vec![0x82, 0x83, 0xec, 0xf6, 0xd7, 0x1c, 0xed, 0xf4, 0xd4];
        let frame = Frame::decode(&mut raw_data.as_slice(), 1024).await.unwrap();
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

    #[tokio::test]
    async fn test_decode_text_frame() {
        let raw_data = vec![
            0x81u8, 0x85, 0x36, 0x80, 0xd6, 0x47, 0x5e, 0xe5, 0xba, 0x2b, 0x59,
        ];
        let frame = Frame::decode(&mut raw_data.as_slice(), 1024).await.unwrap();
        assert!(matches!(
            frame,
            Frame::Text {
                message
            } if message == "hello"
        ))
    }

    #[test]
    fn test_encode_text_frame() {
        let frame = Frame::Text {
            message: "hello".to_owned(),
        };
        let expected = vec![0x81, 0x05, b'h', b'e', b'l', b'l', b'o'];
        assert_eq!(frame.encode().unwrap(), expected);
    }

    #[tokio::test]
    async fn test_decode_data_frame_with_middle_size_data() {
        // payload is "a" repeating `len` times
        let len = 128;
        let mut raw_data = vec![0x81u8, 0xfe, 0x00, 0x80, 0x61, 0xfc, 0xfd, 0x86];
        raw_data.extend(vec![0x00, 0x9d, 0x9c, 0xe7].repeat(len / 4).iter());
        let frame = Frame::decode(&mut raw_data.as_slice(), 1024).await.unwrap();
        if let Frame::Text { message } = frame {
            assert_eq!(message.len(), len,);
        } else {
            panic!("Expected Frame::Text but: {:?}", frame);
        }
    }

    #[tokio::test]
    async fn test_decode_data_frame_with_long_size_data() {
        // payload is "a" repeating `len` times
        let len = 256 * 256 + 4;
        let mut raw_data = vec![
            0x81u8, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04, 0x61, 0xfc, 0xfd, 0x86,
        ];
        raw_data.extend(vec![0x00, 0x9d, 0x9c, 0xe7].repeat(len / 4).iter());
        let frame = Frame::decode(&mut raw_data.as_slice(), len).await.unwrap();
        if let Frame::Text { message } = frame {
            assert_eq!(message.len(), len,);
        } else {
            panic!("Expected Frame::Text but: {:?}", frame);
        }
    }

    #[test]
    fn test_encode_data_frame_with_middle_size_data() {
        // payload is "a" repeating `len` times
        let len = 128;
        let frame = Frame::Text {
            message: "a".repeat(len),
        };
        let expected = {
            let mut buf = vec![0x81, 0x7e, 0x00, 0x80];
            buf.extend("a".repeat(len).bytes());
            buf
        };
        assert_eq!(frame.encode().unwrap(), expected);
    }

    #[test]
    fn test_encode_data_frame_with_long_size_data() {
        // payload is "a" repeating `len` times
        let len = 256 * 256 + 4;
        let frame = Frame::Text {
            message: "a".repeat(len),
        };
        let expected = {
            let mut buf = vec![0x81, 0x7f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04];
            buf.extend("a".repeat(len).bytes());
            buf
        };
        assert_eq!(frame.encode().unwrap(), expected);
    }

    #[tokio::test]
    async fn test_failed_to_decode_bigger_frame_than_limit() {
        let raw_data = vec![
            0x81u8, 0x85, 0x36, 0x80, 0xd6, 0x47, 0x5e, 0xe5, 0xba, 0x2b, 0x59,
        ];
        let res = Frame::decode(&mut raw_data.as_slice(), 4).await;
        assert!(res.is_err());
    }
}
