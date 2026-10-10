//! Snapshot-based undo/redo.

use crate::Document;

const LIMIT: usize = 200;

#[derive(Default)]
pub struct History {
    undo: Vec<(String, Document)>,
    redo: Vec<(String, Document)>,
}

impl History {
    /// Record `before` (the state prior to an edit labelled `label`).
    pub fn push(&mut self, label: impl Into<String>, before: Document) {
        self.undo.push((label.into(), before));
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current: &mut Document) -> Option<String> {
        let (label, prev) = self.undo.pop()?;
        let now = std::mem::replace(current, prev);
        self.redo.push((label.clone(), now));
        Some(label)
    }

    pub fn redo(&mut self, current: &mut Document) -> Option<String> {
        let (label, next) = self.redo.pop()?;
        let now = std::mem::replace(current, next);
        self.undo.push((label.clone(), now));
        Some(label)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|(l, _)| l.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|(l, _)| l.as_str())
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FeatureKind, PlaneRef};

    #[test]
    fn undo_redo() {
        let mut d = Document::default();
        let mut h = History::default();
        h.push("Box", d.clone());
        d.add_feature(FeatureKind::Sphere { plane: PlaneRef::XY, center: [0.0; 2], diameter: 5.0 });
        assert_eq!(d.features.len(), 1);
        assert_eq!(h.undo(&mut d).as_deref(), Some("Box"));
        assert_eq!(d.features.len(), 0);
        h.redo(&mut d);
        assert_eq!(d.features.len(), 1);
        assert!(!h.can_redo());
    }
}
