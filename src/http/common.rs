use std::fmt;
use std::fmt::Formatter;
use std::str::FromStr;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum HTTPVersion {
    V1_1,
}

impl FromStr for HTTPVersion {
    type Err = String;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        match str {
            "HTTP/1.1" => Ok(HTTPVersion::V1_1),
            _ => Err(format!("Illegal format as HTTP version: {}", str)),
        }
    }
}

impl fmt::Display for HTTPVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            HTTPVersion::V1_1 => f.write_str("HTTP/1.1"),
        }
    }
}
