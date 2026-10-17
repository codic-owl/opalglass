use super::*;
use futures::{channel::oneshot, executor::block_on, future::Either};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
};

fn request(socket: &mut TcpStream) -> String {
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0];
        if socket.read(&mut byte).unwrap() == 0 {
            break;
        }
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
        assert!(bytes.len() < 16 * 1024, "unexpected request size");
    }
    String::from_utf8(bytes).unwrap()
}

fn fixture(serve: impl FnOnce(TcpListener) + Send + 'static) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/image", listener.local_addr().unwrap());
    (url, thread::spawn(move || serve(listener)))
}

fn response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

#[test]
fn fetches_bytes_and_follows_redirects() {
    let (url, server) = fixture(|listener| {
        let (mut first, _) = listener.accept().unwrap();
        assert!(request(&mut first).starts_with("GET /image "));
        first
            .write_all(
                b"HTTP/1.1 302 Found\r\nLocation: /actual\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
        let (mut second, _) = listener.accept().unwrap();
        assert!(request(&mut second).starts_with("GET /actual "));
        second
            .write_all(&response("200 OK", b"image bytes"))
            .unwrap();
    });
    assert_eq!(block_on(fetch(url)).unwrap(), b"image bytes");
    server.join().unwrap();
}

#[test]
fn rejects_status_before_reading_the_response_body() {
    let (url, server) = fixture(|listener| {
        let (mut socket, _) = listener.accept().unwrap();
        request(&mut socket);
        socket
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 999999999\r\n\r\n")
            .unwrap();
        let mut byte = [0];
        assert_eq!(socket.read(&mut byte).unwrap(), 0);
    });
    assert_eq!(
        block_on(fetch(url)).unwrap_err(),
        "Image server returned HTTP 404"
    );
    server.join().unwrap();
}

#[test]
fn rejects_declared_oversize_without_waiting_for_a_body() {
    let (url, server) = fixture(|listener| {
        let (mut socket, _) = listener.accept().unwrap();
        request(&mut socket);
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            MAX_ENCODED_BYTES + 1
        )
        .unwrap();
        let mut byte = [0];
        assert_eq!(socket.read(&mut byte).unwrap(), 0);
    });
    assert!(block_on(fetch(url)).unwrap_err().contains("download limit"));
    server.join().unwrap();
}

#[test]
fn rejects_actual_oversize_without_a_content_length() {
    let (url, server) = fixture(|listener| {
        let (mut socket, _) = listener.accept().unwrap();
        request(&mut socket);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
            .unwrap();
        let block = [0_u8; 64 * 1024];
        for _ in 0..MAX_ENCODED_BYTES / block.len() {
            if socket.write_all(&block).is_err() {
                return;
            }
        }
        let _ = socket.write_all(&[1]);
    });
    assert!(block_on(fetch(url)).unwrap_err().contains("download limit"));
    server.join().unwrap();
}

#[test]
fn refuses_redirect_loops() {
    let (url, server) = fixture(|listener| {
        for _ in 0..5 {
            let (mut socket, _) = listener.accept().unwrap();
            request(&mut socket);
            socket
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: /image\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        }
    });
    assert_eq!(
        block_on(fetch(url)).unwrap_err(),
        "Image server redirected too many times"
    );
    server.join().unwrap();
}

#[test]
fn dropping_fetch_cancels_the_request_before_headers_arrive() {
    let (accepted_tx, accepted_rx) = oneshot::channel();
    let (closed_tx, closed_rx) = mpsc::channel();
    let (url, server) = fixture(move |listener| {
        let (mut socket, _) = listener.accept().unwrap();
        request(&mut socket);
        accepted_tx.send(()).unwrap();
        let mut byte = [0];
        closed_tx.send(socket.read(&mut byte).unwrap()).unwrap();
    });
    block_on(async {
        match futures::future::select(Box::pin(fetch(url)), accepted_rx).await {
            Either::Right((Ok(()), pending)) => drop(pending),
            _ => panic!("request completed before the server sent headers"),
        }
    });
    assert_eq!(closed_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 0);
    server.join().unwrap();
}

#[test]
fn refuses_non_http_schemes_and_malformed_urls() {
    for url in [
        "file:///tmp/image.png",
        "ftp://example.com/image",
        "bad url",
        "data:image/png,a",
    ] {
        assert_eq!(
            block_on(fetch(url.into())).unwrap_err(),
            "Invalid remote image URL"
        );
    }
}

#[test]
fn normalized_urls_share_fragments_and_bound_parse_work() {
    assert_eq!(
        normalized_url("HTTPS://EXAMPLE.COM/photo.png?q=1#section"),
        Some("https://example.com/photo.png?q=1".into())
    );
    assert_eq!(
        normalized_url(&format!("https://example.com/{}", "a".repeat(8192))),
        None
    );
    assert_eq!(normalized_url("file:///image.png"), None);
}
