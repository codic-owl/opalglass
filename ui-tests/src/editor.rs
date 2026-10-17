include!("../../src/editor.rs");

#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "editor_context_tests.rs"]
mod context_tests;

#[cfg(test)]
#[path = "editor_callout_tests.rs"]
mod callout_tests;

#[cfg(test)]
#[path = "editor_visibility_tests.rs"]
mod visibility_tests;

#[cfg(test)]
#[path = "editor_block_markers_tests.rs"]
mod block_markers_tests;
