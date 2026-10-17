//! Visible embed rows acquire leases; the composition itself retains no bodies.
use super::*;
use embedded_documents::{Input, Status};
use reading_composition::Instance;
use reading_projection::Row;

#[derive(IntoElement)]
struct LeasedRow {
    owner: WeakEntity<Workspace>,
    cache: Entity<embedded_documents::Cache>,
    editor: EntityId,
    instance: Instance,
    slot: usize,
    input: Option<Input>,
    child: Option<AnyElement>,
    header: Option<AnyElement>,
}

impl RenderOnce for LeasedRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            owner,
            cache,
            editor,
            instance,
            slot,
            input,
            child,
            header,
        } = self;
        let lease = owner
            .update(cx, |workspace, cx| {
                if workspace.loading {
                    return None;
                }
                if let Some(input) = &input {
                    let current = workspace.embed_input(&input.path, cx)?;
                    if current.token != input.token {
                        return None;
                    }
                }
                workspace.demand_reading_instance(
                    editor,
                    instance.id,
                    input.as_ref().map(|input| input.token),
                    cx,
                )
            })
            .ok()
            .flatten();
        let Some((visible, inputs)) = lease else {
            return div().into_any_element();
        };
        // Pin ancestor documents before requesting the child: an offscreen
        // parent must not disappear while its visible descendant is retained.
        let mut snapshots = inputs
            .into_iter()
            .map(|input| cache.update(cx, |cache, cx| cache.request(input, window, cx)))
            .collect::<Vec<_>>();
        let snapshot = input.as_ref().and_then(|_| snapshots.pop());
        let content = child.unwrap_or_else(|| {
            let Some(snapshot) = &snapshot else {
                return div()
                    .child(caption("Embedded note is unavailable"))
                    .into_any_element();
            };
            let label = match &snapshot.status {
                Status::Waiting => "Loading note…".to_owned(),
                Status::Ready => "Opening embedded note…".to_owned(),
                Status::Failed(message) => message.to_string(),
            };
            let mut placeholder = div()
                .py_3()
                .flex()
                .items_center()
                .gap_3()
                .child(caption(label));
            if let Some(header) = header {
                placeholder = placeholder.child(header);
            }
            if let Some(input) = input.filter(|_| matches!(snapshot.status, Status::Failed(_))) {
                placeholder = placeholder.child(
                    button(
                        (
                            SharedString::from(format!("retry-embed-{}", instance.id)),
                            0,
                        ),
                        "Retry",
                    )
                    .debug_selector({
                        let id = instance.id;
                        move || format!("retry-embed-{id}")
                    })
                    .on_click(move |_, window, cx| {
                        let current = owner
                            .update(cx, |workspace, cx| {
                                workspace.tabs.get(workspace.active).is_some_and(|tab| {
                                    tab.editor.entity_id() == editor
                                        && tab.composition.token(instance.id) == Some(input.token)
                                }) && workspace
                                    .embed_input(&input.path, cx)
                                    .is_some_and(|current| current.token == input.token)
                            })
                            .unwrap_or(false);
                        if current {
                            cache.update(cx, |cache, cx| {
                                cache.retry(&input.path, input.token, window, cx)
                            });
                        }
                    }),
                );
            }
            placeholder.into_any_element()
        });
        let retained_id = (
            SharedString::from(format!("visible-embed-leases-{}", instance.id)),
            slot,
        )
            .into();
        let retained = snapshot.map(|snapshot| (snapshot.lease, snapshot.document));
        div()
            .relative()
            .child(content)
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        visible_leases::retain(window, retained_id, (visible, snapshots, retained));
                    },
                )
                .absolute()
                .size_full(),
            )
            .into_any_element()
    }
}

