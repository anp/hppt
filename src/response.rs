use std::io::{Read, Write};

use error::*;

pub enum Status {
    Ok,
    BadRequest,
    NotFound,
    RequestEntityTooLarge,
    InternalServerError,
    NotImplemented,
    HttpVersionNotSupported,
}

pub struct Response {
    status: Status,
    response_type: ResponseType,
}

pub enum ResponseType {
    General(Option<Box<Read>>, Option<ContentType>),
}

impl Response {
    pub fn new(status: Status, response_type: ResponseType) -> Response {
        Response {
            status: status,
            response_type: response_type,
        }
    }

    fn status_line(&self) -> &'static [u8] {
        match self.status {
            Status::Ok => b"HTTP/1.1 200 OK\r\n",
            Status::BadRequest => b"HTTP/1.1 400 Bad Request\r\n",
            Status::NotFound => b"HTTP/1.1 404 Not Found\r\n",
            Status::RequestEntityTooLarge => b"HTTP/1.1 413 Request Entity Too Large\r\n",
            Status::InternalServerError => b"HTTP/1.1 500 Internal Server Error\r\n",
            Status::NotImplemented => b"HTTP/1.1 501 Not Implemented\r\n",
            Status::HttpVersionNotSupported => b"HTTP/1.1 505 HTTP Version not supported\r\n",
        }
    }

    pub fn send<C: Write>(self, mut target: C) -> HpptResult<()> {

        // from http 1.1 spec:
        //
        // Response      = Status-Line               ; Section 6.1
        // (( general-header                         ; Section 4.5
        // | response-header                         ; Section 6.2
        // | entity-header ) CRLF)                   ; Section 7.1
        // CRLF
        // [ message-body ]                          ; Section 7.2

        // from http 1.1 spec:
        // Status-Line = HTTP-Version SP Status-Code SP Reason-Phrase CRLF

        // TODO collect these into a local buffer to avoid multiple syscalls

        let mut buf = Vec::with_capacity(100);

        let status = self.status_line();

        match self.response_type {
            ResponseType::General(data, content_type) => {
                buf.extend_from_slice(status);

                // TODO write any headers here
                buf.extend_from_slice(b"Content-Length: ");

                let mut content_buf = Vec::new();

                // write message body (usually file contents) if present
                if let Some(mut data) = data {
                    // shuffle bytes from the data source (usually a file)
                    // to the target (usually a socket)
                    try!(data.read_to_end(&mut content_buf));
                }

                buf.extend_from_slice(&format!("{}\r\n", content_buf.len()).as_bytes());

                if let Some(ct) = content_type {
                    buf.extend_from_slice(b"Content-Type: ");
                    buf.extend_from_slice(ct.as_bytes());
                    buf.extend_from_slice(b"\r\n");
                }

                buf.extend_from_slice(b"\r\n");
                buf.extend_from_slice(&content_buf);
            }
        }

        try!(target.write_all(&buf));

        Ok(())
    }
}

pub enum ContentType {
    Html,
    Text,
    Markdown,
    Pdf,
    Binary,
}

impl ContentType {
    pub fn from_path(path: &str) -> Self {
        let extension_offset = match path.rfind('.') {
            Some(o) => o,
            None => return ContentType::Binary,
        };

        let (_, extension) = path.split_at(extension_offset + 1);

        match extension {
            "htm" => ContentType::Html,
            "html" => ContentType::Html,
            "toml" => ContentType::Text,
            "txt" => ContentType::Text,
            "md" => ContentType::Markdown,
            "pdf" => ContentType::Pdf,
            _ => ContentType::Binary,
        }
    }

    pub fn as_bytes(&self) -> &'static [u8] {
        match *self {
            ContentType::Html => b"text/html",
            ContentType::Text => b"text/plain",
            ContentType::Pdf => b"application/pdf",
            ContentType::Markdown => b"text/markdown",
            ContentType::Binary => b"application/octet-stream",
        }
    }
}

#[cfg(test)]
mod test {
    use std::str;
    use super::*;

    fn check_response_write(response: Response, expected: &[u8]) {
        let mut recv_buf = Vec::new();

        response.send(&mut recv_buf).unwrap();

        if recv_buf != expected {
            let received = str::from_utf8(&recv_buf);
            let expected = str::from_utf8(expected);

            assert_eq!(received, expected);
        }
    }

    #[test]
    fn empty() {
        let response = Response::new(Status::Ok,
                                     ResponseType::General(None, Some(ContentType::Text)));
        let expected = b"HTTP/1.1 200 OK\r
Content-Length: 0\r
Content-Type: text/plain\r
\r
";

        check_response_write(response, expected);
    }

    #[test]
    fn with_text() {
        let response = Response::new(Status::Ok,
                                     ResponseType::General(Some(Box::new("ABCDEFGHIJK1234567890"
                                                             .as_bytes())),
                                                         Some(ContentType::Text)));
        let expected = b"HTTP/1.1 200 OK\r
Content-Length: 21\r
Content-Type: text/plain\r
\r
ABCDEFGHIJK1234567890";

        check_response_write(response, expected);
    }

    #[test]
    fn not_found() {
        let response = Response::new(Status::NotFound, ResponseType::General(None, None));
        let expected = b"HTTP/1.1 404 Not Found\r
Content-Length: 0\r
\r
";

        check_response_write(response, expected);
    }
}
