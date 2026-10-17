#![allow(dead_code)]

#[global_allocator]
static ALLOCATOR: opalglass::Allocator = opalglass::Allocator;

// Compile the actual application modules with GPUI's simulated platform.
// Tests remain descendants so internal state need not become a public API.
mod editor;
#[path = "../../src/theme.rs"]
mod theme;
mod workspace;
