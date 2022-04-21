use anyhow::Result;
use log::debug;
use std::collections::HashMap;
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
pub struct RequestHeaders(HashMap<String, String>);

impl RequestHeaders {
    pub fn new() -> RequestHeaders {
        RequestHeaders(HashMap::new())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|x| x.as_str())
    }

    pub fn insert(&mut self, key: String, value: String) -> Option<String> {
        self.0.insert(key, value)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn parse(lines: &[&str]) -> Result<RequestHeaders, RequestParseError> {
        // returns (field-name, field-value)
        fn parse_line(line: &str) -> Result<(String, String), RequestParseError> {
            let pos_delim = line
                .chars()
                .position(|c| c == ':')
                .ok_or(RequestParseError(400, "Bad Request".to_string()))?;

            let field_name = line[..pos_delim].to_string();
            let field_value = line[(pos_delim + 1)..].trim().to_string();

            // field_name is not allowed to have leading and following spaces
            // (RFC 7230 3.2.4)
            let trimmed_field_name = field_name.trim().to_string();
            if field_name != trimmed_field_name {
                return Err(RequestParseError(400, "Bad Request".to_string()));
            }

            Ok((field_name, field_value))
        }

        let mut headers = RequestHeaders::new();
        for line in lines.iter() {
            let (key, value) = parse_line(line)?;
            headers.insert(key, value);
        }
        Ok(headers)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    request_line: RequestLine,
    headers: RequestHeaders,
}

impl Request {
    pub fn new(request_line: RequestLine, headers: RequestHeaders) -> Request {
        Request {
            request_line,
            headers,
        }
    }

    pub async fn parse<T: AsyncRead + Unpin>(
        mut reader: RequestReader<'_, T>,
    ) -> Result<Self, RequestParseError> {
        async fn do_parse_request_line<T: AsyncRead + Unpin>(
            reader: &mut RequestReader<'_, T>,
        ) -> Result<RequestLine, RequestParseError> {
            loop {
                match reader.read_request_line().await {
                    Ok(Some(request_line)) => {
                        return Ok(request_line);
                    }
                    Ok(None) => {}
                    Err(err) => return Err(err),
                }
            }
        }

        async fn do_parse_headers<T: AsyncRead + Unpin>(
            reader: &mut RequestReader<'_, T>,
        ) -> Result<RequestHeaders, RequestParseError> {
            loop {
                match reader.read_request_headers().await {
                    Ok(Some(request_headers)) => return Ok(request_headers),
                    Ok(None) => {}
                    Err(err) => return Err(err),
                }
            }
        }

        let request_line = do_parse_request_line(&mut reader).await?;
        let headers = do_parse_headers(&mut reader).await?;
        Ok(Request::new(request_line, headers))
    }
}

pub struct RequestReader<'a, T: AsyncRead> {
    reader: &'a mut T,
    buf: Vec<u8>,
    is_done_request_line: bool,
    is_done_request_headers: bool,
}

impl<T: AsyncRead + Unpin> RequestReader<'_, T> {
    pub fn new(reader: &mut T) -> RequestReader<T> {
        RequestReader {
            reader,
            buf: Vec::new(),
            is_done_request_line: false,
            is_done_request_headers: false,
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

        // Retain \r\n at the beginning
        // self.buf.drain(..2);

        self.is_done_request_line = true;
        Ok(Some(request_line))
    }

    pub async fn read_request_headers(
        &mut self,
    ) -> Result<Option<RequestHeaders>, RequestParseError> {
        if self.is_done_request_headers {
            return Ok(None);
        }

        let mut buf = Vec::new();
        let _n = self.reader.read_buf(&mut buf).await.unwrap(); // FIXME unwrap
        self.buf.extend(buf);

        if self.buf.len() == 0 {
            return Ok(None);
        }

        let mut pos_crlf2 = None;
        for i in 0..(self.buf.len() - 3) {
            if self.buf[i] == b'\r'
                && self.buf[i + 1] == b'\n'
                && self.buf[i + 2] == b'\r'
                && self.buf[i + 3] == b'\n'
            {
                pos_crlf2 = Some(i);
                break;
            }
        }
        let pos_crlf2 = match pos_crlf2 {
            None => return Ok(None),
            Some(i) => i,
        };

        // TODO: parse only allowed characters (RFC 7230 3.2)
        let str_headers =
            String::from_utf8_lossy(&self.buf.drain(..pos_crlf2).collect::<Vec<_>>()).to_string();
        let lines = str_headers.split("\r\n").skip(1).collect::<Vec<_>>();
        let headers = RequestHeaders::parse(&lines)?;

        // Remove \r\n\r\n
        self.buf.drain(..4);
        self.is_done_request_headers = true;
        Ok(Some(headers))
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

    #[test]
    fn test_parse_request_headers() {
        let ss = ["Content-Type: text/plain", "Content-Length: 0"];
        let actual = RequestHeaders::parse(&ss).unwrap();
        assert_eq!(actual.len(), 2);
        assert_eq!(actual.get("Content-Type"), Some("text/plain"));
        assert_eq!(actual.get("Content-Length"), Some("0"))
    }

    #[test]
    fn test_parse_request_headers_with_illegal_format() {
        let ss = ["Content-Type : text/plain"];
        let actual = RequestHeaders::parse(&ss);
        assert!(matches!(actual, Err(RequestParseError(400, _))));
    }

    #[tokio::test]
    async fn test_parse_request_only_request_line() {
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
        assert_eq!(actual.headers.len(), 0)
    }

    #[tokio::test]
    async fn test_parse_request_with_headers() {
        // setup
        let tmp_file = TempFile::new().unwrap();
        let str = "GET / HTTP/1.1\r\nContent-Type: text/plain\r\n\r\n";
        tmp_file
            .access_for_write()
            .await
            .unwrap()
            .write_all(str.as_bytes())
            .await
            .unwrap();

        // exercise
        let actual = Request::parse(RequestReader::new(
            &mut tmp_file.access_for_read().await.unwrap(),
        ))
        .await
        .unwrap();

        // verify
        assert_eq!(actual.request_line.method, RequestMethod::GET);
        assert_eq!(actual.headers.get("Content-Type"), Some("text/plain"));
    }
}
