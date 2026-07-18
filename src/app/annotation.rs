use std::fmt::Write as _;

use slint::Color;

use anyhow::Result;

use super::AnnotationView;
use crate::image::{CapturedImage, DrawStyle, arrow_head};

#[derive(Default)]
pub struct AnnotationHistory {
    commands: Vec<AnnotationCommand>,
    undo: Vec<Vec<AnnotationCommand>>,
    redo: Vec<Vec<AnnotationCommand>>,
    active: bool,
    active_tool: i32,
    active_before: Option<Vec<AnnotationCommand>>,
}

impl AnnotationHistory {
    pub fn clear(&mut self) {
        self.commands.clear();
        self.undo.clear();
        self.redo.clear();
        self.active = false;
        self.active_tool = 0;
        self.active_before = None;
    }

    pub fn begin(&mut self, tool: i32, point: (u32, u32), style: DrawStyle) {
        self.finish();
        self.active_before = Some(self.commands.clone());
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
            5 => {
                self.active = true;
                self.active_tool = tool;
                self.erase_at(point, style.radius.max(4) as f64 * 2.0);
                return;
            }
            6 => {
                let (radius, block_size) = mosaic_parameters(style.radius);
                AnnotationCommand::Mosaic {
                    points: vec![point],
                    radius,
                    block_size,
                }
            }
            _ => {
                self.active_before = None;
                return;
            }
        };
        self.commands.push(command);
        self.active = true;
        self.active_tool = tool;
    }

    pub fn update(&mut self, point: (u32, u32)) {
        if !self.active {
            return;
        }
        if self.active_tool == 5 {
            self.erase_at(point, 10.0);
            return;
        }
        match self.commands.last_mut() {
            Some(AnnotationCommand::Pen { points, .. })
            | Some(AnnotationCommand::Mosaic { points, .. }) => {
                if points.last().copied() != Some(point) {
                    points.push(point);
                }
            }
            Some(AnnotationCommand::Rectangle { end, .. })
            | Some(AnnotationCommand::Arrow { end, .. }) => {
                *end = point;
            }
            Some(AnnotationCommand::Text { .. }) => {}
            None => {}
        }
    }

    pub fn finish(&mut self) {
        if self.active
            && let Some(before) = self.active_before.take()
            && before != self.commands
        {
            self.undo.push(before);
            self.redo.clear();
        }
        self.active = false;
        self.active_tool = 0;
        self.active_before = None;
    }

    pub fn add_text(&mut self, position: (u32, u32), text: &str, style: DrawStyle, font_size: u32) {
        self.finish();
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.undo.push(self.commands.clone());
        self.redo.clear();
        self.commands.push(AnnotationCommand::Text {
            position,
            text: text.to_owned(),
            style,
            font_size,
        });
    }

    pub fn undo(&mut self) {
        self.finish();
        if let Some(previous) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.commands, previous));
        }
    }

    pub fn redo(&mut self) {
        self.finish();
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.commands, next));
        }
    }

    pub fn views(&self) -> Vec<AnnotationView> {
        self.commands
            .iter()
            .filter_map(AnnotationCommand::view)
            .collect()
    }

    pub fn preview_base(&self, base: &CapturedImage) -> CapturedImage {
        let mut image = base.clone();
        for command in &self.commands {
            command.render_mosaic(&mut image);
        }
        image
    }

    pub fn render(&self, base: &CapturedImage) -> Result<CapturedImage> {
        let mut image = self.preview_base(base);
        for command in &self.commands {
            if !matches!(command, AnnotationCommand::Mosaic { .. }) {
                command.render(&mut image)?;
            }
        }
        Ok(image)
    }

    fn erase_at(&mut self, point: (u32, u32), tolerance: f64) {
        self.commands
            .retain(|command| !command.hit_test(point, tolerance));
    }
}

#[derive(Clone, PartialEq)]
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
    Text {
        position: (u32, u32),
        text: String,
        style: DrawStyle,
        font_size: u32,
    },
    Mosaic {
        points: Vec<(u32, u32)>,
        radius: u32,
        block_size: u32,
    },
}

impl AnnotationCommand {
    fn view(&self) -> Option<AnnotationView> {
        match self {
            Self::Pen { points, style } => Some(path_view(pen_path(points), *style)),
            Self::Rectangle { start, end, style } => {
                Some(path_view(rectangle_path(*start, *end), *style))
            }
            Self::Arrow { start, end, style } => Some(path_view(arrow_path(*start, *end), *style)),
            Self::Text {
                position,
                text,
                style,
                font_size,
            } => Some(AnnotationView {
                kind: 1,
                commands: String::new().into(),
                stroke_color: style_color(*style),
                stroke_width: 0.0,
                x: position.0 as f32,
                y: position.1 as f32,
                text: text.clone().into(),
                font_size: *font_size as f32,
            }),
            Self::Mosaic { .. } => None,
        }
    }

    fn render_mosaic(&self, image: &mut CapturedImage) {
        if let Self::Mosaic {
            points,
            radius,
            block_size,
        } = self
        {
            image.pixelate_stroke(points, *radius, *block_size);
        }
    }

