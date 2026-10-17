//! Attachment presentation and constrained image layout.
use super::*;
use opalglass::images::Dimensions;

impl Workspace {
    pub(super) fn preview_images(
        &self,
        context: &reading_context::ReadingContext,
        index: usize,
        row: &PreviewRow,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !row.spans.iter().any(|span| span.style.image) {
            return None;
        }
        let vault = self.vault.as_ref()?;
        let mut content = div().my_3().flex().flex_col().gap_2();
        let mut start = 0;
        let mut spans = row.spans.iter().enumerate().peekable();
        while let Some((part, span)) = spans.next() {
            if !span.style.image {
                continue;
            }
            let Some(target) = span.style.target.as_deref() else {
                continue;
            };
            // Formatted alt text can contain several style spans. They still
            // describe a single image and must not produce duplicate widgets.
            let mut end = span.range.end;
            while let Some((_, next)) = spans.peek()
                && next.range.start == end
                && next.style.image
                && next.style.image_start == span.style.image_start
                && next.style.target.as_deref() == Some(target)
                && next.style.image_dimensions == span.style.image_dimensions
            {
                end = next.range.end;
                spans.next();
            }
            let remote = opalglass::images::http::normalized_url(target);
            let path = if remote.is_some() {
                None
            } else {
                opalglass::attachments::resolve(&vault.root, &context.path, target)
                    .ok()
                    .or_else(|| {
                        vault
                            .resolve_attachment(&context.path, target)
                            .and_then(|p| vault.file_path(&p).ok())
                    })
                    .filter(|p| opalglass::attachments::is_image(p))
            };
            if path.is_none() && remote.is_none() {
                continue;
            }
            if start < span.range.start {
                content = content.child(self.preview_fragment(
                    context,
                    row,
                    start..span.range.start,
                    index,
                    part * 2,
                    cx,
                ));
            }
            content = if let Some(url) = remote {
                content.child(remote_image_view::RemoteImage {
                    url,
                    dimensions: span.style.image_dimensions,
                    cache: self.remote_images.clone(),
                    row: index,
                    part,
                    instance: context.instance,
                })
            } else {
                content.child(LocalImage {
                    path: path.unwrap(),
                    dimensions: span.style.image_dimensions,
                    row: index,
                    part,
                    instance: context.instance,
                })
            };
            start = end;
        }
        if start == 0 {
            return None;
        }
        if start < row.text.len() {
            content = content.child(self.preview_fragment(
                context,
                row,
                start..row.text.len(),
                index,
                usize::MAX,
                cx,
            ));
        }
        Some(content.into_any_element())
    }
}

#[derive(IntoElement)]
struct LocalImage {
    path: PathBuf,
    dimensions: Option<Dimensions>,
    row: usize,
    part: usize,
    instance: u64,
}

impl RenderOnce for LocalImage {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            path,
            dimensions,
            row,
            part,
            instance,
        } = self;
        let selector =
            reading_context::scoped_selector(instance, &format!("attachment-image-{row}-{part}"));
        let mut frame = div()
            .id(reading_context::scoped_id(
                instance,
                format!("attachment-{row}"),
                part,
            ))
            .debug_selector(move || selector.clone())
            .relative()
            .min_w_0()
            .max_w_full()
            .flex_none()
            .cursor_pointer();
        if let Some(dimensions) = dimensions {
            // GPUI's Img fills in its auto height before max-width constraints.
            // A separate ratio-sized frame keeps both dimensions in proportion
            // when the requested image is wider than a Reading/Split pane.
            let image = window.use_asset::<ImgResourceLoader>(&path.clone().into(), cx);
            let ratio = dimensions
                .height
                .map(|height| dimensions.width as f32 / height as f32)
                .or_else(|| {
                    image
                        .as_ref()
                        .and_then(|image| image.as_ref().ok())
                        .map(|image| {
                            let size = image.size(0);
                            size.width.0 as f32 / size.height.0 as f32
                        })
                })
                .unwrap_or(1.);
            frame = frame.w(px(dimensions.width as f32));
            frame.style().aspect_ratio = Some(ratio);
            frame = frame.child(
                img(path.clone())
                    .absolute()
                    .size_full()
                    .object_fit(ObjectFit::Fill),
            );
        } else {
            frame = frame.w_full().child(
                img(path.clone())
                    .w_full()
                    .h(px(320.))
                    .object_fit(ObjectFit::Contain),
            );
        }
        frame.on_click(move |_, _, cx| cx.open_with_system(&path))
    }
}
