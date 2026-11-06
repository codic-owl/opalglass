# GPUI provenance

Copied from https://github.com/zed-industries/zed, revision
3314de81755d7ef949007f6b804f4496092b0cc8, crates/gpui, under Apache-2.0
(see LICENSE-APACHE).

Local changes: normalize workspace-inherited Cargo settings; keep external Zed
crate dependencies at that same Git revision; add TextSystem::layout_session,
which creates an independently owned exact native layout cache. The native font
resolution, shaping and wrapping implementations are unchanged.
