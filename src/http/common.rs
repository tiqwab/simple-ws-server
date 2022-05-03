use chrono::{DateTime, Utc};
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

/// Date format used in HTTP header
/// See RFC7231 7.1.1
pub struct IMFDateTime(DateTime<Utc>);

impl IMFDateTime {
    #[allow(dead_code)]
    pub fn new(inner: DateTime<Utc>) -> IMFDateTime {
        IMFDateTime(inner)
    }

    pub fn now() -> IMFDateTime {
        IMFDateTime(Utc::now())
    }
}

impl fmt::Display for IMFDateTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_imf_datetime_format() {
        let dt = IMFDateTime::new(Utc.ymd(2022, 4, 26).and_hms(12, 24, 36));
        assert_eq!(format!("{}", dt), "Tue, 26 Apr 2022 12:24:36 GMT");
    }
}
