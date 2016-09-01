use std::ffi::OsStr;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Receiver;

use mioco;
use mioco::tcp::TcpListener;

use error::*;
use files::find_file_relative;
use request::{Method, Request};
use response::{ContentType, Response, Status};

pub type NThreads = usize;

pub fn run(listener: TcpListener,
           root_dir: &Path,
           shutdown: Receiver<()>,
           num_threads: NThreads)
           -> HpptResult<()> {

    info!("Server listening on {:?}", listener.local_addr().unwrap());
    let root_dir = root_dir.to_path_buf();

    mioco::start_threads(num_threads, move || {
            loop {
                // if we get a shutdown notice, stop listening for requests
                if let Ok(()) = shutdown.try_recv() {
                    break;
                }

                // this will block the coroutine until a connection is available
                let connection = listener.accept().unwrap();
                let root_dir = root_dir.clone();

                debug!("Connection established with {:?}",
                       connection.peer_addr().unwrap());

                // once we have a connection, handle the request
                mioco::spawn(move || handle_request(connection, root_dir));
            }
        })
        .unwrap();
    // TODO improve error reporting from initializing the server

    Ok(())
}

const BUF_SIZE: usize = 1024; // 1KB

fn handle_request<C>(mut connection: C, root_dir: PathBuf) -> HpptResult<()>
    where C: Read + Write
{

    let mut buf = [0; BUF_SIZE];
    let mut buf_offset = 0;
    let mut error = None;

    loop {
        let bytes_read = try!(connection.read(&mut buf[buf_offset..]));

        buf_offset += bytes_read;

        // handle full buffer
        if buf_offset == buf.len() {

            error = Some(HpptError::RequestTooLarge);
            break;

        } else if bytes_read == 0 {
            break;
        }
    }

    let response = if let Some(e) = error {
        match e {
            HpptError::UnsupportedHttpVersion => {
                Response::new(Status::HttpVersionNotSupported, None, None, false)
            }
            HpptError::Parsing => Response::new(Status::BadRequest, None, None, false),
            HpptError::IoError(why) => {
                error!("Internal I/O error: {:?}", why);
                Response::new(Status::InternalServerError, None, None, false)
            }
            HpptError::RequestTooLarge => {
                Response::new(Status::RequestEntityTooLarge, None, None, false)
            }
        }
    } else {
        match Request::from_bytes(&buf[..buf_offset]) {

            Ok(req) => {

                if req.method() == Method::Get {
                    let uri: &OsStr = req.uri().as_ref();

                    if let Some((file, full_path)) = find_file_relative(&root_dir, Path::new(uri)) {
                        let is_cgi = req.uri().starts_with("cgi-bin");

                        if is_cgi {

                            build_cgi_response(&req, &full_path)

                        } else {
                            Response::new(Status::Ok,
                                          Some(Box::new(file)),
                                          Some(ContentType::from_path(req.uri())),
                                          false)
                        }
                    } else {
                        Response::new(Status::NotFound, None, None, false)
                    }

                } else {
                    // we don't support anything other than GET right now
                    Response::new(Status::NotImplemented, None, None, false)
                }
            }

            Err(why) => {
                match why {
                    HpptError::UnsupportedHttpVersion => {
                        Response::new(Status::HttpVersionNotSupported, None, None, false)
                    }
                    HpptError::Parsing => Response::new(Status::BadRequest, None, None, false),
                    HpptError::IoError(why) => {
                        error!("Internal I/O error: {:?}", why);
                        Response::new(Status::InternalServerError, None, None, false)
                    }
                    HpptError::RequestTooLarge => {
                        Response::new(Status::RequestEntityTooLarge, None, None, false)
                    }
                }
            }
        }
    };

    try!(response.send(&mut connection));

    Ok(())
}

fn build_cgi_response(req: &Request, exe_file: &Path) -> Response {
    match build_command(&req, &exe_file).output() {
        Ok(output) => {
            if output.status.success() {
                Response::new(Status::Ok,
                              Some(Box::new(Cursor::new(output.stdout))),
                              None,
                              true)
            } else {
                Response::new(Status::BadRequest,
                              Some(Box::new(Cursor::new(output.stdout))),
                              None,
                              true)
            }
        }
        Err(_) => Response::new(Status::BadRequest, None, None, false),
    }
}

