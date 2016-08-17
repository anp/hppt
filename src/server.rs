use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam::sync::MsQueue;
use mioco;
use mioco::tcp::TcpListener;

use error::*;
use files::find_file_relative;
use request::{Method, Request};
use response::{ContentType, Response, Status};

pub type NThreads = usize;

const BUF_SIZE: usize = 1024; // 1KB

pub fn run(addr: SocketAddr,
           root_dir: &Path,
           shutdown: Arc<MsQueue<u8>>,
           num_threads: NThreads)
           -> HpptResult<()> {
    let listener = try!(TcpListener::bind(&addr));

    info!("Server listening on {:?}", listener.local_addr().unwrap());
    let root_dir = root_dir.to_path_buf();

    mioco::start_threads(num_threads, move || {
            // spawn one listener coroutine per event loop worker
            for _ in 0..num_threads {

                // need a copy of the listener and content path for our multiple listener threads
                let root_dir = root_dir.clone();
                let local_listener = listener.try_clone().unwrap();

                // the shutdown queue needs to be 'static, so we'll clone the Arc
                let shutdown = shutdown.clone();

                mioco::spawn(move || {
                    loop {
                        // if we get a shutdown notice, stop listening for requests
                        if let Some(_) = shutdown.try_pop() {
                            break;
                        }

                        // this will block the coroutine until a connection is available
                        let connection = local_listener.accept().unwrap();
                        let root_dir = root_dir.clone();

                        debug!("Connection established with {:?}",
                               connection.peer_addr().unwrap());

                        // once we have a connection, handle the request
                        mioco::spawn(|| parse_and_handle_request(connection, root_dir));
                    }
                });

            }
        })
        .unwrap();
    // TODO improve error reporting from initializing the server

    Ok(())
}

