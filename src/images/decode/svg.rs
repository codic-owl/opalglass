//! Restricted SVG preparation: no DTD, nested images, files or network access.
//! Bound the XML and reference expansion before usvg builds its rendering tree,
//! then bound offscreen layer work before resvg allocates raster surfaces.

use super::{Decoded, output_len};
use resvg::{tiny_skia, usvg};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

const MAX_SVG_BYTES: usize = 512 * 1024;
const MAX_NODES: u32 = 4096;
const MAX_DEPTH: usize = 64;
const MAX_RENDER_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn looks_like_xml(bytes: &[u8]) -> bool {
    bytes
        .strip_prefix(&[0xef, 0xbb, 0xbf])
        .unwrap_or(bytes)
        .trim_ascii_start()
        .starts_with(b"<")
}

pub(super) fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    if bytes.len() > MAX_SVG_BYTES {
        return Err("SVG exceeds the 512 KiB size limit".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "SVG must use UTF-8 text")?;
    let doc = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_NODES,
        },
    )
    .map_err(|error| format!("SVG could not be parsed: {error}"))?;
    if doc.root_element().tag_name().name() != "svg" {
        return Err("Image is not an SVG document".into());
    }
    validate_structure(&doc)?;
    let mut options = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    if doc.descendants().any(|node| node.has_tag_name("text")) {
        static FONTS: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
            let mut fonts = usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            Arc::new(fonts)
        });
        options.fontdb = FONTS.clone();
    }
    let tree = usvg::Tree::from_xmltree(&doc, &options)
        .map_err(|error| format!("SVG could not be prepared: {error}"))?;
    let width = tree.size().width().ceil() as u32;
    let height = tree.size().height().ceil() as u32;
    let mut render_bytes = output_len(width, height)? as u64;
    validate_layers(
        tree.root(),
        &mut render_bytes,
        0,
        tiny_skia::Transform::identity(),
    )?;
    let mut pixmap =
        tiny_skia::Pixmap::new(width, height).ok_or("SVG pixel buffer could not be allocated")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    // tiny-skia stores premultiplied RGBA. GPUI's image upload expects straight
    // BGRA, so undo premultiplication before swapping channels in-place.
    let mut bgra = pixmap.take();
    for pixel in bgra.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha != 0 && alpha != 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
        pixel.swap(0, 2);
    }
    Ok(Decoded {
        width,
        height,
        bgra,
    })
}

fn complexity() -> String {
    "SVG exceeds the node, nesting or reference-expansion limit".into()
}

fn validate_structure(doc: &roxmltree::Document<'_>) -> Result<(), String> {
    let nodes: Vec<_> = doc.descendants().collect();
    let mut ids = HashMap::new();
    let mut text_bytes = 0;
    for node in &nodes {
        if node.ancestors().take(MAX_DEPTH + 1).count() > MAX_DEPTH {
            return Err(complexity());
        }
        if let Some(id) = node.attribute("id")
            && ids.insert(id, node.id().get_usize()).is_some()
        {
            return Err("SVG contains duplicate element IDs".into());
        }
        // Filter primitives allocate multiple intermediate surfaces and can
        // refer recursively through feImage. Keep this boundary explicit.
        if node.has_tag_name("filter") {
            return Err("SVG filters are not supported by the bounded image renderer".into());
        }
        if node.is_text() && node.ancestors().any(|parent| parent.has_tag_name("text")) {
            text_bytes += node.text().map_or(0, str::len);
            if text_bytes > 32 * 1024 {
                return Err("SVG text exceeds the 32 KiB layout limit".into());
            }
        }
    }
    let mut edges = vec![Vec::new(); nodes.len()];
    for node in &nodes {
        let children = &mut edges[node.id().get_usize()];
        children.extend(node.children().map(|child| child.id().get_usize()));
        for attribute in node.attributes() {
            if attribute.name() == "href"
                && matches!(
                    node.tag_name().name(),
                    "use" | "linearGradient" | "radialGradient" | "pattern"
                )
                && let Some(target) = attribute
                    .value()
                    .strip_prefix('#')
                    .and_then(|id| ids.get(id))
            {
                children.push(*target);
            }
            add_url_references(attribute.value(), &ids, children);
        }
        if node.has_tag_name("style") {
            for text in node.children().filter_map(|child| child.text()) {
                add_url_references(text, &ids, children);
            }
        }
    }
    expanded_cost(
        0,
        &edges,
        &mut vec![None; nodes.len()],
        &mut vec![false; nodes.len()],
        0,
    )?;
    Ok(())
}