fn build_command(req: &Request, exe_file: &Path) -> Command {
    let mut cmd = Command::new(exe_file);

    cmd.env("SERVER_SOFTWARE",
            concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")));
    cmd.env("SERVER_NAME", ""); // TODO put the IP address here
    cmd.env("GATEWAY_INTERFACE", "CGI/1.1");
    cmd.env("SERVER_PROTOCOL", "HTTP/1.1");
    cmd.env("SERVER_PORT", ""); // TODO put the listen port here
    cmd.env("REQUEST_METHOD", req.method().as_bytes());
    cmd.env("REMOTE_ADDR", ""); // TODO put the client IP address here

    if let Some(ref query_str) = req.query() {
        let query_str: &str = &*query_str;
        cmd.env("QUERY_STRING", query_str);
    }

    cmd
}

#[cfg(test)]
mod test {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::net::{Shutdown, SocketAddr, TcpStream};
    use std::path::Path;
    use std::str;
    use std::str::FromStr;
    use std::sync::mpsc;
    use std::thread::{JoinHandle, sleep, spawn};
    use std::time::Duration;

    use mioco::tcp::TcpListener;

    use ::init_logging;
    use error::HpptResult;

    use super::*;

    /// A RAII-style handle to our mioco server so that we can spawn one and shut it down for each
    /// test, using separate ports.
    struct TestServerHandle {
        num_threads: usize,
        address: SocketAddr,
        queue: mpsc::Sender<()>,
        server: Option<JoinHandle<HpptResult<()>>>,
    }

    // TODO randomly pick server ports and try them until one binds

    impl TestServerHandle {
        pub fn new() -> Self {

            // set to true to get more verbose debug logging
            init_logging(false);

            let num_test_threads = 2;

            let mut listener = None;
            let mut address = None;

            for port in 8080..10_000 {
                let test_server_address = format!("127.0.0.1:{}", port);

                let addr = SocketAddr::from_str(&test_server_address).unwrap();

                match TcpListener::bind(&addr) {
                    Ok(l) => {
                        listener = Some(l);
                        address = Some(addr);
                        break;
                    }
                    Err(_) => (),
                }
            }

            let listener = listener.unwrap();
            let address = address.unwrap();

            let (send, recv) = mpsc::channel();
            debug!("Initializing test server at {} with {} threads...",
                   &address,
                   num_test_threads);

            let server = spawn(move || {
                run(listener,
                    &Path::new(env!("CARGO_MANIFEST_DIR")),
                    recv,
                    num_test_threads)
            });

            debug!("Test server initialized.");

            // FIXME find a better way to check for server liveness before returning
            sleep(Duration::from_millis(1000));

            TestServerHandle {
                num_threads: num_test_threads,
                address: address,
                queue: send,
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
                let _ = self.queue.send(()); // kill a coroutine, several times over
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
        let _server = TestServerHandle::new();
    }

    #[test]
    fn not_found() {
        let server = TestServerHandle::new();

        let response = server.make_request(b"GET /DOES_NOT_EXIST HTTP/1.1\r\n");

        check_bytes_utf8(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n",
                         &response);
    }

    #[test]
    fn file_contents_text() {
        let server = TestServerHandle::new();

        let filename = "Cargo.toml";

        let response = server.make_request(&format!("GET /{} HTTP/1.1\r\n", &filename).as_bytes());

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
        let server = TestServerHandle::new();

        let filename = "test/foo.html";

        let response = server.make_request(&format!("GET /{} HTTP/1.1\r\n", &filename).as_bytes());

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
        let server = TestServerHandle::new();

        let filename = "test/1k.bin";

        let response = server.make_request(&format!("GET /{} HTTP/1.1\r\n", &filename).as_bytes());

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
        let server = TestServerHandle::new();

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

            let response =
                server.make_request(&format!("GET /{} HTTP/1.1\r\n", &filename).as_bytes());
            check_bytes_utf8(&expected, &response);
        }
    }

    #[test]
    #[should_panic]
    fn large_request() {
        let server = TestServerHandle::new();

        let mut request = Vec::new();
        request.extend_from_slice(b"GET /");
        request.extend_from_slice(&[b'a'; 1024]);
        request.extend_from_slice(b" HTTP/1.1\r\n");

        let _response = server.make_request(&request);
    }

    #[test]
    fn unimplemented() {
        let server = TestServerHandle::new();

        let unsupported_requests = ["PUT / HTTP/1.1\r\n",
                                    "OPTIONS / HTTP/1.1\r\n",
                                    "HEAD / HTTP/1.1\r\n",
                                    "POST / HTTP/1.1\r\n",
                                    "PUT / HTTP/1.1\r\n",
                                    "DELETE / HTTP/1.1\r\n",
                                    "TRACE / HTTP/1.1\r\n",
                                    "CONNECT / HTTP/1.1\r\n"];

        for request in unsupported_requests.iter() {
            let response = server.make_request(&request.as_bytes());
            check_bytes_utf8(b"HTTP/1.1 501 Not Implemented\r\nContent-Length: 0\r\n\r\n",
                             &response);
        }

    }

    #[test]
    fn wrong_http_version() {
        let server = TestServerHandle::new();

        let response = server.make_request(b"GET / HTTP/1.0\r\n");

        check_bytes_utf8(b"HTTP/1.1 505 HTTP Version not supported\r\nContent-Length: 0\r\n\r\n",
                         &response);
    }

    #[test]
    fn cgi_hello_world() {
        let server = TestServerHandle::new();

        let response = server.make_request(b"GET /cgi-bin/hello_world.py HTTP/1.1\r\n");

        check_bytes_utf8(b"HTTP/1.1 200 OK\r
Content-Type: text/plain\r
\r
Hello, World!
",
                         &response);
    }

    #[test]
    fn cgi_addition_success() {
        let server = TestServerHandle::new();

        let response = server.make_request(b"GET /cgi-bin/addition.py?num1=1&num2=10 HTTP/1.1\r\n");

        check_bytes_utf8(b"HTTP/1.1 200 OK\r
Content-Type:text/html\r
\r
<h1>Addition Results</h1>\r
<p>1 + 10 = 11</p>\r
",
                         &response);
    }

    #[test]
    fn cgi_addition_fail() {
        let server = TestServerHandle::new();

        let response =
            server.make_request(b"GET /cgi-bin/addition.py?num1=banana&num2=pie HTTP/1.1\r\n");

        check_bytes_utf8(b"HTTP/1.1 400 Bad Request\r
Content-Type:text/html\r
\r
<h1>Addition Results</h1>\r
<p>Sorry, we cannot turn your inputs into integers.</p>\r
",
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
