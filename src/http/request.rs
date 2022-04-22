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
pub struct RequestBody(Vec<u8>);

impl RequestBody {
    pub fn new(v: Vec<u8>) -> RequestBody {
        RequestBody(v)
    }

    pub fn parse<T: BodyParser>(&self) -> Result<T> {
        T::parse(&self.0)
    }
}

trait BodyParser: Sized {
    fn parse(bs: &[u8]) -> Result<Self>;
}

impl BodyParser for String {
    fn parse(bs: &[u8]) -> Result<Self> {
        let vs = bs.to_vec();
        let s = String::from_utf8(vs)?;
        Ok(s)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    request_line: RequestLine,
    headers: RequestHeaders,
    body: RequestBody,
}

impl Request {
    pub fn new(request_line: RequestLine, headers: RequestHeaders, body: RequestBody) -> Request {
        Request {
            request_line,
            headers,
            body,
        }
    }

    pub async fn parse<T: AsyncRead + Unpin>(
        mut reader: RequestReader<'_, T>,
    ) -> Result<Self, RequestParseError> {
        async fn do_parse_request_line<T: AsyncRead + Unpin>(
            reader: &mut RequestReader<'_, T>,
        ) -> Result<(), RequestParseError> {
            loop {
                match reader.read_request_line().await {
                    Ok(Some(_)) => {
                        return Ok(());
                    }
                    Ok(None) => {}
                    Err(err) => return Err(err),
                }
            }
        }

        async fn do_parse_headers<T: AsyncRead + Unpin>(
            reader: &mut RequestReader<'_, T>,
        ) -> Result<(), RequestParseError> {
            loop {
                match reader.read_request_headers().await {
                    Ok(Some(_)) => return Ok(()),
                    Ok(None) => {}
                    Err(err) => return Err(err),
                }
            }
        }

        async fn do_parse_body<T: AsyncRead + Unpin>(
            reader: &mut RequestReader<'_, T>,
        ) -> Result<(), RequestParseError> {
            loop {
                match reader.read_request_body().await {
                    Ok(Some(_)) => return Ok(()),
                    Ok(None) => {}
                    Err(err) => return Err(err),
                }
            }
        }

        do_parse_request_line(&mut reader).await?;
        do_parse_headers(&mut reader).await?;
        do_parse_body(&mut reader).await?;
        reader
            .to_request()
            .ok_or(RequestParseError(500, "Internal Server Error".to_string()))
    }
}

pub struct RequestReader<'a, T: AsyncRead> {
    reader: &'a mut T,
    buf: Vec<u8>,
    request_line: Option<RequestLine>,
    request_headers: Option<RequestHeaders>,
    request_body: Option<RequestBody>,
}

impl<T: AsyncRead + Unpin> RequestReader<'_, T> {
    pub fn new(reader: &mut T) -> RequestReader<T> {
        RequestReader {
            reader,
            buf: Vec::new(),
            request_line: None,
            request_headers: None,
            request_body: None,
        }
    }

    pub async fn read_request_line(&mut self) -> Result<Option<()>, RequestParseError> {
        if self.request_line.is_some() {
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

        self.request_line = Some(request_line);
        Ok(Some(()))
    }

    pub async fn read_request_headers(&mut self) -> Result<Option<()>, RequestParseError> {
        if self.request_headers.is_some() {
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
        self.request_headers = Some(headers);
        Ok(Some(()))
    }

    pub async fn read_request_body(&mut self) -> Result<Option<()>, RequestParseError> {
        let headers = self
            .request_headers
            .as_ref()
            .ok_or(RequestParseError(500, "Internal Server Error".to_string()))?;
        let content_length = {
            let cl = headers.get("Content-Length").unwrap_or("0");
            cl.parse::<usize>()
                .map_err(|_| RequestParseError(400, "Bad Request".to_string()))?
        };

        let mut buf = Vec::new();
        let _n = self.reader.read_buf(&mut buf).await.unwrap(); // FIXME unwrap
        self.buf.extend(buf);

        if self.buf.len() < content_length {
            return Ok(None);
        }

        let body = RequestBody(self.buf[..content_length].to_vec());
        self.buf.clear();
        self.request_body = Some(body);
        Ok(Some(()))
    }

    pub fn to_request(self) -> Option<Request> {
        let line = self.request_line?;
        let headers = self.request_headers?;
        let body = self.request_body?;
        Some(Request::new(line, headers, body))
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
    async fn test_parse_request_post() {
        // setup
        let tmp_file = TempFile::new().unwrap();
        let str = [
            "POST / HTTP/1.1",
            "Content-Type: application/x-www-form-urlencoded",
            "Content-Length: 10",
            "",
            "name=alice",
        ]
        .join("\r\n");
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
        assert_eq!(actual.request_line.method, RequestMethod::POST);
        assert_eq!(
            actual.headers.get("Content-Type"),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(&actual.body.parse::<String>().unwrap(), "name=alice")
    }
}
