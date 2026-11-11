// Imports
use super::chrono_comp::StrokeLayer;
use super::{StrokeKey, StrokeStore};
use crate::WidgetFlags;
use crate::strokes::{AxisErase, BrushStroke, Stroke};
use p2d::bounding_volume::{Aabb, BoundingVolume};
use rnote_compose::PenPath;
use rnote_compose::shapes::Shapeable;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, rename = "trash_component")]
pub struct TrashComponent {
    #[serde(rename = "trashed")]
    pub trashed: bool,
}

impl Default for TrashComponent {
    fn default() -> Self {
        Self { trashed: false }
    }
}

/// Systems that are related to trashing.
impl StrokeStore {
    /// Rebuild the slotmap with empty trash components with the keys returned from the stroke components.
    pub(crate) fn rebuild_trash_components_slotmap(&mut self) {
        self.trash_components = Arc::new(slotmap::SecondaryMap::new());
        self.stroke_components.keys().for_each(|key| {
            Arc::make_mut(&mut self.trash_components)
                .insert(key, Arc::new(TrashComponent::default()));
        });
    }

    /// Ability if trashing is supported.
    #[allow(unused)]
    pub(crate) fn can_trash(&self, key: StrokeKey) -> bool {
        self.trash_components.get(key).is_some()
    }

    pub(crate) fn trashed(&self, key: StrokeKey) -> Option<bool> {
        self.trash_components.get(key).map(|t| t.trashed)
    }

    pub(crate) fn set_trashed(&mut self, key: StrokeKey, trash: bool) {
        if let Some(trash_comp) = Arc::make_mut(&mut self.trash_components)
            .get_mut(key)
            .map(Arc::make_mut)
        {
            trash_comp.trashed = trash;
            self.update_chrono_to_last(key);
        }
    }

    pub(crate) fn set_trashed_keys(&mut self, keys: &[StrokeKey], trash: bool) {
        keys.iter().for_each(|&key| {
            self.set_selected(key, false);
            self.set_trashed(key, trash);
            self.update_chrono_to_last(key);
        });
    }

    pub(crate) fn trashed_keys_unordered(&self) -> Vec<StrokeKey> {
        self.stroke_components
            .keys()
            .filter(|&key| self.trashed(key).unwrap_or(false))
            .collect()
    }

    /// Removes all trashed strokes permanently from the store.
    #[allow(unused)]
    pub(crate) fn remove_trashed_strokes(&mut self) -> Vec<Stroke> {
        self.trashed_keys_unordered()
            .into_iter()
            .filter_map(|k| self.remove_stroke(k))
            .collect()
    }

    /// For a coordinate system axis with unit markings, erase the ticks and numbers for which `hit`
    /// returns true.
    ///
    /// Returns `None` for other strokes. The stroke is only modified when ticks or numbers were hit,
    /// its geometry is then updated and it needs to update its rendering.
    fn erase_axis_parts(
        &mut self,
        key: StrokeKey,
        hit: impl Fn(&Aabb) -> bool,
    ) -> Option<AxisErase> {
        let stroke = self.stroke_components.get(key)?;
        let Stroke::ShapeStroke(shapestroke) = stroke.as_ref() else {
            return None;
        };
        shapestroke.axis_ticks.as_ref()?;
        // Check first, to not copy strokes that were not hit
        if !stroke.hitboxes().iter().any(&hit) {
            return Some(AxisErase::Miss);
        }

        let stroke = Arc::make_mut(&mut self.stroke_components)
            .get_mut(key)
            .map(Arc::make_mut)?;
        let Stroke::ShapeStroke(shapestroke) = stroke else {
            return None;
        };
        let erase = shapestroke.erase_axis_parts(hit);
        if erase == Some(AxisErase::Parts) {
            self.key_tree.update_with_key(key, stroke.bounds());
            self.set_rendering_dirty(key);
        }
        erase
    }

