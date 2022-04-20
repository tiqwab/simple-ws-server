use anyhow::Result;
use log::debug;
use std::error::Error;
use std::fmt;
use std::fmt::Formatter;
use std::io::Read;
use std::str::FromStr;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::TcpStream;

#[derive(Debug, PartialEq, Eq)]
pub struct RequestParseError(u16, String);

impl fmt::Display for RequestParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("{} {}", self.0, self.1))
    }
}

impl Error for RequestParseError {}

#[derive(Debug, PartialEq, Eq)]
pub enum RequestMethod {
    GET,
    POST,
    PUT,
    DELETE,
}

impl FromStr for RequestMethod {
    type Err = RequestParseError;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        match str {
            "GET" => Ok(RequestMethod::GET),
            "POST" => Ok(RequestMethod::POST),
            "PUT" => Ok(RequestMethod::PUT),
            "DELETE" => Ok(RequestMethod::DELETE),
            _ => Err(RequestParseError(501, "Not Implemented".to_string())),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum HTTPVersion {
    V1_1,
}

impl FromStr for HTTPVersion {
    type Err = RequestParseError;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        match str {
            "HTTP/1.1" => Ok(HTTPVersion::V1_1),
            _ => Err(RequestParseError(400, "Bad Request".to_string())),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RequestLine {
    method: RequestMethod,
    path: String,
    version: HTTPVersion,
}

impl RequestLine {
    pub fn new(method: RequestMethod, path: &str, version: HTTPVersion) -> RequestLine {
        RequestLine {
            method,
            path: path.to_string(),
            version,
        }
    }

    pub fn parse(line: &str) -> Result<RequestLine, RequestParseError> {
        let items: Vec<_> = line.split(' ').collect();
        if items.len() != 3 {
            return Err(RequestParseError(400, "Bad Request".to_string()));
        }
        let method = RequestMethod::from_str(items[0])?;
        let path = items[1];
        let version = HTTPVersion::from_str(items[2])?;
        Ok(RequestLine::new(method, path, version))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    request_line: RequestLine,
}

impl Request {
    pub fn new(request_line: RequestLine) -> Request {
        Request { request_line }
    }

    pub async fn parse<T: AsyncRead + Unpin>(
        mut reader: RequestReader<'_, T>,
    ) -> Result<Self, RequestParseError> {
        loop {
            match reader.read_request_line().await {
                Ok(Some(request_line)) => return Ok(Request::new(request_line)),
                Ok(None) => {}
                Err(err) => return Err(err),
            }
        }
    }
}

pub struct RequestReader<'a, T: AsyncRead> {
    reader: &'a mut T,
    buf: Vec<u8>,
    is_done_request_line: bool,
}

impl<T: AsyncRead + Unpin> RequestReader<'_, T> {
    pub fn new(reader: &mut T) -> RequestReader<T> {
        RequestReader {
            reader,
            buf: Vec::new(),
            is_done_request_line: false,
        }
    }

    pub async fn read_request_line(&mut self) -> Result<Option<RequestLine>, RequestParseError> {
        if self.is_done_request_line {
            return Ok(None);
        }

        let mut buf = Vec::new();
        let _n = self.reader.read_buf(&mut buf).await.unwrap(); // FIXME unwrap
        self.buf.extend(buf);

        if self.buf.len() == 0 {
            return Ok(None);
        }

        let mut pos_crlf = 0;
        for i in 0..(self.buf.len() - 1) {
            if self.buf[i] == b'\r' && self.buf[i + 1] == b'\n' {
                pos_crlf = i;
                break;
            }
        }
        if pos_crlf == 0 {
            return Ok(None);
        }

        let line =
            String::from_utf8_lossy(&self.buf.drain(..pos_crlf).collect::<Vec<_>>()).to_string();
        let request_line = RequestLine::parse(&line)?;

        // Remove \r\n
        self.buf.drain(..2);
        self.is_done_request_line = true;
        Ok(Some(request_line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::temp::TempFile;
    use tokio::fs::{File, OpenOptions};
    use tokio::io::AsyncWriteExt;

    #[test]
    fn test_parse_request_line() {
        let str = "GET / HTTP/1.1";
        let actual = RequestLine::parse(str).unwrap();
        let expected = RequestLine::new(RequestMethod::GET, "/", HTTPVersion::V1_1);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_parse_request_line_with_unsupported_method() {
        let str = "PATCH / HTTP/1.1";
        let actual = RequestLine::parse(str);
        assert!(matches!(actual, Err(RequestParseError(501, _))));
    }

    #[tokio::test]
    async fn test_parse_request() {
        // setup
        let tmp_file = TempFile::new().unwrap();
        {
            let mut accessor = OpenOptions::new()
                .write(true)
                .open(tmp_file.get_path())
                .await
                .unwrap();
            let str = "GET / HTTP/1.1\r\n\r\n";
            accessor.write_all(str.as_bytes()).await.unwrap();
        }

        // exercise
        let mut accessor = OpenOptions::new()
            .read(true)
            .open(tmp_file.get_path())
            .await
            .unwrap();
        let actual = Request::parse(RequestReader::new(&mut accessor))
            .await
            .unwrap();

        // verify
        assert_eq!(actual.request_line.method, RequestMethod::GET);
    }
}
