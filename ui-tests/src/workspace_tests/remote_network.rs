use super::{TestAppContext, ViewMode, Workspace};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

#[gpui::test]
fn remote_image_downloads_decodes_and_reaches_reading_through_the_real_loader(
    cx: &mut TestAppContext,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/fixture.svg", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            assert_eq!(socket.read(&mut byte).unwrap(), 1);
            request.push(byte[0]);
            assert!(request.len() < 16384);
        }
        assert!(request.starts_with(b"GET /fixture.svg "));
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2\" height=\"1\"><rect width=\"2\" height=\"1\" fill=\"#123456\"/></svg>";
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: image/svg+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", svg.len()).unwrap();
        socket.write_all(svg).unwrap();
    });
    let folder = tempfile::tempdir().unwrap();
    let text = format!("![120x60]({url})\n");
    std::fs::write(folder.path().join("A.md"), &text).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    view.update_in(cx, |this, w, cx| this.set_mode(ViewMode::Preview, w, cx));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        let ready = view.read_with(cx, |this, cx| this.remote_images.read(cx).image(&url));
        if let Some(image) = ready {
            assert_eq!(image.size(0).width.0, 2);
            assert_eq!(image.size(0).height.0, 1);
            assert_eq!(
                image.as_bytes(0).unwrap(),
                &[0x56, 0x34, 0x12, 255, 0x56, 0x34, 0x12, 255]
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "remote image did not reach the UI"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    cx.update(|w, _| w.refresh());
    cx.run_until_parked();
    let image = cx.debug_bounds("remote-image-0-0").unwrap();
    assert_eq!(image.size, gpui::size(gpui::px(120.), gpui::px(60.)));
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), text)
    });
    server.join().unwrap();
}