    fn render(&self, image: &mut CapturedImage) -> Result<()> {
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
            Self::Text {
                position,
                text,
                style,
                font_size,
            } => image.draw_text(*position, text, *font_size, style.rgba)?,
            Self::Mosaic { .. } => self.render_mosaic(image),
        }
        Ok(())
    }

    fn hit_test(&self, point: (u32, u32), tolerance: f64) -> bool {
        match self {
            Self::Pen { points, style } => {
                let tolerance = tolerance + style.radius.max(1) as f64;
                points
                    .windows(2)
                    .any(|pair| distance_to_segment(point, pair[0], pair[1]) <= tolerance)
                    || points
                        .first()
                        .is_some_and(|candidate| distance(*candidate, point) <= tolerance)
            }
            Self::Rectangle { start, end, style } => {
                let tolerance = tolerance + style.radius.max(1) as f64;
                let left = start.0.min(end.0);
                let right = start.0.max(end.0);
                let top = start.1.min(end.1);
                let bottom = start.1.max(end.1);
                [
                    ((left, top), (right, top)),
                    ((right, top), (right, bottom)),
                    ((right, bottom), (left, bottom)),
                    ((left, bottom), (left, top)),
                ]
                .into_iter()
                .any(|(a, b)| distance_to_segment(point, a, b) <= tolerance)
            }
            Self::Arrow { start, end, style } => {
                let tolerance = tolerance + style.radius.max(1) as f64;
                distance_to_segment(point, *start, *end) <= tolerance
                    || arrow_head(*start, *end).is_some_and(|(left, right)| {
                        distance_to_segment(point, *end, left) <= tolerance
                            || distance_to_segment(point, *end, right) <= tolerance
                    })
            }
            Self::Text {
                position,
                text,
                font_size,
                ..
            } => {
                let (width, height) = estimated_text_size(text, *font_size);
                let x = point.0 as f64;
                let y = point.1 as f64;
                x >= position.0 as f64 - tolerance
                    && x <= position.0 as f64 + width + tolerance
                    && y >= position.1 as f64 - tolerance
                    && y <= position.1 as f64 + height + tolerance
            }
            Self::Mosaic { points, radius, .. } => {
                let tolerance = tolerance + *radius as f64;
                points
                    .windows(2)
                    .any(|pair| distance_to_segment(point, pair[0], pair[1]) <= tolerance)
                    || points
                        .first()
                        .is_some_and(|candidate| distance(*candidate, point) <= tolerance)
            }
        }
    }
}

fn mosaic_parameters(size: i32) -> (u32, u32) {
    match size {
        1 => (10, 6),
        4.. => (30, 14),
        _ => (18, 10),
    }
}

fn path_view(commands: String, style: DrawStyle) -> AnnotationView {
    AnnotationView {
        kind: 0,
        commands: commands.into(),
        stroke_color: style_color(style),
        stroke_width: (style.radius * 2) as f32,
        x: 0.0,
        y: 0.0,
        text: String::new().into(),
        font_size: 0.0,
    }
}

fn style_color(style: DrawStyle) -> Color {
    Color::from_argb_u8(style.rgba[3], style.rgba[0], style.rgba[1], style.rgba[2])
}

fn estimated_text_size(text: &str, font_size: u32) -> (f64, f64) {
    let units = text
        .chars()
        .map(|ch| if ch.is_ascii() { 0.62 } else { 1.0 })
        .sum::<f64>();
    (units * font_size as f64, font_size as f64 * 1.35)
}

fn distance(a: (u32, u32), b: (u32, u32)) -> f64 {
    let dx = a.0 as f64 - b.0 as f64;
    let dy = a.1 as f64 - b.1 as f64;
    (dx * dx + dy * dy).sqrt()
}

fn distance_to_segment(point: (u32, u32), start: (u32, u32), end: (u32, u32)) -> f64 {
    let px = point.0 as f64;
    let py = point.1 as f64;
    let sx = start.0 as f64;
    let sy = start.1 as f64;
    let dx = end.0 as f64 - sx;
    let dy = end.1 as f64 - sy;
    let length_squared = dx * dx + dy * dy;
    if length_squared == 0.0 {
        return distance(point, start);
    }
    let t = (((px - sx) * dx + (py - sy) * dy) / length_squared).clamp(0.0, 1.0);
    let nearest_x = sx + t * dx;
    let nearest_y = sy + t * dy;
    ((px - nearest_x).powi(2) + (py - nearest_y).powi(2)).sqrt()
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

        let rendered = history.render(&base).unwrap();
        assert_eq!(base.rgba_bytes(), original);
        assert_ne!(rendered.rgba_bytes(), original);
    }

    #[test]
    fn eraser_deletes_hit_annotations_as_one_undoable_edit() {
        let mut history = AnnotationHistory::default();
        let style = DrawStyle {
            rgba: [255, 0, 0, 255],
            radius: 2,
        };
        history.begin(1, (1, 1), style);
        history.update((20, 1));
        history.finish();
        history.begin(5, (10, 1), style);
        history.finish();
        assert!(history.commands.is_empty());
        history.undo();
        assert_eq!(history.commands.len(), 1);
    }

    #[test]
    fn mosaic_is_previewed_rendered_and_undoable() {
        let rgba = (0u8..48)
            .flat_map(|value| [value * 5, 0, 0, 255])
            .collect::<Vec<_>>();
        let base = CapturedImage::from_rgba(0, 0, 12, 4, &rgba).unwrap();
        let original = base.rgba_bytes();
        let mut history = AnnotationHistory::default();
        history.begin(
            6,
            (2, 2),
            DrawStyle {
                rgba: [0, 0, 0, 255],
                radius: 1,
            },
        );
        history.update((9, 2));
        history.finish();

        assert!(history.views().is_empty());
        assert_ne!(history.preview_base(&base).rgba_bytes(), original);
        assert_eq!(
            history.render(&base).unwrap().rgba_bytes(),
            history.preview_base(&base).rgba_bytes()
        );

        history.undo();
        assert_eq!(history.render(&base).unwrap().rgba_bytes(), original);
    }
}
