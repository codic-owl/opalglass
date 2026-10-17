pub mod attachments;
pub mod buffer;
pub mod callouts;
pub mod changes;
pub mod commands;
pub mod comparison;
pub mod document;
pub mod editing;
pub mod footnotes;
pub mod history;
pub mod images;
mod inline;
mod inline_notes;
pub mod links;
pub mod live;
pub mod markdown;
pub mod navigation;
mod platform;
pub mod prepared;
pub mod properties;
pub mod rendering;
pub mod state;
pub mod syntax;
pub mod vault;

pub use opalglass_search as search;

// Executables opt in so library users retain control of their global allocator.
pub use mimalloc::MiMalloc as Allocator;
