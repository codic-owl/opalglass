use super::{MAX_SVG_BYTES, decode};

fn svg(body: &str, width: u32, height: u32) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}">{body}</svg>"#
    )
}

#[test]
fn svg_pixels_are_straight_bgra_with_alpha_and_intrinsic_size() {
    let source = svg(
        r##"<rect width="1" height="1" fill="#ff0000" fill-opacity="0.5"/>"##,
        2,
        1,
    );
    let decoded = decode(source.as_bytes()).unwrap();
    assert_eq!((decoded.width, decoded.height), (2, 1));
    assert_eq!(decoded.bgra, [0, 0, 255, 128, 0, 0, 0, 0]);
    let source = format!("\u{feff}<?xml version=\"1.0\"?>{}", svg("", 1, 1));
    assert_eq!(decode(source.as_bytes()).unwrap().bgra, [0; 4]);
}

#[test]
fn svg_does_not_resolve_local_remote_or_data_images() {
    for target in [
        "file:///tmp/opalglass-image.png",
        "/tmp/opalglass-image.png",
        "https://example.com/image.png",
        "data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='1'%20height='1'%3E%3Crect%20width='1'%20height='1'%20fill='red'/%3E%3C/svg%3E",
    ] {
        let source = svg(
            &format!(r#"<image href="{target}" width="1" height="1"/>"#),
            1,
            1,
        );
        assert_eq!(decode(source.as_bytes()).unwrap().bgra, [0; 4], "{target}");
    }
}

#[test]
fn svg_rejects_oversized_canvas_layers_input_and_dtd() {
    for (width, height) in [(8193, 1), (4096, 4096)] {
        assert!(
            decode(svg("", width, height).as_bytes())
                .unwrap_err()
                .contains("limit")
        );
    }
    let source = svg(
        r#"<g opacity="0.5"><rect width="100000" height="100000"/></g>"#,
        1,
        1,
    );
    assert!(
        decode(source.as_bytes())
            .unwrap_err()
            .contains("offscreen layers")
    );
    let source = svg(&" ".repeat(MAX_SVG_BYTES), 1, 1);
    assert!(decode(source.as_bytes()).unwrap_err().contains("512 KiB"));
    let source = r#"<!DOCTYPE svg [<!ENTITY x "text">]><svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">&x;</svg>"#;
    assert!(decode(source.as_bytes()).unwrap_err().contains("DTD"));
}

#[test]
fn svg_bounds_nodes_depth_expansion_and_filter_work() {
    let source = svg(&"<path/>".repeat(5000), 1, 1);
    assert!(decode(source.as_bytes()).is_err());
    let source = svg(&format!("{}{}", "<g>".repeat(65), "</g>".repeat(65)), 1, 1);
    assert!(decode(source.as_bytes()).unwrap_err().contains("nesting"));
    let source = svg(
        r##"<defs><g id="a"><use href="#a"/></g></defs><use href="#a"/>"##,
        1,
        1,
    );
    assert!(
        decode(source.as_bytes())
            .unwrap_err()
            .contains("reference-expansion")
    );
    let mut body = String::from(r#"<defs><path id="a0" d="M0 0h1v1z"/>"#);
    for index in 1..15 {
        let prior = index - 1;
        body.push_str(&format!(
            r##"<g id="a{index}"><use href="#a{prior}"/><use href="#a{prior}"/></g>"##
        ));
    }
    body.push_str(r##"</defs><use href="#a14"/>"##);
    assert!(
        decode(svg(&body, 1, 1).as_bytes())
            .unwrap_err()
            .contains("reference-expansion")
    );
    let source = svg(
        r#"<filter id="f"><feGaussianBlur stdDeviation="1"/></filter>"#,
        1,
        1,
    );
    assert!(decode(source.as_bytes()).unwrap_err().contains("filters"));
    let source = svg(
        r#"<rect width="1" height="1" style="filter:blur(1px)"/>"#,
        1,
        1,
    );
    assert!(decode(source.as_bytes()).unwrap_err().contains("filters"));
}

#[test]
fn svg_supports_internal_references_gradients_and_clipping() {
    let source = svg(
        r##"<defs><linearGradient id="g"><stop stop-color="red"/></linearGradient><clipPath id="c"><rect width="1" height="1"/></clipPath><rect id="r" width="2" height="1" fill="url(#g)"/></defs><use href="#r" clip-path="url(#c)"/>"##,
        2,
        1,
    );
    assert_eq!(
        decode(source.as_bytes()).unwrap().bgra,
        [0, 0, 255, 255, 0, 0, 0, 0]
    );
}

#[test]
fn svg_patterns_and_mask_chains_obey_surface_budget() {
    let source = svg(
        r##"<defs><pattern id="p" patternUnits="userSpaceOnUse" width="100000" height="100000"><rect width="1" height="1"/></pattern></defs><rect width="1" height="1" fill="url(#p)"/>"##,
        1,
        1,
    );
    assert!(
        decode(source.as_bytes())
            .unwrap_err()
            .contains("offscreen layers")
    );
    let source = svg(
        r##"<defs><pattern id="p" patternUnits="userSpaceOnUse" width="1" height="1"><rect width="1" height="1" fill="red"/></pattern></defs><rect width="2" height="1" fill="url(#p)"/>"##,
        2,
        1,
    );
    assert_eq!(
        decode(source.as_bytes()).unwrap().bgra,
        [0, 0, 255, 255, 0, 0, 255, 255]
    );
    let mut body = String::from("<defs>");
    for index in 0..8 {
        let reference = if index == 0 {
            String::new()
        } else {
            format!(r##" mask="url(#m{})""##, index - 1)
        };
        body.push_str(&format!(r#"<mask id="m{index}"{reference}><rect width="2048" height="1024" fill="white"/></mask>"#));
    }
    body.push_str(r##"</defs><rect width="2048" height="1024" fill="red" mask="url(#m7)"/>"##);
    assert!(
        decode(svg(&body, 2048, 1024).as_bytes())
            .unwrap_err()
            .contains("offscreen layers")
    );
}
