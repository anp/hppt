use std::ops::Deref;
use std::str::from_utf8;

use error::{HpptResult, HpptError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request<'a> {
    method: Method,
    uri: Uri<'a>,
    query: Option<Query<'a>>,
    version: Version,
    header_lines: Vec<&'a str>,
    body: &'a [u8],
}

impl<'a> Request<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> HpptResult<Request<'a>> {

        // standard says \r\n is the line terminator, but there are many non-conforming impls
        // so we'll split on newlines, and then trim the \r

        let mut body_start = 0;
        let method;
        let uri;
        let query;
        let version;
        let mut headers = Vec::new();

        {
            let mut lines = bytes.split(|&b| b == b'\n')
                .map(|l| {
                    if l.len() == 0 {
                        l
                    } else if l[l.len() - 1] == b'\r' {
                        body_start += l.len() + 1; // the \n byte was stripped
                        &l[0..l.len() - 1]
                    } else {
                        body_start += l.len() + 1; // the \n byte was stripped
                        l
                    }
                });

            // first line is the method/uri/version
            let request_line = match lines.next() {
                Some(l) => l,
                None => return Err(HpptError::Parsing),
            };

            let mut request_line_tokens = request_line.split(|&b| b == b' ');

            method = match request_line_tokens.next() {
                Some(m) => {
                    match Method::from_bytes(m) {
                        Ok(m) => m,
                        Err(_) => return Err(HpptError::Parsing),
                    }
                }
                None => return Err(HpptError::Parsing),
            };

            match request_line_tokens.next() {
                Some(mut u) => {
                    // URIs must have at least one character
                    if u.len() > 0 {

                        // joining this uri onto an OS path won't work if has a preceding slash
                        if u[0] == b'/' {
                            u = &u[1..];
                        }

                        let uri_fromstr = match from_utf8(u) {
                            Ok(s) => s,
                            Err(_) => return Err(HpptError::Parsing),
                        };

                        let mut halves = uri_fromstr.split('?');

                        let uri_parsed = match halves.next() {
                            Some(u) => Uri(u),
                            None => return Err(HpptError::Parsing), // need a first half of the URI
                        };

                        // but the post ? part of the URI is optional
                        let query_parsed = match halves.next() {
                            Some(q) => if q.len() > 0 { Some(Query(q)) } else { None },
                            None => None,
                        };

                        uri = uri_parsed;
                        query = query_parsed;

                    } else {
                        // this isn't an incomplete request -- we were able to get the next
                        // space-separated token but it's 0-length
                        return Err(HpptError::Parsing);
                    }
                }
                None => return Err(HpptError::Parsing),
            }

            version = match request_line_tokens.next() {
                Some(v) => {
                    match Version::from_bytes(v) {
                        Ok(v) => v,
                        Err(HpptError::UnsupportedHttpVersion) => {
                            return Err(HpptError::UnsupportedHttpVersion)
                        }
                        Err(_) => return Err(HpptError::Parsing),
                    }
                }
                None => return Err(HpptError::Parsing),
            };

            // SIDE EFFECTFUL -- parsing each line will increment out body_start value
            for l in lines.take_while(|l| l.len() > 0) {
                match from_utf8(l) {
                    Ok(s) => headers.push(s),
                    Err(_) => return Err(HpptError::Parsing),
                }
            }
        }


        let request = Request {
            method: method,
            uri: uri,
            query: query,
            version: version,
            header_lines: headers,
            body: &bytes[body_start..],
        };

        debug!("request parsed: {:?}", &request);

        Ok(request)
    }

    pub fn method(&self) -> Method {
        self.method
    }

    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    pub fn query(&self) -> Option<&Query> {
        self.query.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Uri<'a>(&'a str);

impl<'a> Deref for Uri<'a> {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query<'a>(&'a str);

impl<'a> Deref for Query<'a> {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    Options,
    Get,
    Head,
    Post,
    Put,
    Delete,
    Trace,
    Connect,
}

impl Method {
    pub fn from_bytes(method: &[u8]) -> HpptResult<Self> {

        // TODO find a way to differentiate between a partially filled version and an incorrect one

        match method {
            b"OPTIONS" => Ok(Method::Options),
            b"GET" => Ok(Method::Get),
            b"HEAD" => Ok(Method::Head),
            b"POST" => Ok(Method::Post),
            b"PUT" => Ok(Method::Put),
            b"DELETE" => Ok(Method::Delete),
            b"TRACE" => Ok(Method::Trace),
            b"CONNECT" => Ok(Method::Connect),
            _ => Err(HpptError::Parsing),
        }
    }

