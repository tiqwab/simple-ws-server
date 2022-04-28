use crate::http::common::HTTPVersion;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Formatter;

#[derive(Debug, Clone)]
pub struct StatusLine {
    version: HTTPVersion,
    status: ResponseStatus,
}

impl StatusLine {
    pub fn new(version: HTTPVersion, status: ResponseStatus) -> StatusLine {
        StatusLine { version, status }
    }

    pub fn get_status(&self) -> &ResponseStatus {
        &self.status
    }

    pub fn encode(&self) -> Vec<u8> {
        format!(
            "{} {} {}",
            self.version,
            self.status.status_code(),
            self.status.reason_phrase(),
        )
        .as_bytes()
        .to_owned()
    }
}

#[derive(Debug)]
pub struct ResponseHeaders(HashMap<String, String>);

impl ResponseHeaders {
    pub fn new(headers: HashMap<String, String>) -> ResponseHeaders {
        ResponseHeaders(headers)
    }

    pub fn from<const N: usize>(
        arr: [(impl Into<String>, impl Into<String>); N],
    ) -> ResponseHeaders {
        let mut headers = HashMap::new();
        for (k, v) in arr {
            headers.insert(k.into(), v.into());
        }
        ResponseHeaders::new(headers)
    }

    pub fn empty() -> ResponseHeaders {
        ResponseHeaders(HashMap::new())
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

    pub fn encode(&self) -> Vec<u8> {
        let mut res = vec![];
        for (k, v) in self.0.iter() {
            res.extend(format!("{}: {}\r\n", k.trim(), v.trim()).as_bytes());
        }
        res
    }
}

#[derive(Debug)]
pub struct ResponseBody(Vec<u8>);

impl ResponseBody {
    pub fn new(data: Vec<u8>) -> ResponseBody {
        ResponseBody(data)
    }

    /// Return size as bytes
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn encode(&self) -> Vec<u8> {
        self.0.clone()
    }
}

#[derive(Debug)]
pub struct Response {
    status_line: StatusLine,
    headers: ResponseHeaders,
    body: ResponseBody,
}

impl Response {
    pub fn new(status_line: StatusLine, headers: ResponseHeaders, body: ResponseBody) -> Response {
        Response {
            status_line,
            headers,
            body,
        }
    }

    pub fn get_status(&self) -> &ResponseStatus {
        self.status_line.get_status()
    }

    pub fn get_header(&self, key: &str) -> Option<&str> {
        self.headers.get(key)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut res = vec![];
        res.extend(self.status_line.encode());
        res.extend([b'\r', b'\n']);
        res.extend(self.headers.encode());
        res.extend([b'\r', b'\n']);
        res.extend(self.body.encode());
        res
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ResponseStatus {
    SwitchingProtocol,
    Ok,
    BadRequest,
    InternalServerError,
    NotImplemented,
}

impl ResponseStatus {
    pub fn status_code(&self) -> u16 {
        match self {
            ResponseStatus::SwitchingProtocol => 101,
            ResponseStatus::Ok => 200,
            ResponseStatus::BadRequest => 400,
            ResponseStatus::InternalServerError => 500,
            ResponseStatus::NotImplemented => 501,
        }
    }

    pub fn reason_phrase(&self) -> String {
        match self {
            ResponseStatus::SwitchingProtocol => "Switching Protocol",
            ResponseStatus::Ok => "OK",
            ResponseStatus::BadRequest => "Bad Request",
            ResponseStatus::InternalServerError => "Internal Server Error",
            ResponseStatus::NotImplemented => "Not Implemented",
        }
        .to_string()
    }

    pub fn is_server_error(&self) -> bool {
        self.status_code() / 100 == 5
    }
}

impl fmt::Display for ResponseStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("{}", self.status_code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_response() {
        // setup
        let data = "hello".as_bytes();
        let response = Response::new(
            StatusLine::new(HTTPVersion::V1_1, ResponseStatus::Ok),
            ResponseHeaders::from([("Content-Length", data.len().to_string())]),
            ResponseBody::new(data.to_owned()),
        );

        // exercise
        let actual = String::from_utf8_lossy(&response.encode()).to_string();

        // verify
        let expected = ["HTTP/1.1 200 OK", "Content-Length: 5", "", "hello"].join("\r\n");

        assert_eq!(actual, expected);
    }
}
