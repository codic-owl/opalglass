//! Network and CPU work share two slots, including decoding work that cannot be
//! interrupted midway when its tab disappears. Poll `load` off the UI thread.
use super::{decode, http};
use std::sync::{Arc, LazyLock};

static SLOTS: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(2)));

pub async fn load(url: String) -> Result<Arc<gpui::RenderImage>, String> {
    let _permit = SLOTS
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| e.to_string())?;
    let bytes = http::fetch(url).await?;
    prepare(&bytes)
}

pub fn prepare(bytes: &[u8]) -> Result<Arc<gpui::RenderImage>, String> {
    let decoded = decode::decode(bytes)?;
    let buffer = image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.bgra)
        .ok_or("Invalid decoded image size")?;
    Ok(Arc::new(gpui::RenderImage::new(vec![image::Frame::new(
        buffer,
    )])))
}