    /// Trash strokes that collide with the given bounds.
    ///
    /// Ticks and numbers of coordinate system axes are erased individually. Axes that had parts
    /// erased are added to `spared` and are not trashed as a whole while they are in it, so wiping
    /// over a tick does not take the axis it crosses with it.
    ///
    /// Returns the keys of the modified strokes, which need to update their rendering.
    pub(crate) fn trash_colliding_strokes(
        &mut self,
        eraser_bounds: Aabb,
        viewport: Aabb,
        spared: &mut HashSet<StrokeKey>,
    ) -> (Vec<StrokeKey>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();
        let mut modified_keys = vec![];

        self.stroke_keys_as_rendered_intersecting_bounds(viewport)
            .into_iter()
            .for_each(|key| {
                let mut trash_current_stroke = false;

                match self.erase_axis_parts(key, |hitbox| eraser_bounds.intersects(hitbox)) {
                    Some(AxisErase::Parts) => {
                        spared.insert(key);
                        modified_keys.push(key);
                        widget_flags.store_modified = true;
                        widget_flags.resize = true;
                        return;
                    }
                    Some(AxisErase::Axis) if !spared.contains(&key) => {
                        trash_current_stroke = true;
                    }
                    Some(_) => return,
                    None => {}
                }

                if let Some(stroke) = self.stroke_components.get(key)
                    && !trash_current_stroke
                {
                    match stroke.as_ref() {
                        Stroke::BrushStroke(_) | Stroke::ShapeStroke(_) => {
                            // First check if eraser even intersects stroke bounds, avoiding unnecessary work
                            if eraser_bounds.intersects(&stroke.bounds()) {
                                for hitbox in stroke.hitboxes().into_iter() {
                                    if eraser_bounds.intersects(&hitbox) {
                                        trash_current_stroke = true;

                                        break;
                                    }
                                }
                            }
                        }
                        // Ignore other strokes when trashing with the Eraser
                        Stroke::TextStroke(_) | Stroke::VectorImage(_) | Stroke::BitmapImage(_) => {
                        }
                    }
                }

                if trash_current_stroke {
                    self.set_trashed(key, true);
                    widget_flags.store_modified = true;
                    widget_flags.resize = true;
                }
            });

        (modified_keys, widget_flags)
    }

    /// Trash the strokes that the stroke with the given key was drawn over.
    ///
    /// Used for the scribble gesture: the strokes the scribble actually went over are trashed, not
    /// everything inside its bounds, so scratching out a word between two others only takes that
    /// word. Ticks and numbers of coordinate system axes are erased individually.
    ///
    /// Returns how many strokes were trashed or modified, and the keys of the modified strokes,
    /// which need to update their rendering.
    pub(crate) fn trash_strokes_hit_by(
        &mut self,
        key: StrokeKey,
        viewport: Aabb,
    ) -> (usize, Vec<StrokeKey>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();

        let Some(hitting_stroke) = self.stroke_components.get(key) else {
            return (0, vec![], widget_flags);
        };
        let hitting_bounds = hitting_stroke.bounds();
        let hitting_hitboxes = hitting_stroke.hitboxes();

        let candidate_keys = self
            .stroke_keys_as_rendered_intersecting_bounds(viewport)
            .into_iter()
            .filter(|k| *k != key)
            .collect::<Vec<StrokeKey>>();

        let mut modified_keys = vec![];
        let mut axis_trashed_keys = vec![];
        for &k in candidate_keys.iter() {
            match self.erase_axis_parts(k, |hitbox| {
                hitting_hitboxes
                    .iter()
                    .any(|hitting_hitbox| hitting_hitbox.intersects(hitbox))
            }) {
                Some(AxisErase::Parts) => modified_keys.push(k),
                Some(AxisErase::Axis) => axis_trashed_keys.push(k),
                _ => {}
            }
        }

        let mut hit_keys = candidate_keys
            .into_iter()
            .filter(|k| {
                // Axes with unit markings were handled above
                if let Some(Stroke::ShapeStroke(shapestroke)) =
                    self.stroke_components.get(*k).map(|s| s.as_ref())
                    && shapestroke.axis_ticks.is_some()
                {
                    return false;
                }
                let Some(stroke) = self.stroke_components.get(*k) else {
                    return false;
                };
                match stroke.as_ref() {
                    Stroke::BrushStroke(_) | Stroke::ShapeStroke(_) => {}
                    // Ignore the same strokes the eraser ignores.
                    Stroke::TextStroke(_) | Stroke::VectorImage(_) | Stroke::BitmapImage(_) => {
                        return false;
                    }
                }
                // First check the bounds, avoiding unnecessary work
                if !hitting_bounds.intersects(&stroke.bounds()) {
                    return false;
                }

                stroke.hitboxes().into_iter().any(|hitbox| {
                    hitting_hitboxes
                        .iter()
                        .any(|hitting_hitbox| hitting_hitbox.intersects(&hitbox))
                })
            })
            .collect::<Vec<StrokeKey>>();
        hit_keys.extend(axis_trashed_keys);

        for hit_key in hit_keys.iter() {
            self.set_trashed(*hit_key, true);
        }
        if !hit_keys.is_empty() || !modified_keys.is_empty() {
            widget_flags.store_modified = true;
            widget_flags.resize = true;
        }

        (
            hit_keys.len() + modified_keys.len(),
            modified_keys,
            widget_flags,
        )
    }

