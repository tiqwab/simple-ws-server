use crate::http::common::HTTPVersion;
use crate::http::headers;
use crate::http::response::ResponseStatus;
use anyhow::Result;
use log::error;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use tokio::io::{AsyncRead, AsyncReadExt};

#[derive(Debug, PartialEq, Eq)]
pub struct RequestParseError(ResponseStatus, String);

impl RequestParseError {
    pub fn new(status: ResponseStatus, message: &str) -> RequestParseError {
        RequestParseError(status, message.to_string())
    }

    #[allow(dead_code)]
    pub fn get_status(&self) -> &ResponseStatus {
        &self.0
    }

    #[allow(dead_code)]
    pub fn get_error_message(&self) -> &str {
        &self.1
    }
}

impl fmt::Display for RequestParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("{} {}", self.0, self.1))
    }
}

impl Error for RequestParseError {}

#[derive(Debug, PartialEq, Eq, Clone)]
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
            _ => Err(RequestParseError::new(
                ResponseStatus::NotImplemented,
                "Unknown request method",
            )),
        }
    }
}

impl Display for RequestMethod {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RequestMethod::GET => f.write_str("GET"),
            RequestMethod::POST => f.write_str("POST"),
            RequestMethod::PUT => f.write_str("PUT"),
            RequestMethod::DELETE => f.write_str("DELETE"),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
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
            return Err(RequestParseError::new(
                ResponseStatus::BadRequest,
                "Illegal request line",
            ));
        }
        let method = RequestMethod::from_str(items[0])?;
        let path = items[1];
        let version = HTTPVersion::from_str(items[2]).map_err(|_| {
            RequestParseError::new(
                ResponseStatus::BadRequest,
                &format!("Illegal request line: {}", line),
            )
        })?;
        Ok(RequestLine::new(method, path, version))
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RequestHeaders(HashMap<String, String>);

type RequestHeadersIter<'a, K, V> = std::collections::hash_map::Iter<'a, K, V>;

impl RequestHeaders {
    pub fn new() -> RequestHeaders {
        RequestHeaders(HashMap::new())
    }

    #[allow(dead_code)]
    pub fn from<const N: usize>(
        arr: [(impl Into<String>, impl Into<String>); N],
    ) -> RequestHeaders {
        let mut headers = HashMap::new();
        for (k, v) in arr {
            headers.insert(k.into(), v.into());
        }
        RequestHeaders(headers)
    }