fn add_url_references(value: &str, ids: &HashMap<&str, usize>, edges: &mut Vec<usize>) {
    let lower = value.to_ascii_lowercase();
    for (start, _) in lower.match_indices("url(") {
        let tail = &value[start + 4..];
        let Some((argument, _)) = tail.split_once(')') else {
            continue;
        };
        let id = argument.trim().trim_matches(['\'', '"']).strip_prefix('#');
        if let Some(target) = id.and_then(|id| ids.get(id)) {
            edges.push(*target);
        }
    }
}

fn expanded_cost(
    node: usize,
    edges: &[Vec<usize>],
    memo: &mut [Option<(u32, usize)>],
    visiting: &mut [bool],
    nesting: usize,
) -> Result<(u32, usize), String> {
    if nesting > MAX_DEPTH || visiting[node] {
        return Err(complexity());
    }
    if let Some(cost) = memo[node] {
        return Ok(cost);
    }
    visiting[node] = true;
    let (mut cost, mut depth) = (1u32, 1usize);
    for &child in &edges[node] {
        let (child_cost, child_depth) = expanded_cost(child, edges, memo, visiting, nesting + 1)?;
        cost = cost.saturating_add(child_cost);
        depth = depth.max(child_depth + 1);
        if cost > MAX_NODES || depth > MAX_DEPTH {
            return Err(complexity());
        }
    }
    visiting[node] = false;
    memo[node] = Some((cost, depth));
    Ok((cost, depth))
}

fn validate_layers(
    group: &usvg::Group,
    bytes: &mut u64,
    depth: usize,
    transform: tiny_skia::Transform,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(complexity());
    }
    if !group.filters().is_empty() {
        return Err("SVG filters are not supported by the bounded image renderer".into());
    }
    let transform = transform.pre_concat(group.transform());
    if group.should_isolate()
        && let Some(bbox) = group.layer_bounding_box().transform(transform)
    {
        let width = (bbox.width().ceil() as u64).saturating_add(4);
        let height = (bbox.height().ceil() as u64).saturating_add(4);
        reserve_surface(bytes, width, height)?;
        validate_clip_masks(group, bytes, depth, transform, width, height)?;
    }
    for node in group.children() {
        match node {
            usvg::Node::Group(child) => validate_layers(child, bytes, depth + 1, transform)?,
            usvg::Node::Text(text) => {
                validate_layers(text.flattened(), bytes, depth + 1, transform)?
            }
            usvg::Node::Path(path) => {
                for paint in [
                    path.fill().map(|fill| fill.paint()),
                    path.stroke().map(|stroke| stroke.paint()),
                ]
                .into_iter()
                .flatten()
                {
                    if let usvg::Paint::Pattern(pattern) = paint {
                        validate_pattern(pattern, bytes, depth, transform)?;
                    }
                }
            }
            usvg::Node::Image(_) => return Err("Nested SVG images are not supported".into()),
        }
    }
    Ok(())
}

fn reserve_surface(bytes: &mut u64, width: u64, height: u64) -> Result<(), String> {
    *bytes = bytes.saturating_add(width.saturating_mul(height).saturating_mul(4));
    if *bytes > MAX_RENDER_BYTES {
        return Err("SVG offscreen layers exceed the 64 MiB rendering limit".into());
    }
    Ok(())
}

fn validate_clip_masks(
    group: &usvg::Group,
    bytes: &mut u64,
    depth: usize,
    transform: tiny_skia::Transform,
    width: u64,
    height: u64,
) -> Result<(), String> {
    let mut clip = group.clip_path();
    while let Some(current) = clip {
        // A clip renders a full layer plus an alpha mask. Count the alpha
        // mask as another RGBA layer, conservatively, before rendering.
        reserve_surface(bytes, width, height)?;
        reserve_surface(bytes, width, height)?;
        validate_layers(
            current.root(),
            bytes,
            depth + 1,
            transform.pre_concat(current.transform()),
        )?;
        clip = current.clip_path();
    }
    let mut mask = group.mask();
    while let Some(current) = mask {
        reserve_surface(bytes, width, height)?;
        reserve_surface(bytes, width, height)?;
        validate_layers(current.root(), bytes, depth + 1, transform)?;
        mask = current.mask();
    }
    Ok(())
}

fn validate_pattern(
    pattern: &usvg::Pattern,
    bytes: &mut u64,
    depth: usize,
    transform: tiny_skia::Transform,
) -> Result<(), String> {
    let (sx, sy) = transform.pre_concat(pattern.transform()).get_scale();
    let width = (pattern.rect().width() * sx).round() as u64;
    let height = (pattern.rect().height() * sy).round() as u64;
    reserve_surface(bytes, width, height)?;
    validate_layers(
        pattern.root(),
        bytes,
        depth + 1,
        tiny_skia::Transform::from_scale(sx, sy),
    )
}

#[cfg(test)]
#[path = "svg_tests.rs"]
mod tests;
