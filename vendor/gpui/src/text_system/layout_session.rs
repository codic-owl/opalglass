use super::{TextSystem, WindowTextSystem};
use std::sync::Arc;

impl TextSystem {
    /// Create an independent exact native text-layout session.
    ///
    /// Its cache is separate from every window and is released with the session.
    /// Background preparation can drop bounded sessions without affecting a
    /// window's frame lifecycle, glyph shaping, font fallback or wrap geometry.
    pub fn layout_session(self: &Arc<Self>) -> WindowTextSystem {
        WindowTextSystem::new(self.clone())
    }
}
