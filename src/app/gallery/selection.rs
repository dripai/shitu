use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

// Kit ListState 0.7.1 holds one selected index. Our virtual thumbnail grid
// keeps only selection state here; native events/menu/dialog still own input.
#[derive(Default)]
pub(super) struct PictureSelection {
    pub(super) paths: HashSet<PathBuf>,
    pub(super) current: Option<PathBuf>,
    anchor: Option<PathBuf>,
}
impl PictureSelection {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
    pub(super) fn single(&mut self, path: PathBuf) {
        self.paths.clear();
        self.paths.insert(path.clone());
        self.current = Some(path.clone());
        self.anchor = Some(path);
    }
    pub(super) fn click(&mut self, path: PathBuf, ctrl: bool, shift: bool, ordered: &[PathBuf]) {
        if shift {
            let anchor = self
                .anchor
                .as_ref()
                .or(self.current.as_ref())
                .and_then(|p| ordered.iter().position(|item| item == p));
            let target = ordered.iter().position(|p| p == &path);
            if let (Some(anchor), Some(target)) = (anchor, target) {
                if !ctrl {
                    self.paths.clear();
                }
                self.paths.extend(
                    ordered[anchor.min(target)..=anchor.max(target)]
                        .iter()
                        .cloned(),
                );
                self.current = Some(path);
                return;
            }
        }
        if ctrl {
            if !self.paths.remove(&path) {
                self.paths.insert(path.clone());
            }
            self.anchor = Some(path.clone());
            self.current = Some(path);
        } else {
            self.single(path);
        }
    }
    pub(super) fn context(&mut self, path: PathBuf) {
        if self.paths.contains(&path) {
            self.current = Some(path);
        } else {
            self.single(path);
        }
    }
    pub(super) fn retain(&mut self, ordered: &[PathBuf]) {
        let visible: HashSet<&Path> = ordered.iter().map(PathBuf::as_path).collect();
        self.paths.retain(|p| visible.contains(p.as_path()));
        if self
            .current
            .as_ref()
            .is_some_and(|p| !visible.contains(p.as_path()))
        {
            self.current = ordered.iter().find(|p| self.paths.contains(*p)).cloned();
        }
        if self
            .anchor
            .as_ref()
            .is_some_and(|p| !visible.contains(p.as_path()))
        {
            self.anchor = None;
        }
    }
    pub(super) fn ordered(&self, ordered: &[PathBuf]) -> Vec<PathBuf> {
        ordered
            .iter()
            .filter(|p| self.paths.contains(*p))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::PictureSelection;
    use std::path::PathBuf;
    fn paths() -> Vec<PathBuf> {
        ["a", "b", "c", "d", "e"].map(PathBuf::from).to_vec()
    }
    #[test]
    fn ctrl_toggles_and_shift_uses_fixed_anchor_in_both_directions() {
        let items = paths();
        let mut s = PictureSelection::default();
        s.click(items[1].clone(), false, false, &items);
        s.click(items[3].clone(), true, false, &items);
        assert_eq!(s.ordered(&items), vec![items[1].clone(), items[3].clone()]);
        s.click(items[3].clone(), true, false, &items);
        assert_eq!(s.ordered(&items), vec![items[1].clone()]);
        s.click(items[0].clone(), false, true, &items);
        assert_eq!(s.ordered(&items), items[..4]);
        s.click(items[4].clone(), false, true, &items);
        assert_eq!(s.ordered(&items), items[3..]);
    }
    #[test]
    fn context_on_selection_keeps_group_and_unselected_context_replaces_it() {
        let items = paths();
        let mut s = PictureSelection::default();
        s.single(items[0].clone());
        s.click(items[2].clone(), false, true, &items);
        s.context(items[1].clone());
        assert_eq!(s.ordered(&items), items[..3]);
        s.context(items[4].clone());
        assert_eq!(s.ordered(&items), items[4..]);
        s.retain(&items[..4]);
        assert!(s.paths.is_empty());
        assert!(s.current.is_none());
    }
    #[test]
    fn ranges_follow_current_sort_and_ctrl_shift_adds_to_group() {
        let mut items = paths();
        let mut s = PictureSelection::default();
        s.single(items[0].clone());
        s.click(items[2].clone(), true, false, &items);
        items.reverse();
        s.click(items[0].clone(), true, true, &items);
        assert_eq!(
            s.ordered(&items),
            vec![
                items[0].clone(),
                items[1].clone(),
                items[2].clone(),
                items[4].clone()
            ]
        );
        s.clear();
        s.click(items[0].clone(), false, true, &items);
        assert_eq!(s.paths.len(), 1);
    }
}
