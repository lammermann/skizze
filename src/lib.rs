//! Buffer and diagram-drawing primitives for skizze.
//!
//! This library deliberately has no terminal, file-system, or clipboard integration.

mod buffer;
mod document;

pub use buffer::{Buffer, Point};
pub use document::Document;
