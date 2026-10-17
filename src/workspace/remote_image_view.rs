//! Remote image frames reserve dimensions and adopt intrinsic width-only ratios.
use super::*;
use opalglass::images::Dimensions;
use remote_images::{Cache, Status};

#[derive(IntoElement)]
pub(super) struct RemoteImage {
    pub(super) url: String,
    pub(super) dimensions: Option<Dimensions>,
    pub(super) cache: Entity<Cache>,
    pub(super) row: usize,
    pub(super) part: usize,
    pub(super) instance: u64,
}

impl RenderOnce for RemoteImage {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let snapshot = self
            .cache
            .update(cx, |cache, cx| cache.request(&self.url, window, cx));
        let selector = reading_context::scoped_selector(
            self.instance,
            &format!("remote-image-{}-{}", self.row, self.part),
        );
        let mut frame = div()
            .id(reading_context::scoped_id(
                self.instance,
                format!("remote-image-{}", self.row),
                self.part,
            ))
            .debug_selector(move || selector.clone())
            .relative()
            .min_w_0()
            .max_w_full()
            .flex_none()
            .overflow_hidden()
            .border_1()
            .border_color(rgb(LINE))
            .rounded_md();
        // Explicit dimensions stay stable throughout loading. Width-only images
        // start with a square reservation and adopt their intrinsic ratio once
        // decoded. List preserves the Reading pane's logical scroll anchor.
        // Unknown images keep a fixed 320px frame throughout their lifetime.
        if let Some(dimensions) = self.dimensions {
            frame = frame.w(px(dimensions.width as f32));
            frame.style().aspect_ratio = Some(
                dimensions
                    .height
                    .map(|height| dimensions.width as f32 / height as f32)
                    .or(snapshot.intrinsic_ratio)
                    .unwrap_or(1.),
            );
        } else {
            frame = frame.w_full().h(px(320.));
        }
        let body = match snapshot.status {
            Status::Ready => {
                let cache = self.cache.clone();
                let url = self.url.clone();
                img(move |_: &mut Window, cx: &mut App| cache.read(cx).image(&url).map(Ok))
                    .absolute()
                    .size_full()
                    .object_fit(if self.dimensions.and_then(|d| d.height).is_some() {
                        ObjectFit::Fill
                    } else {
                        ObjectFit::Contain
                    })
                    .into_any_element()
            }
            status => self.placeholder(status),
        };
        frame
            .child(body)
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                let _lease = &snapshot.lease;
                cx.open_url(&self.url);
            })
    }
}

impl RemoteImage {
    fn placeholder(&self, status: Status) -> AnyElement {
        let failed = matches!(status, Status::Failed(_));
        let message: Arc<str> = match status {
            Status::Failed(message) => message,
            _ => "Loading image…".into(),
        };
        let row = self.row;
        let part = self.part;
        let selector = reading_context::scoped_selector(
            self.instance,
            &format!(
                "remote-{}-{row}-{part}",
                if failed { "error" } else { "loading" }
            ),
        );
        let retry_selector =
            reading_context::scoped_selector(self.instance, &format!("retry-image-{row}-{part}"));
        div()
            .absolute()
            .size_full()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .items_center()
            .justify_center()
            .bg(rgb(0x202532))
            .debug_selector(move || selector.clone())
            .child(caption(message.to_string()))
            .when(failed, |el| {
                let cache = self.cache.clone();
                let url = self.url.clone();
                el.child(
                    button(
                        reading_context::scoped_id(
                            self.instance,
                            format!("retry-image-{row}"),
                            part,
                        ),
                        "Retry",
                    )
                    .debug_selector(move || retry_selector.clone())
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        cache.update(cx, |cache, cx| cache.retry(&url, window, cx));
                    }),
                )
            })
            .into_any_element()
    }
}
