//! Retain resources for the lifetime of a painted element, including cached frames.
use gpui::{ElementId, Window};

/// Canvas paint callbacks are consumed after painting. Store their resources in
/// window element state instead: GPUI keeps this state while the element remains
/// visible and releases it when its row leaves the rendered frame.
pub(super) fn retain<T: 'static>(window: &mut Window, id: ElementId, resources: T) {
    window.with_global_id(id, |global_id, window| {
        window.with_element_state::<T, _>(global_id, |_previous, _| ((), resources));
    });
}
