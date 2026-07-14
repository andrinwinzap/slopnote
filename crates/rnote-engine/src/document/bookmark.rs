// Imports
use p2d::math::Vector2;
use serde::{Deserialize, Serialize};

/// A bookmarked location in the document.
///
/// Stores the viewport center position (in document coordinate space) and the camera zoom,
/// so jumping to a bookmark restores the exact view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename = "bookmark")]
pub struct Bookmark {
    /// The bookmarked viewport center in document coordinate space.
    #[serde(rename = "pos")]
    pub pos: Vector2,
    /// The camera zoom of the bookmarked view.
    #[serde(rename = "zoom")]
    pub zoom: f64,
    /// An optional user-given name. Empty when not named.
    #[serde(rename = "name")]
    pub name: String,
}

impl Default for Bookmark {
    fn default() -> Self {
        Self {
            pos: Vector2::ZERO,
            zoom: 1.0,
            name: String::new(),
        }
    }
}

impl Bookmark {
    /// Key for ordering bookmarks by their position in the document (rows top to bottom, then left to right).
    pub(crate) fn order_key(&self) -> (f64, f64) {
        (self.pos[1], self.pos[0])
    }
}