    /// Remove colliding stroke segments with the given bounds.
    /// The stroke is then split. Strokes that don't have segments are trashed completely.
    ///
    /// Returns the keys of all created or modified strokes.
    ///
    /// The returned strokes need to update their rendering.
    pub(crate) fn split_colliding_strokes(
        &mut self,
        eraser_bounds: Aabb,
        viewport: Aabb,
        spared: &mut HashSet<StrokeKey>,
    ) -> (Vec<StrokeKey>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();
        let mut modified_keys = vec![];

        // Coordinate system axes have their ticks and numbers erased individually
        let mut axis_keys = HashSet::new();
        for key in self.stroke_keys_as_rendered_intersecting_bounds(viewport) {
            match self.erase_axis_parts(key, |hitbox| eraser_bounds.intersects(hitbox)) {
                Some(AxisErase::Parts) => {
                    spared.insert(key);
                    modified_keys.push(key);
                }
                Some(AxisErase::Axis) if !spared.contains(&key) => {
                    self.set_trashed(key, true);
                    modified_keys.push(key);
                }
                Some(_) => {}
                None => continue,
            }
            axis_keys.insert(key);
        }

        let new_strokes = self
            .stroke_keys_as_rendered_intersecting_bounds(viewport)
            .into_iter()
            .filter(|key| !axis_keys.contains(key))
            .flat_map(|key| {
                // Check first, to not copy strokes the eraser is nowhere near
                if !self
                    .stroke_components
                    .get(key)
                    .is_some_and(|stroke| eraser_bounds.intersects(&stroke.bounds()))
                {
                    return vec![];
                }
                let Some(stroke) = Arc::make_mut(&mut self.stroke_components)
                    .get_mut(key)
                    .map(Arc::make_mut)
                else {
                    return vec![];
                };

                let Some(chrono_comp) = self.chrono_components.get(key) else {
                    return vec![];
                };

                let mut new_strokes = vec![];
                let mut trash_current_stroke = false;
                let stroke_bounds = stroke.bounds();

                match stroke {
                    Stroke::BrushStroke(brushstroke) => {
                        if eraser_bounds.intersects(&stroke_bounds) {
                            let mut split = Vec::new();

                            let mut hits = brushstroke
                                .path
                                .hittest(&eraser_bounds, brushstroke.style.stroke_width() * 0.5)
                                .into_iter();

                            if let Some(first_hit) = hits.next() {
                                let mut prev = first_hit;
                                for hit in hits {
                                    let split_slice = &brushstroke.path.segments[prev..hit];

                                    // skip splits that don't have at least two segments (one's end as path start, one additional)
                                    if split_slice.len() > 1 {
                                        split.push(split_slice.to_vec());
                                    }

                                    prev = hit;
                                }

                                // Catch the last
                                let last_split = &brushstroke.path.segments[prev..];
                                if last_split.len() > 1 {
                                    split.push(last_split.to_vec());
                                }

                                for next_split in split {
                                    let mut next_split_iter = next_split.into_iter();
                                    let next_start = next_split_iter.next().unwrap().end();

                                    new_strokes.push((
                                        Stroke::BrushStroke(BrushStroke::from_penpath(
                                            PenPath::new_w_segments(next_start, next_split_iter),
                                            brushstroke.style.clone(),
                                        )),
                                        chrono_comp.layer,
                                    ));
                                }

                                let first_split = &brushstroke.path.segments[..first_hit];
                                // Modify the original stroke at the end.
                                // We keep the start, so we only need at least one segment
                                if !first_split.is_empty() {
                                    brushstroke.replace_path(PenPath::new_w_segments(
                                        brushstroke.path.start,
                                        first_split.to_vec(),
                                    ));
                                } else {
                                    trash_current_stroke = true;
                                }

                                modified_keys.push(key);
                            }
                        }
                    }
                    Stroke::ShapeStroke(_) => {
                        if eraser_bounds.intersects(&stroke_bounds) {
                            for hitbox_elem in stroke.hitboxes().iter() {
                                if eraser_bounds.intersects(hitbox_elem) {
                                    trash_current_stroke = true;
                                    modified_keys.push(key);
                                }
                            }
                        }
                    }
                    // Ignore other strokes when trashing with the Eraser
                    Stroke::TextStroke(_) | Stroke::VectorImage(_) | Stroke::BitmapImage(_) => {}
                }

                if trash_current_stroke {
                    self.set_trashed(key, true);
                    widget_flags.store_modified = true;
                    widget_flags.resize = true;
                }

                new_strokes
            })
            .collect::<Vec<(Stroke, StrokeLayer)>>();

        modified_keys.append(
            &mut new_strokes
                .into_iter()
                .map(|(new_stroke, layer)| self.insert_stroke(new_stroke, Some(layer)))
                .collect(),
        );

        if !modified_keys.is_empty() {
            widget_flags.store_modified = true;
            widget_flags.resize = true;
        }

        (modified_keys, widget_flags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p2d::math::Vector2;
    use rnote_compose::Style;
    use rnote_compose::penpath::Element;
    use rnote_compose::style::smooth::SmoothOptions;

    /// Insert a horizontal stroke at the given height, reaching from `x_start` to `x_end`.
    fn insert_line(store: &mut StrokeStore, y: f64, x_start: f64, x_end: f64) -> StrokeKey {
        let path = PenPath::try_from_elements(
            [x_start, x_end]
                .into_iter()
                .map(|x| Element::new(Vector2::new(x, y), 0.5)),
        )
        .unwrap();
        let key = store.insert_stroke(
            Stroke::BrushStroke(BrushStroke::from_penpath(
                path,
                Style::Smooth(SmoothOptions::default()),
            )),
            None,
        );
        store.update_geometry_for_stroke(key);

        key
    }

    #[test]
    fn trashes_only_the_strokes_that_were_scribbled_over() {
        let mut store = StrokeStore::default();
        let scribbled_over = insert_line(&mut store, 100.0, 100.0, 200.0);
        let untouched = insert_line(&mut store, 400.0, 100.0, 200.0);
        let scribble = insert_line(&mut store, 100.0, 120.0, 180.0);
        let viewport = Aabb::new(Vector2::new(-500.0, -500.0), Vector2::new(1000.0, 1000.0));

        let (trashed, _, _) = store.trash_strokes_hit_by(scribble, viewport);

        assert_eq!(trashed, 1);
        assert_eq!(store.trashed(scribbled_over), Some(true));
        assert_eq!(store.trashed(untouched), Some(false));
        // The scribble itself is left to the caller, which removes it.
        assert_eq!(store.trashed(scribble), Some(false));
    }
}
