use once_cell::sync::Lazy;

pub struct HTTPHeader<Parser: HeaderParser> {
    name: &'static str,
    parser: Parser,
}

impl<Parser: HeaderParser> HTTPHeader<Parser> {
    pub fn name(&self) -> &str {
        self.name
    }

    pub fn parse(&self, s: &str) -> Option<Parser::Value> {
        self.parser.parse(s)
    }
}

pub trait HeaderParser {
    type Value;

    fn parse(&self, s: &str) -> Option<Self::Value>;
}

pub struct VecHeaderParser;

impl HeaderParser for VecHeaderParser {
    type Value = Vec<String>;

    fn parse(&self, s: &str) -> Option<Self::Value> {
        Some(s.split(',').map(|x| x.trim().to_string()).collect())
    }
}

pub static CONNECTION: Lazy<HTTPHeader<VecHeaderParser>> = Lazy::new(|| HTTPHeader {
    name: "Connection",
    parser: VecHeaderParser,
});
