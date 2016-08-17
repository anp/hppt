use std::ops::Deref;
use std::str::from_utf8;

use error::{HpptResult, HpptError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request<'a> {
    method: Method,
    uri: Uri<'a>,
    version: Version,
}

impl<'buf> Request<'buf> {
    pub fn from_bytes(bytes: &'buf [u8]) -> HpptResult<Request<'buf>> {
        // the standard says \r\n is the line terminator, but there are many non-conforming impls
        // so we'll split on newlines, and then trim the \r
        let mut lines = bytes.split(|&b| b == b'\n')
            .map(|l| {
                if l.len() == 0 {
                    l
                } else if l[l.len() - 1] == b'\r' {
                    &l[0..l.len() - 1]
                } else {
                    l
                }
            });

        // first line is the method/uri/version
        let request_line = match lines.next() {
            Some(l) => l,
            None => return Err(HpptError::IncompleteRequest),
        };

        let (method, uri, version) = try!(Self::parse_request_line(request_line));

        // TODO parse headers
        // TODO parse request bodies for non-GET requests

        Ok(Request {
            method: method,
            uri: uri,
            version: version,
        })
    }

    pub fn method(&self) -> Method {
        self.method
    }

    pub fn uri(&self) -> Uri {
        self.uri
    }

    fn parse_request_line<'a>(request_line: &'a [u8]) -> HpptResult<(Method, Uri<'a>, Version)> {
        let mut request_line_tokens = request_line.split(|&b| b == b' ');

        let method = match request_line_tokens.next() {
            Some(m) => try!(Method::from_bytes(m)),
            None => return Err(HpptError::IncompleteRequest),
        };

        let uri = match request_line_tokens.next() {
            Some(mut u) => {
                // URIs must have at least one character
                if u.len() > 0 {
                    // TODO validate URI has a protocol and known domain/is a relative path/etc.

                    // joining this uri onto a filesystem path won't work if has a preceding slash
                    if u[0] == b'/' {
                        u = &u[1..];
                    }

                    // FIXME: not compat on windows
                    match from_utf8(u) {
                        Ok(s) => Uri(s),
                        Err(_) => return Err(HpptError::Parsing),
                    }

                } else {
                    // this isn't an incomplete request -- we were able to get the next
                    // space-separated token but it's 0-length
                    return Err(HpptError::Parsing);
                }
            }
            None => return Err(HpptError::IncompleteRequest),
        };

        let version = match request_line_tokens.next() {
            Some(v) => try!(Version::from_bytes(v)),
            None => return Err(HpptError::IncompleteRequest),
        };

        Ok((method, uri, version))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Uri<'a>(&'a str);

impl<'a> Deref for Uri<'a> {
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Version {
    OneDotOne,
}

impl Version {
    pub fn from_bytes(version: &[u8]) -> HpptResult<Self> {

        // TODO find a way to differentiate between a partially filled version and an incorrect one

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
        let request_bytes = b"GET / HTTP/1.1\r\n\r\n";
        let expected = Request {
            method: Method::Get,
            uri: Uri(""),
            version: Version::OneDotOne,
        };

        let request = Request::from_bytes(request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_post() {
        let request_bytes = b"POST /posturi HTTP/1.1\r\nKey1=Value1&Key2=Value2+SpacedValue\r\n";
        let expected = Request {
            method: Method::Post,
            uri: Uri("posturi"),
            version: Version::OneDotOne,
        };

        let request = Request::from_bytes(request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_with_headers() {
        let request_bytes = b"GET /extended/path HTTP/1.1\r\nAccept-Charset: utf-8\r\n\r\n";
        let expected = Request {
            method: Method::Get,
            uri: Uri("extended/path"),
            version: Version::OneDotOne,
        };

        let request = Request::from_bytes(request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_get_ignore_body() {
        let request_bytes = b"GET /extended/path HTTP/1.1\r\nAccept-Charset: utf-8\r\n\r\n";
        let expected = Request {
            method: Method::Get,
            uri: Uri("extended/path"),
            version: Version::OneDotOne,
        };

        let request = Request::from_bytes(request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    #[should_panic]
    fn fail_empty() {
        let request_bytes = b"";
        Request::from_bytes(request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_only_newlines() {
        let request_bytes = b"\r\n\r\n";
        Request::from_bytes(request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_bad_version() {
        let request_bytes = b"GET / HTTP/0.9\r\n\r\n";
        Request::from_bytes(request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_bad_method() {
        let request_bytes = b"HRY / HTTP/1.1\r\n\r\n";
        Request::from_bytes(request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_no_method() {
        let request_bytes = b" / HTTP/1.1\r\n\r\n";
        Request::from_bytes(request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_missing_uri() {
        let request_bytes = b"GET HTTP/1.1\r\n\r\n";
        Request::from_bytes(request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_empty_uri() {
        let request_bytes = b"GET  HTTP/1.1\r\n\r\n";
        Request::from_bytes(request_bytes).unwrap();
    }

    // TODO test header parsing
    // TODO test for handling missing/too many newlines when request has a body
}
