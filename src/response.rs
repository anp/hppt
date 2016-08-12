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
}

impl Response {
    pub fn new(status: Status) -> Self {
        Response {
            status: status,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        match self.status {
            Status::Ok => b"HTTP/1.1 200 OK\r\n\r\n",
            Status::BadRequest => b"HTTP/1.1 400 Bad Request\r\n\r\n",
            Status::NotFound => b"HTTP/1.1 404 Not Found\r\n\r\n",
            Status::RequestEntityTooLarge => b"HTTP/1.1 413 Request Entity Too Large\r\n\r\n",
            Status::InternalServerError => b"HTTP/1.1 500 Internal Server Error\r\n\r\n",
            Status::NotImplemented => b"HTTP/1.1 501 Not Implemented\r\n\r\n",
            Status::HttpVersionNotSupported => b"HTTP/1.1 505 HTTP Version not supported\r\n\r\n",
        }
    }

    pub fn send<C: Write, R: Read>(&self,
                                   mut target: C,
                                   data: Option<R>)
                                   -> HpptResult<()> {

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

        buf.extend_from_slice(self.as_bytes());

        // TODO write any headers here

        // write message body (usually file contents) if present
        if let Some(mut data) = data {
            // shuffle bytes from the data source (usually a file) to the target (usually a socket)
            try!(data.read_to_end(&mut buf));
        }

        try!(target.write_all(&buf));

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::str;
    use super::*;

    fn check_response_write(response: Response, data: Option<&[u8]>, expected: &[u8]) {
        let mut recv_buf = Vec::new();

        response.send(&mut recv_buf, data).unwrap();

        if recv_buf != expected {
            let received = str::from_utf8(&recv_buf);
            let expected = str::from_utf8(expected);

            assert_eq!(received, expected);
        }
    }

    #[test]
    fn empty() {
        let response = Response::new(Status::Ok);
        let expected = b"HTTP/1.1 200 OK\r\n\r\n";

        check_response_write(response, None, expected);
    }

    #[test]
    fn with_text() {
        let response = Response::new(Status::Ok);
        let expected = b"HTTP/1.1 200 OK\r\n\r\nABCDEFGHIJK1234567890";

        check_response_write(response, Some(b"ABCDEFGHIJK1234567890"), expected);
    }

    #[test]
    fn not_found() {
        let response = Response::new(Status::NotFound);
        let expected = b"HTTP/1.1 404 Not Found\r\n\r\n";

        check_response_write(response, None, expected);
    }

    // TODO tests with response headers (once implemented)
}
