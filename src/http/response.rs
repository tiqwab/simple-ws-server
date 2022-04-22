use crate::http::common::HTTPVersion;
use std::collections::HashMap;

pub struct StatusLine {
    version: HTTPVersion,
    status_code: u16,
    reason_phrase: String,
}

impl StatusLine {
    pub fn new(version: HTTPVersion, status_code: u16, reason_phrase: String) -> StatusLine {
        StatusLine {
            version,
            status_code,
            reason_phrase,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        format!(
            "{} {} {}",
            self.version, self.status_code, self.reason_phrase
        )
        .as_bytes()
        .to_owned()
    }
}

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

pub struct ResponseBody(Vec<u8>);

impl ResponseBody {
    pub fn new(data: Vec<u8>) -> ResponseBody {
        ResponseBody(data)
    }

    pub fn encode(&self) -> Vec<u8> {
        self.0.clone()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_response() {
        // setup
        let data = "hello".as_bytes();
        let response = Response::new(
            StatusLine::new(HTTPVersion::V1_1, 200, "OK".to_string()),
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