fn parse_and_handle_request<C>(mut connection: C, root_dir: PathBuf)
    where C: Read + Write
{
    let mut buf_offset = 0;
    let mut buf = [0u8; BUF_SIZE];

    'readreq: loop {

        // read from the least read offset until the buffer is either full
        // or we're out of bytes to read
        let bytes_read = match connection.read(&mut buf[buf_offset..]) {
            Ok(n) => n,
            Err(why) => {
                error!("Unable to read from socket: {:?}", why);
                return;
            }
        };

        buf_offset += bytes_read;

        // now that we have a potential request, handle the request
        let (status, mut reader, content_type) = {

            // handle full buffer
            if buf_offset == buf.len() {
                (Status::RequestEntityTooLarge, None, None)

            } else if bytes_read == 0 {

                // the connection may produce further bytes down the line,
                // but is probably not going to

                // if EOF has been reached but we still got an incomplete request on the last loop,
                // then the request is probably invalid
                (Status::BadRequest, None, None)

            } else {
                let request = Request::from_bytes(&buf[..buf_offset]);

                match request {

                    // the request parsed successfully
                    Ok(req) => {

                        if req.method() == Method::Get {
                            let uri = &*req.uri();
                            let file = find_file_relative(&root_dir, Path::new(uri));

                            match file {
                                Some(f) => (Status::Ok, Some(f), Some(ContentType::from_path(uri))),
                                None => (Status::NotFound, None, None),
                            }

                        } else {
                            // we don't support anything other than GET right now
                            (Status::NotImplemented, None, None)
                        }
                    }

                    // we couldn't parse the request
                    Err(why) => {
                        match why {
                            HpptError::UnsupportedHttpVersion => {
                                (Status::HttpVersionNotSupported, None, None)
                            }
                            HpptError::Parsing => (Status::BadRequest, None, None),
                            HpptError::IoError(why) => {
                                error!("Internal I/O error: {:?}", why);
                                (Status::InternalServerError, None, None)
                            }
                            // if the request doesn't have enough tokens (i.e. is an incomplete
                            // request) then we'll loop again to add more to the buffer
                            HpptError::IncompleteRequest => continue 'readreq,
                        }
                    }
                }
            }
        };

        let response = Response::new(status);

        match response.send(&mut connection, reader.as_mut(), content_type) {
            Ok(()) => {
                // TODO debug log request
                return;
            }
            Err(_why) => {
                // TODO error log failure
                return;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::net::{Shutdown, SocketAddr, TcpStream};
    use std::path::Path;
    use std::str;
    use std::str::FromStr;
    use std::sync::Arc;
    use std::thread::{JoinHandle, sleep, spawn};
    use std::time::Duration;

    use crossbeam::sync::MsQueue;

    use ::init_logging;
    use error::HpptResult;

    use super::*;

    /// A RAII-style handle to our mioco server so that we can spawn one and shut it down for each
    /// test, using separate ports.
    struct TestServerHandle {
        num_threads: usize,
        address: SocketAddr,
        queue: Arc<MsQueue<u8>>,
        server: Option<JoinHandle<HpptResult<()>>>,
    }

    // TODO randomly pick server ports and try them until one binds

    impl TestServerHandle {
        pub fn new(port: u16) -> Self {

            // set to true to get more verbose debug logging
            init_logging(false);

            let num_test_threads = 2;
            let test_server_address = format!("127.0.0.1:{}", port);

            let queue = Arc::new(MsQueue::new());
            let server_queue = queue.clone();

            let addr = SocketAddr::from_str(&test_server_address).unwrap();

            debug!("Initializing test server at {} with {} threads...",
                   &test_server_address,
                   num_test_threads);

            let server = spawn(move || {
                run(addr,
                    &Path::new(env!("CARGO_MANIFEST_DIR")),
                    server_queue,
                    num_test_threads)
            });

            debug!("Test server initialized.");

            // FIXME find a better way to check for server liveness before returning
            sleep(Duration::from_millis(1000));

            TestServerHandle {
                num_threads: num_test_threads,
                address: addr,
                queue: queue.clone(),
                server: Some(server),
            }
        }

        pub fn make_request(&self, request: &[u8]) -> Vec<u8> {
            debug!("Making request to {}...", self.address);
            let mut buf = Vec::new();

            let mut connection = TcpStream::connect(self.address).unwrap();

            connection.write_all(request).unwrap();

            connection.shutdown(Shutdown::Write).unwrap();

            connection.read_to_end(&mut buf).unwrap();

            buf
        }
    }

    impl Drop for TestServerHandle {
        fn drop(&mut self) {
            debug!("Sending poison pills to test server listener coroutines @ {:?}...",
                   self.address);

            for _ in 0..(self.num_threads * 3) {
                self.queue.push(0); // kill a coroutine, several times over
            }

            // try to get the coroutines to eat the poison pills
            // FIXME need a better cancellation method
            for _ in 0..(self.num_threads * 3) {

                if let Ok(c) = TcpStream::connect(self.address) {
                    c.shutdown(Shutdown::Both).unwrap();
                }
            }

            debug!("Waiting on test server @ {:?} to finish handling requests...",
                   self.address);

            let server = self.server.take().unwrap();

            server.join().unwrap().unwrap();

            debug!("Test server @ {:?} exited.", self.address)
        }
    }

    #[test]
    fn server_handle_drop() {
        let _server = TestServerHandle::new(8080);
    }

    #[test]
    fn not_found() {
        let server = TestServerHandle::new(8081);

        let response = server.make_request(b"GET /DOES_NOT_EXIST HTTP/1.1");

        check_bytes_utf8(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n",
                         &response);
    }

    #[test]
    fn file_contents_text() {
        let server = TestServerHandle::new(8082);

        let filename = "Cargo.toml";

        let response = server.make_request(&format!("GET /{} HTTP/1.1", &filename).as_bytes());

        let mut expected = Vec::new();

        // need to prepopulate the expected response headers before the file data
        expected.extend_from_slice(b"HTTP/1.1 200 OK\r
Content-Length: 345\r
Content-Type: text/plain\r
\r
");

        File::open(&filename).unwrap().read_to_end(&mut expected).unwrap();

        check_bytes_utf8(&expected, &response);
    }

    #[test]
    fn file_contents_html() {
        let server = TestServerHandle::new(8090);

        let filename = "test/foo.html";

        let response = server.make_request(&format!("GET /{} HTTP/1.1", &filename).as_bytes());

        let mut expected = Vec::new();

        // need to prepopulate the expected response headers before the file data
        expected.extend_from_slice(b"HTTP/1.1 200 OK\r
Content-Length: 28\r
Content-Type: text/html\r
\r
");

        File::open(&filename).unwrap().read_to_end(&mut expected).unwrap();

        check_bytes_utf8(&expected, &response);
    }

    #[test]
    fn file_contents_binary() {
        let server = TestServerHandle::new(8087);

        let filename = "test/1k.bin";

        let response = server.make_request(&format!("GET /{} HTTP/1.1", &filename).as_bytes());

        let mut expected = Vec::new();

        // need to prepopulate the expected response headers before the file data
        expected.extend_from_slice(b"HTTP/1.1 200 OK\r
Content-Length: 1024\r
Content-Type: application/octet-stream\r
\r
");

        File::open(&filename).unwrap().read_to_end(&mut expected).unwrap();

        check_bytes_utf8(&expected, &response);
    }

    #[test]
    fn multiple_requests() {
        let server = TestServerHandle::new(8086);

        let filename = "Cargo.toml";
        let mut expected = Vec::new();

        // need to prepopulate the expected response headers before the file data
        expected.extend_from_slice(b"HTTP/1.1 200 OK\r
Content-Length: 345\r
Content-Type: text/plain\r
\r
");

        File::open(&filename).unwrap().read_to_end(&mut expected).unwrap();

        for _ in 0..100 {

            let response = server.make_request(&format!("GET /{} HTTP/1.1", &filename).as_bytes());
            check_bytes_utf8(&expected, &response);
        }
    }

    #[test]
    #[should_panic]
    fn large_request() {
        let server = TestServerHandle::new(8083);

        let mut request = Vec::new();
        request.extend_from_slice(b"GET /");
        request.extend_from_slice(&[b'a'; 1024]);
        request.extend_from_slice(b" HTTP/1.1");

        let _response = server.make_request(&request);
    }

    #[test]
    fn unimplemented() {
        let server = TestServerHandle::new(8084);

        let unsupported_requests = ["PUT / HTTP/1.1",
                                    "OPTIONS / HTTP/1.1",
                                    "HEAD / HTTP/1.1",
                                    "POST / HTTP/1.1",
                                    "PUT / HTTP/1.1",
                                    "DELETE / HTTP/1.1",
                                    "TRACE / HTTP/1.1",
                                    "CONNECT / HTTP/1.1"];

        for request in unsupported_requests.iter() {
            let response = server.make_request(&request.as_bytes());
            check_bytes_utf8(b"HTTP/1.1 501 Not Implemented\r\nContent-Length: 0\r\n\r\n",
                             &response);
        }

    }

    #[test]
    fn wrong_http_version() {
        let server = TestServerHandle::new(8085);

        let response = server.make_request(b"GET / HTTP/1.0");

        check_bytes_utf8(b"HTTP/1.1 505 HTTP Version not supported\r\nContent-Length: 0\r\n\r\n",
                         &response);
    }

    fn check_bytes_utf8(expected: &[u8], response: &[u8]) {
        let expected = Vec::from(expected);
        let response = Vec::from(response);

        if expected != response {
            let expected = String::from_utf8_lossy(&expected);
            let response = String::from_utf8_lossy(&response);

            assert_eq!(response, expected);
        }
    }
}
