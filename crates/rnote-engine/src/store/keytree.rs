// Imports
use super::StrokeKey;
use p2d::bounding_volume::Aabb;
use p2d::math::Vector2;
use rstar::primitives::GeomWithData;

/// The rtree object that holds the bounds and [StrokeKey].
type KeyTreeObject = GeomWithData<rstar::primitives::Rectangle<[f64; 2]>, StrokeKey>;

#[derive(Debug, Default)]
/// A Rtree with [StrokeKey]'s as associated data.
///
/// Used for faster spatial queries.
pub(super) struct KeyTree(rstar::RTree<KeyTreeObject, rstar::DefaultParams>);

impl KeyTree {
    /// Insert a new tree object with the given [StrokeKey] and bounds.
    pub(crate) fn insert_with_key(&mut self, key: StrokeKey, bounds: Aabb) {
        self.0.insert(new_keytree_object(key, bounds));
    }

    /// Removes the [KeyTreeObject] for the given key.
    pub(crate) fn remove_with_key(&mut self, key: StrokeKey) -> Option<KeyTreeObject> {
        let object_to_remove = self.0.iter().find(|&object| object.data == key)?.to_owned();

        self.0.remove(&object_to_remove)
    }

    /// Update the Tree with new bounds for the given key.
    ///
    /// Has to be called when the geometry of the stroke has changed.
    pub(crate) fn update_with_key(&mut self, key: StrokeKey, new_bounds: Aabb) {
        self.remove_with_key(key);
        self.insert_with_key(key, new_bounds);
    }

    /// Return the keys that intersect with the given bounds.
    pub(crate) fn keys_intersecting_bounds(&self, bounds: Aabb) -> Vec<StrokeKey> {
        self.0
            .locate_in_envelope_intersecting(&rstar::AABB::from_corners(
                [bounds.mins[0], bounds.mins[1]],
                [bounds.maxs[0], bounds.maxs[1]],
            ))
            .map(|object| object.data)
            .collect()
    }

    /// Return the keys that are completely contained in the given bounds.
    pub(crate) fn keys_in_bounds(&self, bounds: Aabb) -> Vec<StrokeKey> {
        self.0
            .locate_in_envelope(&rstar::AABB::from_corners(
                [bounds.mins[0], bounds.mins[1]],
                [bounds.maxs[0], bounds.maxs[1]],
            ))
            .map(|object| object.data)
            .collect()
    }

    /// Get containing bounds for the supplied keys.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (StrokeKey, Aabb)> {
        self.0.iter().map(|geom| {
            let bounds = geom.geom();
            let lower = bounds.lower();
            let upper = bounds.upper();
            (
                geom.data,
                Aabb::new(
                    Vector2::new(lower[0], lower[1]),
                    Vector2::new(upper[0], upper[1]),
                ),
            )
        })
    }

    /// Rebuild the entire rtree from the given Vec of (key, bounds).
    pub(crate) fn rebuild_from_vec(&mut self, strokes: Vec<(StrokeKey, Aabb)>) {
        let objects = strokes
            .into_iter()
            .map(|(key, bounds)| new_keytree_object(key, bounds))
            .collect();

        self.0 = rstar::RTree::bulk_load(objects);
    }

    ///  Clear the entire tree.
    pub(crate) fn clear(&mut self) {
        *self = Self::default()
    }
}

fn new_keytree_object(key: StrokeKey, bounds: Aabb) -> KeyTreeObject {
    KeyTreeObject::new(
        rstar::primitives::Rectangle::from_corners(
            [bounds.mins[0], bounds.mins[1]],
            [bounds.maxs[0], bounds.maxs[1]],
        ),
        key,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    /// Moving every stroke of a loaded document, as inserting space above all of it does.
    ///
    /// rstar 0.13.0 panics here ("This is a bug in rstar.") on documents of handwriting, which is
    /// why it is held at 0.12: a merge from upstream brings 0.13 back.
    #[test]
    fn moving_all_strokes_of_a_loaded_document() {
        let mut keys = SlotMap::<StrokeKey, ()>::with_key();
        // Deterministic pseudo-random small boxes, as the letters of a few pages of handwriting
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 10_000) as f64 / 10_000.0
        };
        let strokes = (0..1600)
            .map(|_| {
                let pos = Vector2::new(40.0 + next() * 700.0, next() * 4000.0);
                let size = Vector2::new(2.0 + next() * 14.0, 2.0 + next() * 14.0);
                (keys.insert(()), Aabb::new(pos, pos + size))
            })
            .collect::<Vec<_>>();

        let mut tree = KeyTree::default();
        tree.rebuild_from_vec(strokes.clone());
        for offset in [30.0, -30.0] {
            for (key, bounds) in &strokes {
                let moved = Aabb::new(
                    bounds.mins + Vector2::new(0.0, offset),
                    bounds.maxs + Vector2::new(0.0, offset),
                );
                tree.update_with_key(*key, moved);
            }
        }

        assert_eq!(tree.iter().count(), strokes.len());
    }
}