    pub fn as_bytes(&self) -> &'static str {
        match *self {
            Method::Options => "OPTIONS",
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Trace => "TRACE",
            Method::Connect => "CONNECT",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Version {
    OneDotOne,
}

impl Version {
    pub fn from_bytes(version: &[u8]) -> HpptResult<Self> {

        // only support HTTP/1.1 at the moment
        match version {
            b"HTTP/1.1" => Ok(Version::OneDotOne),
            _ => return Err(HpptError::UnsupportedHttpVersion),
        }
    }
}


#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn successful_get() {
        let request_bytes = "GET / HTTP/1.1\r\n\r\n".as_bytes();
        let expected = Request {
            method: Method::Get,
            uri: Uri(""),
            query: None,
            version: Version::OneDotOne,
            body: b"",
            header_lines: Vec::new(),
        };

        let request = Request::from_bytes(&request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_post() {
        let request_bytes = "POST /posturi HTTP/1.1\r\n\r\nKey1=Value1&Key2=Value2+SpacedValue"
            .as_bytes();
        let expected = Request {
            method: Method::Post,
            uri: Uri("posturi"),
            query: None,
            version: Version::OneDotOne,
            body: b"Key1=Value1&Key2=Value2+SpacedValue",
            header_lines: Vec::new(),
        };

        let request = Request::from_bytes(&request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_with_headers() {
        let request_bytes = "GET /extended/path HTTP/1.1\r\nAccept-Charset: utf-8\r\n\r\n"
            .as_bytes();
        let expected = Request {
            method: Method::Get,
            uri: Uri("extended/path"),
            query: None,
            version: Version::OneDotOne,
            body: b"",
            header_lines: vec!["Accept-Charset: utf-8"],
        };

        let request = Request::from_bytes(&request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_with_query() {
        let request_bytes = "GET /extended/path?key1=val1&key2=val2 HTTP/1.1\r
Accept-Charset: utf-8\r
\r
"
            .as_bytes();
        let expected = Request {
            method: Method::Get,
            uri: Uri("extended/path"),
            query: Some(Query("key1=val1&key2=val2")),
            version: Version::OneDotOne,
            body: b"",
            header_lines: vec!["Accept-Charset: utf-8"],
        };

        let request = Request::from_bytes(&request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_with_empty_query() {
        let request_bytes = "GET /extended/path? HTTP/1.1\r
Accept-Charset: utf-8\r
\r
"
            .as_bytes();
        let expected = Request {
            method: Method::Get,
            uri: Uri("extended/path"),
            query: None,
            version: Version::OneDotOne,
            body: b"",
            header_lines: vec!["Accept-Charset: utf-8"],
        };

        let request = Request::from_bytes(&request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_get_ignore_body() {
        let request_bytes = "GET /extended/path HTTP/1.1\r\nAccept-Charset: utf-8\r\n\r\n"
            .as_bytes();
        let expected = Request {
            method: Method::Get,
            uri: Uri("extended/path"),
            query: None,
            version: Version::OneDotOne,
            body: b"",
            header_lines: vec!["Accept-Charset: utf-8"],
        };

        let request = Request::from_bytes(&request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    #[should_panic]
    fn fail_empty() {
        let request_bytes = "".as_bytes();
        let request = Request::from_bytes(&request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_only_newlines() {
        let request_bytes = "\r\n\r\n".as_bytes();
        let request = Request::from_bytes(&request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_bad_version() {
        let request_bytes = "GET / HTTP/0.9\r\n\r\n".as_bytes();
        let request = Request::from_bytes(&request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_bad_method() {
        let request_bytes = "HRY / HTTP/1.1\r\n\r\n".as_bytes();
        let request = Request::from_bytes(&request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_no_method() {
        let request_bytes = " / HTTP/1.1\r\n\r\n".as_bytes();
        let request = Request::from_bytes(&request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_missing_uri() {
        let request_bytes = "GET HTTP/1.1\r\n\r\n".as_bytes();
        let request = Request::from_bytes(&request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_empty_uri() {
        let request_bytes = "GET  HTTP/1.1\r\n\r\n".as_bytes();
        let request = Request::from_bytes(&request_bytes).unwrap();
    }

    // TODO test header parsing
    // TODO test for handling missing/too many newlines when request has a body
}