    #[allow(dead_code)]
    pub fn get_raw(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|x| x.as_str())
    }

    #[allow(dead_code)]
    pub fn get<T, U: headers::HeaderParser<Value = T>>(
        &self,
        key: &headers::HTTPHeader<U>,
    ) -> Option<T> {
        self.get_raw(key.name()).and_then(|s| key.parse(s))
    }

    #[allow(dead_code)]
    pub fn insert(&mut self, key: String, value: String) -> Option<String> {
        self.0.insert(key, value)
    }

    #[allow(dead_code)]
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.0.remove(key)
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[allow(dead_code)]
    pub fn iter(&self) -> RequestHeadersIter<'_, String, String> {
        self.0.iter()
    }

    pub fn parse(lines: &[&str]) -> Result<RequestHeaders, RequestParseError> {
        // returns (field-name, field-value)
        fn parse_line(line: &str) -> Result<(String, String), RequestParseError> {
            let pos_delim = line.chars().position(|c| c == ':').ok_or_else(|| {
                RequestParseError::new(
                    ResponseStatus::BadRequest,
                    &format!("Illegal header field: {}", line),
                )
            })?;

            let field_name = line[..pos_delim].to_string();
            let field_value = line[(pos_delim + 1)..].trim().to_string();

            // field_name is not allowed to have leading and following spaces
            // (RFC 7230 3.2.4)
            let trimmed_field_name = field_name.trim().to_string();
            if field_name != trimmed_field_name {
                return Err(RequestParseError::new(
                    ResponseStatus::BadRequest,
                    &format!("Illegal header field: {}", line),
                ));
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RequestBody(Vec<u8>);

impl RequestBody {
    pub fn new(v: Vec<u8>) -> RequestBody {
        RequestBody(v)
    }

    #[allow(dead_code)]
    pub fn parse<T: BodyParser>(&self) -> Result<T> {
        T::parse(&self.0)
    }
}

pub trait BodyParser: Sized {
    fn parse(bs: &[u8]) -> Result<Self>;
}

impl BodyParser for String {
    fn parse(bs: &[u8]) -> Result<Self> {
        let vs = bs.to_vec();
        let s = String::from_utf8(vs)?;
        Ok(s)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
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

    #[allow(dead_code)]
    pub fn get_method(&self) -> &RequestMethod {
        &self.request_line.method
    }

    #[allow(dead_code)]
    pub fn get_path(&self) -> &str {
        &self.request_line.path
    }

    #[allow(dead_code)]
    pub fn get_headers(&self) -> &RequestHeaders {
        &self.headers
    }

    #[allow(dead_code)]
    pub fn get_header(&self, key: &str) -> Option<&str> {
        self.headers.get_raw(key)
    }

    #[allow(dead_code)]
    pub fn insert_header(&mut self, key: String, value: String) -> Option<String> {
        self.headers.insert(key, value)
    }

    #[allow(dead_code)]
    pub fn remove_header(&mut self, key: &str) -> Option<String> {
        self.headers.remove(key)
    }

    /// Return header value converted to lower cases
    #[allow(dead_code)]
    pub fn get_header_lc(&self, key: &str) -> Option<String> {
        self.headers.get_raw(key).map(|s| s.to_ascii_lowercase())
    }

    #[allow(dead_code)]
    pub fn get_body(&self) -> &[u8] {
        &self.body.0
    }

    pub async fn parse<T: AsyncRead + Unpin>(reader: &mut T) -> Result<Self, RequestParseError> {
        let mut metadata_reader = reader::RequestMetadataReader::new(reader);

        let request_line = RequestLine::parse(&metadata_reader.read().await.map_err(|err| {
            RequestParseError::new(
                ResponseStatus::InternalServerError,
                &format!("Failed to read request line: {:?}", err),
            )
        })?)?;

        let mut lines = vec![];
        loop {
            let line = metadata_reader.read().await.map_err(|err| {
                RequestParseError::new(
                    ResponseStatus::InternalServerError,
                    &format!("Failed to read header line: {:?}", err),
                )
            })?;
            if line.is_empty() {
                break;
            }
            lines.push(line);
        }
        let request_headers =
            RequestHeaders::parse(&lines.iter().map(|x| x.as_str()).collect::<Vec<_>>()[..])?;
        let content_length = {
            let cl = request_headers.get_raw("Content-Length").unwrap_or("0");
            cl.parse::<usize>().map_err(|_| {
                RequestParseError::new(ResponseStatus::BadRequest, "Illegal Content-Length")
            })?
        };

        let mut body_reader = metadata_reader.into_body_reader(content_length);
        let request_body = RequestBody::new(
            body_reader
                .read()
                .await
                .map_err(|err| {
                    error!("Failed to read header line: {:?}", err);
                    RequestParseError::new(
                        ResponseStatus::BadRequest,
                        "Failed to read request body",
                    )
                })?
                .to_vec(),
        );

        Ok(Request::new(request_line, request_headers, request_body))
    }
}

mod reader {
    use super::*;
    use anyhow::bail;

    pub struct RequestMetadataReader<'a, T: AsyncRead> {
        reader: &'a mut T,
        buf: Vec<u8>,
    }

    impl<'a, T: AsyncRead + Unpin> RequestMetadataReader<'a, T> {
        pub fn new(reader: &mut T) -> RequestMetadataReader<T> {
            RequestMetadataReader {
                reader,
                buf: Vec::new(),
            }
        }

        pub async fn read(&mut self) -> Result<String> {
            loop {
                if !self.buf.is_empty() {
                    let mut pos_crlf = None;
                    for i in 0..(self.buf.len() - 1) {
                        if self.buf[i] == b'\r' && self.buf[i + 1] == b'\n' {
                            pos_crlf = Some(i);
                            break;
                        }
                    }
                    if let Some(pos_crlf) = pos_crlf {
                        let line = String::from_utf8_lossy(
                            &self.buf.drain(..pos_crlf).collect::<Vec<_>>(),
                        )
                        .to_string();
                        self.buf.drain(..2);
                        return Ok(line);
                    }
                }

                let mut buf = Vec::new();
                let n = self.reader.read_buf(&mut buf).await?;
                if n == 0 {
                    // this should be the case when the client disconnected
                    bail!("client disconnected");
                }
                self.buf.extend(buf);
            }
        }

        pub fn into_body_reader(self, length: usize) -> RequestBodyReader<'a, T> {
            RequestBodyReader::new(self.reader, self.buf, length)
        }
    }

    pub struct RequestBodyReader<'a, T: AsyncRead> {
        reader: &'a mut T,
        buf: Vec<u8>,
        content_length: usize,
    }

    impl<T: AsyncRead + Unpin> RequestBodyReader<'_, T> {
        pub fn new(
            reader: &mut T,
            buf: Vec<u8>,
            content_length: usize,
        ) -> RequestBodyReader<'_, T> {
            RequestBodyReader {
                reader,
                buf,
                content_length,
            }
        }

        pub async fn read(&mut self) -> Result<&[u8]> {
            loop {
                if self.buf.len() >= self.content_length {
                    return Ok(&self.buf[..self.content_length]);
                }

                let mut buf = Vec::new();
                let n = self.reader.read_buf(&mut buf).await?;
                if n == 0 {
                    // this should be the case when the client disconnected
                    bail!("client disconnected");
                }
                self.buf.extend(buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::temp::TempFile;
    use tokio::fs::OpenOptions;
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
        assert!(matches!(
            actual,
            Err(RequestParseError(ResponseStatus::NotImplemented, _))
        ));
    }

    #[test]
    fn test_parse_request_headers() {
        let ss = [
            "Content-Type: text/plain",
            "Content-Length: 0",
            "Connection: keep-alive, Upgrade",
        ];
        let actual = RequestHeaders::parse(&ss).unwrap();
        assert_eq!(actual.len(), 3);
        assert_eq!(actual.get_raw("Content-Type"), Some("text/plain"));
        assert_eq!(actual.get_raw("Content-Length"), Some("0"));
        assert_eq!(
            actual.get(&headers::CONNECTION),
            Some(vec!["keep-alive".to_string(), "Upgrade".to_string()])
        );
    }

    #[test]
    fn test_parse_request_headers_with_illegal_format() {
        let ss = ["Content-Type : text/plain"];
        let actual = RequestHeaders::parse(&ss);
        assert!(matches!(
            actual,
            Err(RequestParseError(ResponseStatus::BadRequest, _))
        ));
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
        let actual = Request::parse(&mut tmp_file.access_for_read().await.unwrap())
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
        let actual = Request::parse(&mut tmp_file.access_for_read().await.unwrap())
            .await
            .unwrap();

        // verify
        assert_eq!(actual.request_line.method, RequestMethod::POST);
        assert_eq!(
            actual.headers.get_raw("Content-Type"),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(&actual.body.parse::<String>().unwrap(), "name=alice")
    }
}