impl Workspace {
    pub(super) fn composed_preview_row(
        &mut self,
        list_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(tab) = self.tabs.get(self.active) else {
            return div().into_any_element();
        };
        let Some(row) = tab.composition.projection.row(list_index) else {
            return div().into_any_element();
        };
        let instance = row.instance();
        let editor = tab.editor.entity_id();
        let metadata = tab.composition.instances.get(&instance).cloned();
        let context = self.reading_context_for(instance, cx);
        let index = match &row {
            Row::Document { index, .. } => context.as_ref().map_or(0, |c| c.projection.row(*index)),
            Row::Fragment { index, .. } => *index,
            _ => 0,
        };
        let child = match (row, context.as_ref()) {
            (Row::Document { .. }, Some(context)) => context
                .prepared
                .rows
                .get(index)
                .map(|row| self.preview_row(context, index, row, cx)),
            (Row::Fragment { range, .. }, Some(context)) => context
                .prepared
                .rows
                .get(index)
                .map(|row| self.preview_row(context, index, &clipped(row, range), cx)),
            (Row::Header { .. }, _) => metadata
                .as_ref()
                .map(|metadata| self.embed_header(metadata, cx)),
            (Row::Footer { .. }, _) => Some(div().h(px(10.)).into_any_element()),
            _ => None,
        };
        let content = if let Some(metadata) = metadata {
            let child = if let Some(message) = &metadata.error {
                Some(
                    div()
                        .py_3()
                        .child(caption(message.to_string()))
                        .child(self.embed_header(&metadata, cx))
                        .into_any_element(),
                )
            } else {
                child
            };
            let input = metadata
                .input
                .as_ref()
                .and_then(|input| self.embed_input(&input.path, cx));
            let content = LeasedRow {
                owner: cx.entity().downgrade(),
                cache: self.embedded_documents.clone(),
                editor,
                header: child.is_none().then(|| self.embed_header(&metadata, cx)),
                instance: metadata.clone(),
                slot: list_index,
                input,
                child,
            }
            .into_any_element();
            div()
                .ml(px(metadata.depth.min(8) as f32 * 12.))
                .pl_3()
                .pr_3()
                .border_l_1()
                .border_color(rgb(0x424a60))
                .bg(rgb(0x1b202b))
                .child(content)
                .into_any_element()
        } else {
            child.unwrap_or_else(|| div().into_any_element())
        };
        div()
            .w_full()
            .px_6()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .min_w_0()
                    .debug_selector(move || {
                        reading_context::scoped_selector(
                            instance,
                            &format!("preview-column-{index}"),
                        )
                    })
                    .child(content),
            )
            .into_any_element()
    }

    fn embed_header(&self, metadata: &Instance, cx: &mut Context<Self>) -> AnyElement {
        let (origin, target, id) = (metadata.origin.clone(), metadata.label.clone(), metadata.id);
        div()
            .pt_2()
            .pb_1()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .debug_selector(move || format!("embed-header-{id}"))
            .child(caption(metadata.label.to_string()))
            .child(
                button(
                    (SharedString::from(format!("open-embed-{id}")), 0),
                    "Open note",
                )
                .debug_selector(move || format!("open-embed-{id}"))
                .on_click(cx.listener(move |workspace, _, window, cx| {
                    workspace.follow_from(&origin, target.to_string(), window, cx);
                })),
            )
            .into_any_element()
    }
}

fn clipped(row: &PreviewRow, range: std::ops::Range<usize>) -> PreviewRow {
    let mut result = row.clone();
    result.text = row.text[range.clone()].into();
    result.spans = row
        .spans
        .iter()
        .filter(|span| span.range.start < range.end && span.range.end > range.start)
        .map(|span| markdown::Span {
            range: span.range.start.max(range.start) - range.start
                ..span.range.end.min(range.end) - range.start,
            style: span.style.clone(),
        })
        .collect();
    if range.start > 0 {
        result.continuation = true;
        result.task = None;
        if matches!(result.kind, Kind::CalloutTitle) {
            result.kind = Kind::Paragraph;
        }
    }
    result
}
