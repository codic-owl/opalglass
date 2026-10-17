//! Shared image metadata and bounded background loading.

pub mod decode;
pub mod http;

/// Requested pixel dimensions from an Obsidian image label.
/// A missing height preserves the image's intrinsic aspect ratio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dimensions {
    pub width: u32,
    pub height: Option<u32>,
}

impl Dimensions {
    /// Accept `![[image|300]]`, `![caption|300x200](image)` and
    /// `![300](image)`. An ordinary or malformed label is left unchanged.
    pub fn from_label(label: &str) -> Option<Self> {
        let size = label
            .rsplit_once('|')
            .map_or(label, |(_, size)| size)
            .trim();
        let (width, height) = size
            .split_once('x')
            .map_or((size, None), |(width, height)| (width, Some(height)));
        Some(Self {
            width: positive_pixels(width)?,
            height: match height {
                Some(height) => Some(positive_pixels(height)?),
                None => None,
            },
        })
    }
}

fn positive_pixels(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok().filter(|value| *value > 0)
}

mod loading;
pub use loading::{load, prepare};
