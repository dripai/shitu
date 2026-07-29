use anyhow::{Result, anyhow};
use shi_foundation::i18n;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bounds {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

impl Bounds {
    pub(crate) fn validate(self) -> Result<Self> {
        if self.width < 16 || self.height < 16 {
            return Err(anyhow!(i18n::text(
                "录制区域至少需要 16 × 16 像素",
                "The recording region must be at least 16 × 16 pixels"
            )));
        }
        Ok(self)
    }

    pub(crate) fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left
            && y >= self.top
            && x < self.left.saturating_add(self.width)
            && y < self.top.saturating_add(self.height)
    }
}

/// 平台窗口的不可解释标识。应用核心只负责保存和比较，不依赖 HWND 等原生类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WindowId(u64);

impl WindowId {
    pub(crate) fn from_platform_value(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn platform_value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecordingTarget {
    Screen(Bounds),
    Window {
        id: WindowId,
        initial_bounds: Bounds,
    },
    Region(Bounds),
}

impl RecordingTarget {
    pub(crate) fn initial_bounds(self) -> Bounds {
        match self {
            Self::Screen(bounds) | Self::Region(bounds) => bounds,
            Self::Window { initial_bounds, .. } => initial_bounds,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MonitorCandidate {
    pub(crate) bounds: Bounds,
    pub(crate) primary: bool,
}

pub(crate) struct MonitorCandidates {
    values: Vec<MonitorCandidate>,
}

impl MonitorCandidates {
    pub(crate) fn new(values: Vec<MonitorCandidate>) -> Self {
        Self { values }
    }

    pub(crate) fn get(&self, index: usize) -> Option<MonitorCandidate> {
        self.values.get(index).copied()
    }

    pub(crate) fn primary_index(&self) -> usize {
        self.values
            .iter()
            .position(|monitor| monitor.primary)
            .unwrap_or(0)
    }

    pub(crate) fn index_of(&self, bounds: Bounds) -> Option<usize> {
        self.values
            .iter()
            .position(|monitor| monitor.bounds == bounds)
    }

    pub(crate) fn labels(&self) -> Vec<String> {
        self.values
            .iter()
            .enumerate()
            .map(|(index, monitor)| {
                format!(
                    "{} {} · {} × {}{}",
                    i18n::text("显示器", "Display"),
                    index + 1,
                    monitor.bounds.width,
                    monitor.bounds.height,
                    if monitor.primary {
                        i18n::text("（主显示器）", " (primary)")
                    } else {
                        ""
                    }
                )
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WindowCandidate {
    pub(crate) id: WindowId,
    pub(crate) bounds: Bounds,
    pub(crate) title: String,
}

pub(crate) struct WindowCandidates {
    values: Vec<WindowCandidate>,
}

impl WindowCandidates {
    pub(crate) fn new(values: Vec<WindowCandidate>) -> Self {
        Self { values }
    }

    pub(crate) fn target_at(&self, x: i32, y: i32) -> Option<WindowCandidate> {
        self.values
            .iter()
            .find(|candidate| candidate.bounds.contains(x, y))
            .cloned()
    }

    pub(crate) fn exclude(&mut self, id: WindowId) {
        self.values.retain(|candidate| candidate.id != id);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioSourceKind {
    System,
    Microphone,
}

pub(crate) fn output_size(source: Bounds, quality_preset: u8) -> (u32, u32) {
    let maximum = match quality_preset {
        1 => Some((1280_u32, 720_u32)),
        2 | 0 => Some((1920_u32, 1080_u32)),
        _ => None,
    };
    let source_width = source.width.max(16) as u32;
    let source_height = source.height.max(16) as u32;
    let (mut width, mut height) = if let Some((max_width, max_height)) = maximum {
        let scale = (max_width as f64 / source_width as f64)
            .min(max_height as f64 / source_height as f64)
            .min(1.0);
        (
            (source_width as f64 * scale).round() as u32,
            (source_height as f64 * scale).round() as u32,
        )
    } else {
        (source_width, source_height)
    };
    width = width.max(16) & !1;
    height = height.max(16) & !1;
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::{
        Bounds, MonitorCandidate, MonitorCandidates, WindowCandidate, WindowCandidates, WindowId,
        output_size,
    };

    #[test]
    fn bounds_validate_and_contain_points() {
        let bounds = Bounds {
            left: -20,
            top: 10,
            width: 100,
            height: 60,
        };
        assert!(bounds.validate().is_ok());
        assert!(bounds.contains(-20, 10));
        assert!(bounds.contains(79, 69));
        assert!(!bounds.contains(80, 70));
    }

    #[test]
    fn monitor_candidates_identify_primary_and_format_labels() {
        let primary = Bounds {
            left: 0,
            top: 0,
            width: 1920,
            height: 1080,
        };
        let secondary = Bounds {
            left: -2560,
            top: 0,
            width: 2560,
            height: 1440,
        };
        let monitors = MonitorCandidates::new(vec![
            MonitorCandidate {
                bounds: primary,
                primary: true,
            },
            MonitorCandidate {
                bounds: secondary,
                primary: false,
            },
        ]);

        assert_eq!(monitors.primary_index(), 0);
        assert_eq!(monitors.index_of(secondary), Some(1));
        assert_eq!(
            monitors.labels(),
            vec![
                "显示器 1 · 1920 × 1080（主显示器）",
                "显示器 2 · 2560 × 1440",
            ]
        );
    }

    #[test]
    fn window_candidates_keep_platform_ids_opaque() {
        let front = WindowCandidate {
            id: WindowId::from_platform_value(1),
            bounds: Bounds {
                left: 100,
                top: 100,
                width: 200,
                height: 200,
            },
            title: "Front window".to_owned(),
        };
        let back = WindowCandidate {
            id: WindowId::from_platform_value(2),
            bounds: Bounds {
                left: 0,
                top: 0,
                width: 500,
                height: 500,
            },
            title: "Back window".to_owned(),
        };
        let mut candidates = WindowCandidates::new(vec![front.clone(), back]);

        assert_eq!(candidates.target_at(150, 150), Some(front.clone()));
        candidates.exclude(front.id);
        assert_ne!(candidates.target_at(150, 150), Some(front));
    }

    #[test]
    fn output_dimensions_preserve_ratio_and_even_dimensions() {
        let source = Bounds {
            left: 0,
            top: 0,
            width: 2560,
            height: 1440,
        };
        assert_eq!(output_size(source, 1), (1280, 720));
        assert_eq!(output_size(source, 2), (1920, 1080));
        assert_eq!(output_size(source, 3), (2560, 1440));
        let odd = Bounds {
            width: 801,
            height: 601,
            ..source
        };
        let size = output_size(odd, 3);
        assert_eq!(size.0 % 2, 0);
        assert_eq!(size.1 % 2, 0);
    }
}
