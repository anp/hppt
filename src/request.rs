use std::io::Read;
use std::ops::Deref;
use std::str::from_utf8;

use error::{HpptResult, HpptError};

pub const BUF_SIZE: usize = 1024; // 1KB

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    method: Method,
    uri: Uri,
    query: Option<Query>,
    version: Version,
}

impl Request {
    pub fn from_bytes<R>(reader: &mut R) -> HpptResult<Request>
        where R: Read
    {
        let mut buf_offset = 0;
        let mut buf = [0u8; BUF_SIZE];

        // read from the least read offset until the buffer is either full
        // or we're out of bytes to read

        loop {
            let bytes_read = try!(reader.read(&mut buf[buf_offset..]));

            buf_offset += bytes_read;

            // handle full buffer
            if buf_offset == buf.len() {

                return Err(HpptError::RequestTooLarge);

            } else if bytes_read == 0 {

                // we've already continued the loop and attempted to re-read from the socket

                // the connection may produce further bytes down the line,
                // but is probably not going to
                // so the request is invalid
                return Err(HpptError::BadRequest);
            }

            let bytes = &buf[..buf_offset];

            // standard says \r\n is the line terminator, but there are many non-conforming impls
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
                None => continue,
            };

            let mut request_line_tokens = request_line.split(|&b| b == b' ');

            let method = match request_line_tokens.next() {
                Some(m) => {
                    match Method::from_bytes(m) {
                        Ok(m) => m,
                        Err(_) => continue,
                    }
                }
                None => continue,
            };

            let (uri, query) = match request_line_tokens.next() {
                Some(mut u) => {
                    // URIs must have at least one character
                    if u.len() > 0 {
                        // TODO validate URI has a protocol and known domain/is a relative path/etc.

                        // joining this uri onto an OS path won't work if has a preceding slash
                        if u[0] == b'/' {
                            u = &u[1..];
                        }

                        let uri = match from_utf8(u) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };

                        let mut halves = uri.split('?');

                        let uri = match halves.next() {
                            Some(u) => Uri(u.to_string()),
                            None => continue, // we need a first half of the URI
                        };

                        // but the post ? part of the URI is optional
                        let query = match halves.next() {
                            Some(q) => {
                                if q.len() > 0 {
                                    Some(Query(q.to_string()))
                                } else {
                                    None
                                }
                            }
                            None => None,
                        };

                        (uri, query)

                    } else {
                        // this isn't an incomplete request -- we were able to get the next
                        // space-separated token but it's 0-length
                        return Err(HpptError::Parsing);
                    }
                }
                None => continue,
            };

            let version = match request_line_tokens.next() {
                Some(v) => {
                    match Version::from_bytes(v) {
                        Ok(v) => v,
                        Err(HpptError::UnsupportedHttpVersion) => {
                            return Err(HpptError::UnsupportedHttpVersion)
                        }
                        Err(_) => continue,
                    }
                }
                None => continue,
            };

            return Ok(Request {
                method: method,
                uri: uri,
                query: query,
                version: version,
            });
        }
    }

    pub fn method(&self) -> Method {
        self.method
    }

    pub fn uri(&self) -> &Uri {
        &self.uri
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Uri(String);

impl Deref for Uri {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query(String);

impl Deref for Query {
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
        let mut request_bytes = "GET / HTTP/1.1\r\n\r\n".as_bytes();
        let expected = Request {
            method: Method::Get,
            uri: Uri("".to_string()),
            query: None,
            version: Version::OneDotOne,
        };

        let request = Request::from_bytes(&mut request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_post() {
        let mut request_bytes = "POST /posturi HTTP/1.1\r\nKey1=Value1&Key2=Value2+SpacedValue\r\n"
            .as_bytes();
        let expected = Request {
            method: Method::Post,
            uri: Uri("posturi".to_string()),
            query: None,
            version: Version::OneDotOne,
        };

        let request = Request::from_bytes(&mut request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_with_headers() {
        let mut request_bytes = "GET /extended/path HTTP/1.1\r\nAccept-Charset: utf-8\r\n\r\n"
            .as_bytes();
        let expected = Request {
            method: Method::Get,
            uri: Uri("extended/path".to_string()),
            query: None,
            version: Version::OneDotOne,
        };

        let request = Request::from_bytes(&mut request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_with_query() {
        let mut request_bytes = "GET /extended/path?key1=val1&key2=val2 HTTP/1.1\r
Accept-Charset: utf-8\r
\r
"
            .as_bytes();
        let expected = Request {
            method: Method::Get,
            uri: Uri("extended/path".to_string()),
            query: Some(Query("key1=val1&key2=val2".to_string())),
            version: Version::OneDotOne,
        };

        let request = Request::from_bytes(&mut request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_with_empty_query() {
        let mut request_bytes = "GET /extended/path? HTTP/1.1\r
Accept-Charset: utf-8\r
\r
"
            .as_bytes();
        let expected = Request {
            method: Method::Get,
            uri: Uri("extended/path".to_string()),
            query: None,
            version: Version::OneDotOne,
        };

        let request = Request::from_bytes(&mut request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    fn successful_get_ignore_body() {
        let mut request_bytes = "GET /extended/path HTTP/1.1\r\nAccept-Charset: utf-8\r\n\r\n"
            .as_bytes();
        let expected = Request {
            method: Method::Get,
            uri: Uri("extended/path".to_string()),
            query: None,
            version: Version::OneDotOne,
        };

        let request = Request::from_bytes(&mut request_bytes).unwrap();

        assert_eq!(request, expected);
    }

    #[test]
    #[should_panic]
    fn fail_empty() {
        let mut request_bytes = "".as_bytes();
        Request::from_bytes(&mut request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_only_newlines() {
        let mut request_bytes = "\r\n\r\n".as_bytes();
        Request::from_bytes(&mut request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_bad_version() {
        let mut request_bytes = "GET / HTTP/0.9\r\n\r\n".as_bytes();
        Request::from_bytes(&mut request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_bad_method() {
        let mut request_bytes = "HRY / HTTP/1.1\r\n\r\n".as_bytes();
        Request::from_bytes(&mut request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_no_method() {
        let mut request_bytes = " / HTTP/1.1\r\n\r\n".as_bytes();
        Request::from_bytes(&mut request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_missing_uri() {
        let mut request_bytes = "GET HTTP/1.1\r\n\r\n".as_bytes();
        Request::from_bytes(&mut request_bytes).unwrap();
    }

    #[test]
    #[should_panic]
    fn fail_empty_uri() {
        let mut request_bytes = "GET  HTTP/1.1\r\n\r\n".as_bytes();
        Request::from_bytes(&mut request_bytes).unwrap();
    }

    // TODO test header parsing
    // TODO test for handling missing/too many newlines when request has a body
}
