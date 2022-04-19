use std::str::FromStr;

type RequestParseError = (u16, String);

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
            _ => Err((501, "Not Implemented".to_string())),
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
            _ => Err((400, "Bad Request".to_string())),
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
            return Err((400, "Bad Request".to_string()));
        }
        let method = RequestMethod::from_str(items[0])?;
        let path = items[1];
        let version = HTTPVersion::from_str(items[2])?;
        Ok(RequestLine::new(method, path, version))
    }
}

pub struct Request {
    request_line: RequestLine,
}

impl Request {
    pub fn new(request_line: RequestLine) -> Request {
        Request { request_line }
    }

    pub fn parse(str: &str) -> Result<Self, RequestParseError> {
        let mut lines = str.split("\r\n");
        let request_line =
            RequestLine::parse(lines.next().ok_or((400, "Bad Request".to_string()))?)?;
        Ok(Request::new(request_line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(matches!(actual, Err((501, _))));
    }

    #[test]
    fn test_parse_request() {
        let str = ["GET / HTTP/1.1", "", ""].join("\r\n");
        let actual = Request::parse(&str).unwrap();
        assert_eq!(actual.request_line.method, RequestMethod::GET);
    }
}
