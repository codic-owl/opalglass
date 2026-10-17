use super::Dimensions;
use crate::{markdown, prepared::Prepared};

#[test]
fn dimensions_accept_documented_labels_and_ignore_malformed_sizes() {
    for (label, width, height) in [
        ("300", 300, None),
        ("caption|300", 300, None),
        ("caption|300x200", 300, Some(200)),
        ("300x200", 300, Some(200)),
        ("日本語 | 00300x0200 ", 300, Some(200)),
    ] {
        assert_eq!(
            Dimensions::from_label(label),
            Some(Dimensions { width, height })
        );
    }
    for label in [
        "",
        "caption",
        "image300.png",
        "0",
        "-1",
        "+1",
        "300px",
        "30%",
        "300x0",
        "0x300",
        "300x",
        "x200",
        "300x200x100",
        "1.5",
        "300X200",
        "３００",
        "4294967296",
        "100x4294967296",
        "caption|invalid",
        "caption|300x 200",
    ] {
        assert_eq!(Dimensions::from_label(label), None, "{label:?}");
    }
}

#[test]
fn image_dimensions_preserve_targets_labels_and_unicode_caret_mapping() {
    for (syntax, label, width, height) in [
        ("![[Photo.png|160]]", "160", 160, None),
        ("![[Photo.png|160x90]]", "160x90", 160, Some(90)),
        ("![日本語|160](Photo.png)", "日本語|160", 160, None),
        ("![160x90](Photo.png)", "160x90", 160, Some(90)),
        (
            "![**日本語**|160x90](Photo.png)",
            "日本語|160x90",
            160,
            Some(90),
        ),
    ] {
        let source = format!("é {syntax} after");
        let (text, spans, mapping) = markdown::inline_mapped(&source);
        assert_eq!(text, format!("é {label} after"));
        let images: Vec<_> = spans.iter().filter(|span| span.style.image).collect();
        assert!(!images.is_empty(), "{syntax}");
        for span in images {
            assert_eq!(span.style.target.as_deref(), Some("Photo.png"));
            assert_eq!(
                span.style.image_dimensions,
                Some(Dimensions { width, height })
            );
        }
        assert_eq!(mapping.len(), text.len() + 1);
        assert!(mapping.windows(2).all(|pair| pair[0] <= pair[1]));
        for (at, _) in text.char_indices() {
            assert!(source.is_char_boundary(mapping[at]), "{syntax}, {at}");
        }
        let at = text.find("after").unwrap();
        assert_eq!(mapping[at], source.find("after").unwrap());
    }
}

#[test]
fn image_dimensions_work_with_reference_images_tables_and_remote_targets() {
    let source = "![caption|123x45][photo]\n\n[photo]: https://example.com/Photo.png?q=1#part\n\n| Photo |\n| --- |\n| ![[Photo.png\\|80]] |\n";
    let prepared = Prepared::new(source);
    let ordinary = prepared
        .rows
        .iter()
        .flat_map(|row| &row.spans)
        .find(|span| span.style.image)
        .unwrap();
    assert_eq!(
        ordinary.style.image_dimensions,
        Some(Dimensions {
            width: 123,
            height: Some(45)
        })
    );
    assert_eq!(
        ordinary.style.target.as_deref(),
        Some("https://example.com/Photo.png?q=1#part")
    );
    let table = prepared
        .rows
        .iter()
        .filter_map(|row| row.table.as_ref())
        .flat_map(|table| &table.row.cells)
        .flat_map(|cell| &cell.spans)
        .find(|span| span.style.image)
        .unwrap();
    assert_eq!(
        table.style.image_dimensions,
        Some(Dimensions {
            width: 80,
            height: None
        })
    );
    assert_eq!(table.style.target.as_deref(), Some("Photo.png"));
    let (_, spans) = markdown::inline("[160x90](Photo.png) ![caption|no](Photo.png)");
    assert!(
        spans
            .iter()
            .all(|span| span.style.image_dimensions.is_none())
    );
}

#[test]
fn adjacent_images_keep_independent_sizes_and_source_identity() {
    let (_, spans) = markdown::inline("![160](Photo.png)![80x40](Photo.png)![caption](Photo.png)");
    let images: Vec<_> = spans.iter().filter(|span| span.style.image).collect();
    assert_eq!(images.len(), 3);
    assert_eq!(
        images[0].style.image_dimensions,
        Some(Dimensions {
            width: 160,
            height: None
        })
    );
    assert_eq!(
        images[1].style.image_dimensions,
        Some(Dimensions {
            width: 80,
            height: Some(40)
        })
    );
    assert_eq!(images[2].style.image_dimensions, None);
    assert!(
        images
            .windows(2)
            .all(|pair| pair[0].style.image_start < pair[1].style.image_start)
    );
}
