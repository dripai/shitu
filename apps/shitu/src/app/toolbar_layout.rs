use anyhow::{Result, ensure};

// Slint 1.17 exposes native layouts and window coordinates, but no work-area
// placement for a selection. Keep this policy independent of Slint and Win32;
// the callers supply monitor work areas and actual toolbar metrics in one unit.
const MARGIN: f64 = 8.0;
const GAP: f64 = 12.0;
const SWITCH_ROOM: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Rect {
    pub fn width(self) -> f64 {
        self.right - self.left
    }

    pub fn height(self) -> f64 {
        self.bottom - self.top
    }

    /// Convert physical screen coordinates into a window's logical coordinates.
    pub fn to_logical(self, origin_x: f64, origin_y: f64, scale: f64) -> Self {
        Self {
            left: (self.left - origin_x) / scale,
            top: (self.top - origin_y) / scale,
            right: (self.right - origin_x) / scale,
            bottom: (self.bottom - origin_y) / scale,
        }
    }

    fn valid(self) -> bool {
        [self.left, self.top, self.right, self.bottom]
            .into_iter()
            .all(f64::is_finite)
            && self.width() > 0.0
            && self.height() > 0.0
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Metrics {
    pub width: f64,
    pub height: f64,
    pub main_height: f64,
    pub expanded_height: f64,
    // Existing pin windows need extra room for native ComboBox popups.
    pub popup_padding: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Below,
    Above,
    Inside,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Placement {
    pub x: f64,
    pub y: f64,
    pub properties_above: bool,
}

#[derive(Default)]
pub(super) struct ToolbarLayout {
    anchor_ratio: Option<f64>,
    side: Option<Side>,
    properties_above: Option<bool>,
}

impl ToolbarLayout {
    pub fn is_initialized(&self) -> bool {
        self.anchor_ratio.is_some()
    }

    /// Only the first selection/show captures a pointer anchor. Dragging,
    /// resizing, reopening and changing tools must preserve that anchor.
    pub fn initialize(&mut self, selection: Rect, pointer_x: f64) -> Result<()> {
        ensure!(
            selection.valid() && pointer_x.is_finite(),
            "Invalid toolbar anchor"
        );
        if self.anchor_ratio.is_none() {
            self.anchor_ratio =
                Some(((pointer_x - selection.left) / selection.width()).clamp(0.0, 1.0));
        }
        Ok(())
    }

    pub fn place(&mut self, selection: Rect, work: Rect, metrics: Metrics) -> Result<Placement> {
        ensure!(
            selection.valid() && work.valid(),
            "Invalid toolbar geometry"
        );
        ensure!(
            self.is_initialized(),
            "Toolbar anchor has not been initialized"
        );
        ensure!(
            [
                metrics.width,
                metrics.height,
                metrics.main_height,
                metrics.expanded_height,
                metrics.popup_padding
            ]
            .into_iter()
            .all(f64::is_finite)
                && metrics.width > 0.0
                && metrics.main_height > 0.0
                && metrics.height >= metrics.main_height
                && metrics.expanded_height >= metrics.height
                && metrics.popup_padding >= 0.0,
            "Invalid toolbar dimensions"
        );
        let full_height = metrics.expanded_height + metrics.popup_padding;
        ensure!(
            work.width() >= metrics.width + 2.0 * MARGIN
                && work.height() >= full_height + 2.0 * MARGIN,
            "Monitor work area is too small for the toolbar"
        );

        let left = work.left + MARGIN;
        let right = work.right - MARGIN;
        let top = work.top + MARGIN;
        let bottom = work.bottom - MARGIN;
        let center = selection.left + selection.width() * self.anchor_ratio.unwrap();
        let preferred_x = if selection.width() >= metrics.width {
            (center - metrics.width / 2.0).clamp(selection.left, selection.right - metrics.width)
        } else {
            selection.left + (selection.width() - metrics.width) / 2.0
        };
        let x = preferred_x.clamp(left, right - metrics.width);

        let below_room = bottom - (selection.bottom + GAP);
        let above_room = selection.top - GAP - top;
        let row_height = metrics.main_height + metrics.popup_padding;
        let side = match self.side {
            Some(Side::Above) => {
                if below_room >= full_height + SWITCH_ROOM {
                    Side::Below
                } else if above_room >= row_height {
                    Side::Above
                } else if below_room >= row_height {
                    Side::Below
                } else {
                    Side::Inside
                }
            }
            Some(Side::Inside) => {
                if below_room >= full_height + SWITCH_ROOM {
                    Side::Below
                } else if above_room >= full_height + SWITCH_ROOM {
                    Side::Above
                } else {
                    Side::Inside
                }
            }
            previous => {
                let switch_room = if previous.is_some() { SWITCH_ROOM } else { 0.0 };
                if below_room >= full_height {
                    Side::Below
                } else if above_room >= full_height + switch_room {
                    Side::Above
                } else if below_room >= row_height {
                    Side::Below
                } else if above_room >= row_height {
                    Side::Above
                } else {
                    Side::Inside
                }
            }
        };
        let mut main_y = match side {
            Side::Below => selection.bottom + GAP,
            Side::Above => selection.top - GAP - metrics.main_height,
            Side::Inside => selection.bottom - GAP - metrics.main_height,
        }
        .clamp(top, bottom - metrics.main_height);

        // Reserve expansion space even while collapsed, so changing tools
        // cannot move the main row or change its attachment side.
        let extra = metrics.expanded_height - metrics.main_height + metrics.popup_padding;
        let room_before = main_y - top;
        let room_after = bottom - main_y - metrics.main_height;
        let prefer_above = side != Side::Below;
        let properties_above = if self.side == Some(side) {
            match self.properties_above {
                Some(true) if room_before >= extra => {
                    prefer_above || room_after < extra + SWITCH_ROOM
                }
                Some(false) if room_after >= extra => {
                    prefer_above && room_before >= extra + SWITCH_ROOM
                }
                _ => {
                    if room_before >= extra && room_after >= extra {
                        prefer_above
                    } else {
                        room_before > room_after
                    }
                }
            }
        } else if room_before >= extra && room_after >= extra {
            prefer_above
        } else {
            room_before > room_after
        };
        main_y = if properties_above {
            main_y.clamp(top + extra, bottom - metrics.main_height)
        } else {
            main_y.clamp(top, bottom - metrics.main_height - extra)
        };
        let y = if properties_above {
            main_y - (metrics.height - metrics.main_height) - metrics.popup_padding
        } else {
            main_y
        };
        self.side = Some(side);
        self.properties_above = Some(properties_above);
        Ok(Placement {
            x,
            y,
            properties_above,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect {
        left: 0.0,
        top: 0.0,
        right: 1920.0,
        bottom: 1040.0,
    };
    const COLLAPSED: Metrics = Metrics {
        width: 432.0,
        height: 38.0,
        main_height: 38.0,
        expanded_height: 106.0,
        popup_padding: 0.0,
    };

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        }
    }

    fn initialized(selection: Rect, pointer: f64) -> ToolbarLayout {
        let mut state = ToolbarLayout::default();
        state.initialize(selection, pointer).unwrap();
        state
    }

    fn main_y(placement: Placement, metrics: Metrics) -> f64 {
        placement.y
            + if placement.properties_above {
                metrics.height - metrics.main_height + metrics.popup_padding
            } else {
                0.0
            }
    }

    #[test]
    fn first_toolbar_center_is_near_release_and_stays_inside_wide_selection() {
        let selection = rect(300.0, 200.0, 900.0, 300.0);
        for (pointer, expected_x) in [(300.0, 300.0), (750.0, 534.0), (1200.0, 768.0)] {
            let placement = initialized(selection, pointer)
                .place(selection, WORK, COLLAPSED)
                .unwrap();
            assert_eq!(placement.x, expected_x);
            assert_eq!(placement.y, 512.0);
        }
    }

    #[test]
    fn narrow_selection_centers_toolbar_independent_of_release_direction() {
        let selection = rect(800.0, 200.0, 100.0, 100.0);
        for pointer in [800.0, 900.0] {
            let placement = initialized(selection, pointer)
                .place(selection, WORK, COLLAPSED)
                .unwrap();
            assert_eq!(placement.x, 634.0);
        }
    }

    #[test]
    fn dragging_and_resizing_preserve_first_anchor() {
        let selection = rect(300.0, 200.0, 900.0, 300.0);
        let mut state = initialized(selection, 750.0);
        let first = state.place(selection, WORK, COLLAPSED).unwrap();
        let moved = rect(400.0, 250.0, 900.0, 300.0);
        state.initialize(moved, 1290.0).unwrap();
        let next = state.place(moved, WORK, COLLAPSED).unwrap();
        assert_eq!(next.x - first.x, 100.0);
        assert_eq!(next.y - first.y, 50.0);
        let resized = rect(400.0, 250.0, 1100.0, 300.0);
        assert_eq!(state.place(resized, WORK, COLLAPSED).unwrap().x, 734.0);
    }

    #[test]
    fn main_row_stays_fixed_when_properties_expand_on_either_side() {
        let expanded = Metrics {
            height: 106.0,
            ..COLLAPSED
        };
        for selection in [
            rect(300.0, 200.0, 700.0, 300.0),
            rect(300.0, 600.0, 700.0, 420.0),
        ] {
            let mut state = initialized(selection, selection.right);
            let closed = state.place(selection, WORK, COLLAPSED).unwrap();
            let open = state.place(selection, WORK, expanded).unwrap();
            assert_eq!(closed.x, open.x);
            assert_eq!(main_y(closed, COLLAPSED), main_y(open, expanded));
            assert_eq!(closed.properties_above, open.properties_above);
        }
    }

    #[test]
    fn switching_back_below_requires_extra_room() {
        let mut selection = rect(300.0, 600.0, 700.0, 340.0);
        let mut state = initialized(selection, selection.right);
        assert!(
            state
                .place(selection, WORK, COLLAPSED)
                .unwrap()
                .properties_above
        );
        selection.bottom = 905.0; // 115px below: fits, but is within hysteresis.
        state.place(selection, WORK, COLLAPSED).unwrap();
        assert_eq!(state.side, Some(Side::Above));
        selection.bottom = 898.0; // 122px below: enough expansion + 16px margin.
        state.place(selection, WORK, COLLAPSED).unwrap();
        assert_eq!(state.side, Some(Side::Below));
    }

    #[test]
    fn full_screen_selection_uses_inside_edge_above_taskbar() {
        let selection = rect(0.0, 0.0, 1920.0, 1080.0);
        let metrics = Metrics {
            height: 106.0,
            ..COLLAPSED
        };
        let mut state = initialized(selection, 1919.0);
        let placement = state.place(selection, WORK, metrics).unwrap();
        assert_eq!(state.side, Some(Side::Inside));
        assert!(placement.properties_above);
        assert_eq!(placement.x + metrics.width, 1912.0);
        assert_eq!(placement.y + metrics.height, 1032.0);
    }

    #[test]
    fn pin_popup_padding_preserves_main_row_and_fits_work_area() {
        for selection in [
            rect(300.0, 200.0, 700.0, 300.0),
            rect(300.0, 600.0, 700.0, 400.0),
        ] {
            let collapsed = Metrics {
                popup_padding: 34.0,
                ..COLLAPSED
            };
            let expanded = Metrics {
                height: 106.0,
                ..collapsed
            };
            let mut state = initialized(selection, selection.right);
            let closed = state.place(selection, WORK, collapsed).unwrap();
            let open = state.place(selection, WORK, expanded).unwrap();
            assert_eq!(main_y(closed, collapsed), main_y(open, expanded));
            assert!(open.y >= WORK.top + MARGIN);
            assert!(open.y + expanded.height + expanded.popup_padding <= WORK.bottom - MARGIN);
        }
    }

    #[test]
    fn negative_monitor_coordinates_are_preserved() {
        let work = rect(-1920.0, -200.0, 1920.0, 1040.0);
        let selection = rect(-1850.0, -180.0, 700.0, 300.0);
        let placement = initialized(selection, -1840.0)
            .place(selection, work, COLLAPSED)
            .unwrap();
        assert_eq!(placement.x, -1850.0);
        assert_eq!(placement.y, 132.0);
    }

    #[test]
    fn physical_to_logical_conversion_handles_mixed_dpi_and_virtual_origin() {
        let logical = rect(600.0, 200.0, 800.0, 300.0);
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let origin = (-1920.0, -200.0);
            let physical = Rect {
                left: origin.0 + logical.left * scale,
                top: origin.1 + logical.top * scale,
                right: origin.0 + logical.right * scale,
                bottom: origin.1 + logical.bottom * scale,
            };
            assert_eq!(physical.to_logical(origin.0, origin.1, scale), logical);
        }
    }

    #[test]
    fn all_edges_keep_expansion_inside_work_area_without_moving_main_row() {
        for popup_padding in [0.0, 34.0] {
            for x in [-100.0, 0.0, 1000.0, 1900.0] {
                for y in [-100.0, 0.0, 500.0, 950.0] {
                    for width in [20.0, 600.0, 2100.0] {
                        let selection = rect(x, y, width, 100.0);
                        let collapsed = Metrics {
                            popup_padding,
                            ..COLLAPSED
                        };
                        let expanded = Metrics {
                            height: 106.0,
                            ..collapsed
                        };
                        let mut state = initialized(selection, selection.right);
                        let closed = state.place(selection, WORK, collapsed).unwrap();
                        let open = state.place(selection, WORK, expanded).unwrap();
                        assert_eq!(main_y(closed, collapsed), main_y(open, expanded));
                        assert!(open.x >= WORK.left + MARGIN);
                        assert!(open.x + expanded.width <= WORK.right - MARGIN);
                        assert!(open.y >= WORK.top + MARGIN);
                        assert!(open.y + expanded.height + popup_padding <= WORK.bottom - MARGIN);
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_geometry_and_missing_anchor_report_errors() {
        let selection = rect(300.0, 200.0, 700.0, 300.0);
        assert!(
            ToolbarLayout::default()
                .place(selection, WORK, COLLAPSED)
                .is_err()
        );
        let mut state = initialized(selection, selection.right);
        assert!(
            state
                .place(rect(0.0, 0.0, 0.0, 100.0), WORK, COLLAPSED)
                .is_err()
        );
        assert!(
            state
                .place(selection, rect(0.0, 0.0, 400.0, 100.0), COLLAPSED)
                .is_err()
        );
        assert!(
            state
                .place(
                    selection,
                    WORK,
                    Metrics {
                        height: f64::NAN,
                        ..COLLAPSED
                    }
                )
                .is_err()
        );
    }
}
