pub mod attachments;
pub mod buffer;
pub mod changes;
pub mod commands;
pub mod comparison;
pub mod document;
pub mod editing;
pub mod live;
pub mod markdown;
pub mod navigation;
pub mod prepared;
pub mod properties;
pub mod state;
pub mod vault;

pub use opalglass_search as search;

// Executables opt in so library users retain control of their global allocator.
pub use mimalloc::MiMalloc as Allocator;
