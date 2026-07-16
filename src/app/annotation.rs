use std::fmt::Write as _;

use slint::Color;

use super::AnnotationView;
use crate::image::{CapturedImage, DrawStyle, arrow_head};

#[derive(Default)]
pub struct AnnotationHistory {
    commands: Vec<AnnotationCommand>,
    redo: Vec<AnnotationCommand>,
    active: bool,
}

impl AnnotationHistory {
    pub fn clear(&mut self) {
        self.commands.clear();
        self.redo.clear();
        self.active = false;
    }

    pub fn begin(&mut self, tool: i32, point: (u32, u32), style: DrawStyle) {
        self.finish();
        let command = match tool {
            1 => AnnotationCommand::Pen {
                points: vec![point],
                style,
            },
            2 => AnnotationCommand::Rectangle {
                start: point,
                end: point,
                style,
            },
            3 => AnnotationCommand::Arrow {
                start: point,
                end: point,
                style,
            },
            _ => return,
        };
        self.redo.clear();
        self.commands.push(command);
        self.active = true;
    }

    pub fn update(&mut self, point: (u32, u32)) {
        if !self.active {
            return;
        }
        match self.commands.last_mut() {
            Some(AnnotationCommand::Pen { points, .. }) => {
                if points.last().copied() != Some(point) {
                    points.push(point);
                }
            }
            Some(AnnotationCommand::Rectangle { end, .. })
            | Some(AnnotationCommand::Arrow { end, .. }) => {
                *end = point;
            }
            None => {}
        }
    }

    pub fn finish(&mut self) {
        self.active = false;
    }

    pub fn undo(&mut self) {
        self.finish();
        if let Some(command) = self.commands.pop() {
            self.redo.push(command);
        }
    }

    pub fn redo(&mut self) {
        self.finish();
        if let Some(command) = self.redo.pop() {
            self.commands.push(command);
        }
    }

    pub fn views(&self) -> Vec<AnnotationView> {
        self.commands.iter().map(AnnotationCommand::view).collect()
    }

    pub fn render(&self, base: &CapturedImage) -> CapturedImage {
        let mut image = base.clone();
        for command in &self.commands {
            command.render(&mut image);
        }
        image
    }
}

#[derive(Clone)]
enum AnnotationCommand {
    Pen {
        points: Vec<(u32, u32)>,
        style: DrawStyle,
    },
    Rectangle {
        start: (u32, u32),
        end: (u32, u32),
        style: DrawStyle,
    },
    Arrow {
        start: (u32, u32),
        end: (u32, u32),
        style: DrawStyle,
    },
}

impl AnnotationCommand {
    fn view(&self) -> AnnotationView {
        let (commands, style) = match self {
            Self::Pen { points, style } => (pen_path(points), *style),
            Self::Rectangle { start, end, style } => (rectangle_path(*start, *end), *style),
            Self::Arrow { start, end, style } => (arrow_path(*start, *end), *style),
        };
        AnnotationView {
            commands: commands.into(),
            stroke_color: Color::from_argb_u8(
                style.rgba[3],
                style.rgba[0],
                style.rgba[1],
                style.rgba[2],
            ),
            stroke_width: (style.radius * 2) as f32,
        }
    }

    fn render(&self, image: &mut CapturedImage) {
        match self {
            Self::Pen { points, style } => {
                if points.len() == 1 {
                    image.draw_line(points[0], points[0], *style);
                } else {
                    for pair in points.windows(2) {
                        image.draw_line(pair[0], pair[1], *style);
                    }
                }
            }
            Self::Rectangle { start, end, style } => {
                image.draw_rectangle(*start, *end, *style);
            }
            Self::Arrow { start, end, style } => {
                image.draw_arrow(*start, *end, *style);
            }
        }
    }
}

fn pen_path(points: &[(u32, u32)]) -> String {
    let Some(first) = points.first() else {
        return String::new();
    };
    let mut path = format!("M {} {}", first.0, first.1);
    for point in &points[1..] {
        let _ = write!(path, " L {} {}", point.0, point.1);
    }
    if points.len() == 1 {
        let _ = write!(path, " L {} {}", first.0, first.1);
    }
    path
}

fn rectangle_path(start: (u32, u32), end: (u32, u32)) -> String {
    let left = start.0.min(end.0);
    let right = start.0.max(end.0);
    let top = start.1.min(end.1);
    let bottom = start.1.max(end.1);
    format!("M {left} {top} L {right} {top} L {right} {bottom} L {left} {bottom} Z")
}

fn arrow_path(start: (u32, u32), end: (u32, u32)) -> String {
    let mut path = format!("M {} {} L {} {}", start.0, start.1, end.0, end.1);
    if let Some((left, right)) = arrow_head(start, end) {
        let _ = write!(
            path,
            " M {} {} L {} {} M {} {} L {} {}",
            end.0, end.1, left.0, left.1, end.0, end.1, right.0, right.1
        );
    }
    path
}

#[cfg(test)]
mod tests {
    use super::{AnnotationHistory, DrawStyle, arrow_path, rectangle_path};
    use crate::image::CapturedImage;

    #[test]
    fn undo_and_redo_preserve_commands() {
        let mut history = AnnotationHistory::default();
        history.begin(
            2,
            (10, 20),
            DrawStyle {
                rgba: [255, 0, 0, 255],
                radius: 2,
            },
        );
        history.update((30, 40));
        history.finish();
        assert_eq!(history.commands.len(), 1);
        history.undo();
        assert!(history.commands.is_empty());
        history.redo();
        assert_eq!(history.commands.len(), 1);
    }

    #[test]
    fn shape_paths_use_image_coordinates() {
        assert_eq!(
            rectangle_path((30, 40), (10, 20)),
            "M 10 20 L 30 20 L 30 40 L 10 40 Z"
        );
        assert!(arrow_path((10, 10), (30, 20)).starts_with("M 10 10 L 30 20"));
    }

    #[test]
    fn rendering_composites_annotations_without_changing_the_base_image() {
        let base = CapturedImage::from_rgba(0, 0, 4, 4, &[0; 4 * 4 * 4]).unwrap();
        let original = base.rgba_bytes();
        let mut history = AnnotationHistory::default();
        history.begin(
            1,
            (1, 1),
            DrawStyle {
                rgba: [255, 0, 0, 255],
                radius: 1,
            },
        );
        history.update((2, 2));
        history.finish();

        let rendered = history.render(&base);
        assert_eq!(base.rgba_bytes(), original);
        assert_ne!(rendered.rgba_bytes(), original);
    }
}
