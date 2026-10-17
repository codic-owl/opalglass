//! Fetch/decode an image through the app's bounded loader, without opening a UI.
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .ok_or("Usage: remote_image_probe HTTP_IMAGE_URL")?;
    let started = Instant::now();
    let image = futures::executor::block_on(opalglass::images::load(url))?;
    let size = image.size(0);
    println!(
        "{}",
        serde_json::json!({
            "width": size.width.0,
            "height": size.height.0,
            "decoded_bytes": image.as_bytes(0).map_or(0, <[u8]>::len),
            "frames": image.frame_count(),
            "seconds": started.elapsed().as_secs_f64(),
        })
    );
    Ok(())
}
