use anyhow::Result;

use crate::image::{
    CapturedImage, DrawStyle, OutlineShape, TextFont, arrow_head, arrow_polylines,
    outline_polylines, stroke_polylines,
};

#[derive(Default)]
pub struct AnnotationHistory {
    commands: Vec<AnnotationCommand>,
    undo: Vec<Vec<AnnotationCommand>>,
    redo: Vec<Vec<AnnotationCommand>>,
    active: bool,
    active_tool: i32,
    eraser: Option<((u32, u32), f64)>,
    active_before: Option<Vec<AnnotationCommand>>,
    selected: Option<usize>,
    edit: Option<EditDrag>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationBounds {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

impl AnnotationBounds {
    fn from_points(start: (u32, u32), end: (u32, u32)) -> Self {
        Self {
            left: start.0.min(end.0),
            top: start.1.min(end.1),
            width: start.0.abs_diff(end.0),
            height: start.1.abs_diff(end.1),
        }
    }

    fn right(self) -> u32 {
        self.left.saturating_add(self.width)
    }

    fn bottom(self) -> u32 {
        self.top.saturating_add(self.height)
    }

    fn expanded(self, padding: u32) -> Self {
        Self::from_points(
            (
                self.left.saturating_sub(padding),
                self.top.saturating_sub(padding),
            ),
            (
                self.right().saturating_add(padding),
                self.bottom().saturating_add(padding),
            ),
        )
    }

    fn corner(self, handle: i32) -> (u32, u32) {
        (
            if handle % 2 == 0 {
                self.left
            } else {
                self.right()
            },
            if handle < 2 { self.top } else { self.bottom() },
        )
    }
}

struct EditDrag {
    index: usize,
    original: AnnotationCommand,
    point: (u32, u32),
    handle: i32,
}

impl AnnotationHistory {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.commands.clear();
        self.undo.clear();
        self.redo.clear();
        self.active = false;
        self.active_tool = 0;
        self.eraser = None;
        self.active_before = None;
        self.selected = None;
        self.edit = None;
    }

    pub fn begin(&mut self, tool: i32, point: (u32, u32), style: DrawStyle) {
        self.finish();
        if tool == 8 {
            self.selected = self
                .commands
                .iter()
                .rposition(|command| command.selection_hit_test(point));
            self.begin_edit(point, -1);
            return;
        }
        self.clear_selection();
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
            7 => AnnotationCommand::Ellipse {
                start: point,
                end: point,
                style,
            },
            5 => {
                self.active = true;
                self.active_tool = tool;
                let radius = style.radius.max(1) as f64;
                self.eraser = Some((point, radius));
                self.erase_at(point, radius);
                return;
            }
            6 => {
                AnnotationCommand::Mosaic {
                    points: vec![point],
                    radius: style.radius.max(1) as u32,
                    // Brush coverage and pixelation strength are independent.
                    block_size: 10,
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

    pub fn update(&mut self, point: (u32, u32), constrained: bool, bounds: (u32, u32)) {
        if !self.active {
            return;
        }
        let point = clamp_point(point, bounds);
        if let Some(edit) = &self.edit {
            let mut command = edit.original.clone();
            let original_bounds = command.bounds();
            let delta = (
                point.0 as i64 - edit.point.0 as i64,
                point.1 as i64 - edit.point.1 as i64,
            );
            if delta == (0, 0) {
                self.commands[edit.index] = command;
                return;
            }
            if edit.handle == -1 {
                command.translate(
                    clamp_translation(
                        delta.0,
                        original_bounds.left,
                        original_bounds.right(),
                        bounds.0,
                    ),
                    clamp_translation(
                        delta.1,
                        original_bounds.top,
                        original_bounds.bottom(),
                        bounds.1,
                    ),
                );
            } else {
                let anchor = original_bounds.corner(3 - edit.handle);
                let corner = original_bounds.corner(edit.handle);
                let corner = clamp_point(
                    (
                        shift_coordinate(corner.0, delta.0),
                        shift_coordinate(corner.1, delta.1),
                    ),
                    bounds,
                );
                let corner = if constrained && command.is_outline() {
                    constrained_endpoint(anchor, corner, bounds)
                } else {
                    corner
                };
                command.resize_to(AnnotationBounds::from_points(anchor, corner), edit.handle);
            }
            command.fit_within(bounds);
            self.commands[edit.index] = command;
            return;
        }
        if self.active_tool == 5 {
            if let Some((previous, radius)) = self.eraser {
                // Cover the path between mouse samples at the selected size.
                let steps = (distance(previous, point) / (radius / 2.).max(1.)).ceil() as u32;
                for step in 1..=steps.max(1) {
                    let t = step as f64 / steps.max(1) as f64;
                    let sample = (
                        (previous.0 as f64 + (point.0 as f64 - previous.0 as f64) * t).round()
                            as u32,
                        (previous.1 as f64 + (point.1 as f64 - previous.1 as f64) * t).round()
                            as u32,
                    );
                    self.erase_at(sample, radius);
                }
                self.eraser = Some((point, radius));
            }
            return;
        }
        match self.commands.last_mut() {
            Some(AnnotationCommand::Pen { points, .. })
            | Some(AnnotationCommand::Mosaic { points, .. }) => {
                if points.last().copied() != Some(point) {
                    points.push(point);
                }
            }
            Some(AnnotationCommand::Rectangle { start, end, .. })
            | Some(AnnotationCommand::Ellipse { start, end, .. }) => {
                *end = if constrained {
                    constrained_endpoint(*start, point, bounds)
                } else {
                    point
                };
            }
            Some(AnnotationCommand::Arrow { end, .. }) => {
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
        self.eraser = None;
        self.edit = None;
    }

    pub fn selection_bounds(&self) -> Option<AnnotationBounds> {
        self.selected
            .and_then(|index| self.commands.get(index))
            .map(AnnotationCommand::bounds)
    }

    pub fn clear_selection(&mut self) {
        self.finish();
        self.selected = None;
    }

    pub fn delete_selected(&mut self) -> bool {
        // Deletion is a separate edit after the pointer gesture has finished.
        if self.active {
            return false;
        }
        let Some(index) = self
            .selected
            .take()
            .filter(|index| *index < self.commands.len())
        else {
            return false;
        };
        self.undo.push(self.commands.clone());
        self.redo.clear();
        self.commands.remove(index);
        true
    }

    pub fn begin_edit(&mut self, point: (u32, u32), handle: i32) {
        self.finish();
        if !(-1..=3).contains(&handle) {
            return;
        }
        let Some(index) = self.selected.filter(|index| *index < self.commands.len()) else {
            self.selected = None;
            return;
        };
        self.active_before = Some(self.commands.clone());
        self.edit = Some(EditDrag {
            index,
            original: self.commands[index].clone(),
            point,
            handle,
        });
        self.active = true;
        self.active_tool = 8;
    }

    /// Cancel a drawing, erasing, or editing gesture without recording an undo step.
    pub fn cancel_edit(&mut self) {
        if let Some(before) = self.active_before.take() {
            self.commands = before;
        }
        self.active = false;
        self.active_tool = 0;
        self.eraser = None;
        self.edit = None;
        if self
            .selected
            .is_some_and(|index| index >= self.commands.len())
        {
            self.selected = None;
        }
    }

    pub fn add_text(
        &mut self,
        position: (u32, u32),
        text: &str,
        style: DrawStyle,
        font_size: u32,
        font: TextFont,
    ) {
        self.finish();
        self.selected = None;
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
            font,
        });
    }

    pub fn undo(&mut self) {
        self.finish();
        self.selected = None;
        if let Some(previous) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.commands, previous));
        }
    }

    pub fn redo(&mut self) {
        self.finish();
        self.selected = None;
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.commands, next));
        }
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
    Ellipse {
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
        font: TextFont,
    },
    Mosaic {
        points: Vec<(u32, u32)>,
        radius: u32,
        block_size: u32,
    },
}

impl AnnotationCommand {
    fn is_outline(&self) -> bool {
        matches!(self, Self::Rectangle { .. } | Self::Ellipse { .. })
    }

    fn geometry_bounds(&self) -> AnnotationBounds {
        match self {
            Self::Pen { points, .. } | Self::Mosaic { points, .. } => points_bounds(points),
            Self::Rectangle { start, end, .. } | Self::Ellipse { start, end, .. } => {
                AnnotationBounds::from_points(*start, *end)
            }
            Self::Arrow { start, end, .. } => {
                let mut points = vec![*start, *end];
                if let Some((left, right)) = arrow_head(*start, *end) {
                    points.extend([left, right]);
                }
                points_bounds(&points)
            }
            Self::Text {
                position,
                text,
                font_size,
                ..
            } => {
                let (width, height) = estimated_text_size(text, *font_size);
                AnnotationBounds {
                    left: position.0,
                    top: position.1,
                    width: width.ceil() as u32,
                    height: height.ceil() as u32,
                }
            }
        }
    }

    fn padding(&self) -> u32 {
        match self {
            Self::Pen { style, .. }
            | Self::Rectangle { style, .. }
            | Self::Ellipse { style, .. }
            | Self::Arrow { style, .. } => style.radius.max(1) as u32,
            Self::Mosaic { radius, .. } => *radius,
            Self::Text { .. } => 0,
        }
    }

    fn bounds(&self) -> AnnotationBounds {
        self.geometry_bounds().expanded(self.padding())
    }

    fn selection_hit_test(&self, point: (u32, u32)) -> bool {
        match self {
            Self::Rectangle { .. } => point_in_bounds(point, self.bounds(), 4),
            Self::Ellipse { .. } => {
                let bounds = self.geometry_bounds();
                let radius_x = bounds.width as f64 / 2.0;
                let radius_y = bounds.height as f64 / 2.0;
                if radius_x == 0.0 || radius_y == 0.0 {
                    return self.hit_test(point, 4.0);
                }
                let tolerance = self.padding() as f64 + 4.0;
                let x = (point.0 as f64 - bounds.left as f64 - radius_x) / (radius_x + tolerance);
                let y = (point.1 as f64 - bounds.top as f64 - radius_y) / (radius_y + tolerance);
                x * x + y * y <= 1.0
            }
            _ => self.hit_test(point, 4.0),
        }
    }

    fn translate(&mut self, dx: i64, dy: i64) {
        let translate = |point: &mut (u32, u32)| {
            point.0 = shift_coordinate(point.0, dx);
            point.1 = shift_coordinate(point.1, dy);
        };
        match self {
            Self::Pen { points, .. } | Self::Mosaic { points, .. } => {
                for point in points {
                    translate(point);
                }
            }
            Self::Rectangle { start, end, .. }
            | Self::Ellipse { start, end, .. }
            | Self::Arrow { start, end, .. } => {
                translate(start);
                translate(end);
            }
            Self::Text { position, .. } => translate(position),
        }
    }

    fn resize_to(&mut self, target: AnnotationBounds, handle: i32) {
        let source = self.bounds();
        let geometry = self.geometry_bounds();
        let scale = (target.width as f64 / source.width.max(1) as f64)
            .min(target.height as f64 / source.height.max(1) as f64);
        match self {
            Self::Text {
                position,
                text,
                font_size,
                ..
            } => {
                *font_size = (*font_size as f64 * scale).floor().max(1.0) as u32;
                let (width, height) = estimated_text_size(text, *font_size);
                // Text preserves its aspect ratio; the opposite corner stays fixed.
                position.0 = if handle % 2 == 0 {
                    target.right().saturating_sub(width.ceil() as u32)
                } else {
                    target.left
                };
                position.1 = if handle < 2 {
                    target.bottom().saturating_sub(height.ceil() as u32)
                } else {
                    target.top
                };
            }
            Self::Pen { points, style } => {
                style.radius = (style.radius as f64 * scale).round().max(1.0) as i32;
                let destination = inset_bounds(target, style.radius as u32);
                for point in points {
                    *point = map_point(*point, geometry, destination);
                }
            }
            Self::Mosaic {
                points,
                radius,
                block_size,
            } => {
                *radius = (*radius as f64 * scale).round().max(1.0) as u32;
                *block_size = (*block_size as f64 * scale).round().max(1.0) as u32;
                let destination = inset_bounds(target, *radius);
                for point in points {
                    *point = map_point(*point, geometry, destination);
                }
            }
            Self::Rectangle { start, end, style }
            | Self::Ellipse { start, end, style }
            | Self::Arrow { start, end, style } => {
                let destination = inset_bounds(target, style.radius.max(1) as u32);
                let original_start = *start;
                let original_end = *end;
                *start = map_point(original_start, geometry, destination);
                *end = map_point(original_end, geometry, destination);
                // A zero-length axis cannot be expanded by multiplication alone.
                if geometry.width == 0 {
                    start.0 = destination.left;
                    end.0 = destination.right();
                }
                if geometry.height == 0 {
                    start.1 = destination.top;
                    end.1 = destination.bottom();
                }
            }
        }
    }

    fn fit_within(&mut self, image: (u32, u32)) {
        let max_x = image.0.saturating_sub(1);
        let max_y = image.1.saturating_sub(1);
        let bounds = self.bounds();
        if bounds.width > max_x || bounds.height > max_y {
            self.resize_to(
                AnnotationBounds {
                    left: 0,
                    top: 0,
                    width: max_x,
                    height: max_y,
                },
                3,
            );
        }
        // Arrow heads are derived from endpoints, so check their final extent as well.
        let bounds = self.bounds();
        self.translate(
            -(bounds.right().saturating_sub(max_x) as i64),
            -(bounds.bottom().saturating_sub(max_y) as i64),
        );
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
                image.draw_stroke(points, *style);
            }
            Self::Rectangle { start, end, style } => {
                image.draw_rectangle(*start, *end, *style);
            }
            Self::Ellipse { start, end, style } => {
                image.draw_ellipse(*start, *end, *style);
            }
            Self::Arrow { start, end, style } => {
                image.draw_arrow(*start, *end, *style);
            }
            Self::Text {
                position,
                text,
                style,
                font_size,
                font,
            } => image.draw_text(*position, text, *font_size, font, style.rgba)?,
            Self::Mosaic { .. } => self.render_mosaic(image),
        }
        Ok(())
    }

    fn hit_test(&self, point: (u32, u32), tolerance: f64) -> bool {
        match self {
            Self::Pen { points, style } => {
                let tolerance = tolerance + style.radius.max(1) as f64;
                paths_hit_test(&stroke_polylines(points, *style), point, tolerance)
            }
            Self::Rectangle { start, end, style } => {
                let tolerance = tolerance + style.radius.max(1) as f64;
                outline_hit_test(
                    OutlineShape::Rectangle,
                    *start,
                    *end,
                    *style,
                    point,
                    tolerance,
                )
            }
            Self::Ellipse { start, end, style } => {
                let tolerance = tolerance + style.radius.max(1) as f64;
                outline_hit_test(
                    OutlineShape::Ellipse,
                    *start,
                    *end,
                    *style,
                    point,
                    tolerance,
                )
            }
            Self::Arrow { start, end, style } => {
                let tolerance = tolerance + style.radius.max(1) as f64;
                paths_hit_test(&arrow_polylines(*start, *end, *style), point, tolerance)
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

fn clamp_point(point: (u32, u32), bounds: (u32, u32)) -> (u32, u32) {
    (
        point.0.min(bounds.0.saturating_sub(1)),
        point.1.min(bounds.1.saturating_sub(1)),
    )
}

fn constrained_endpoint(start: (u32, u32), point: (u32, u32), bounds: (u32, u32)) -> (u32, u32) {
    let start = clamp_point(start, bounds);
    let positive_x = point.0 >= start.0;
    let positive_y = point.1 >= start.1;
    let available_x = if positive_x {
        bounds.0.saturating_sub(1).saturating_sub(start.0)
    } else {
        start.0
    };
    let available_y = if positive_y {
        bounds.1.saturating_sub(1).saturating_sub(start.1)
    } else {
        start.1
    };
    let side = start
        .0
        .abs_diff(point.0)
        .max(start.1.abs_diff(point.1))
        .min(available_x)
        .min(available_y);
    (
        if positive_x {
            start.0 + side
        } else {
            start.0 - side
        },
        if positive_y {
            start.1 + side
        } else {
            start.1 - side
        },
    )
}

fn shift_coordinate(value: u32, delta: i64) -> u32 {
    (value as i64 + delta).clamp(0, u32::MAX as i64) as u32
}

fn clamp_translation(delta: i64, start: u32, end: u32, size: u32) -> i64 {
    let minimum = -(start as i64);
    let maximum = size.saturating_sub(1) as i64 - end as i64;
    if minimum > maximum {
        return minimum;
    }
    delta.clamp(minimum, maximum)
}

fn points_bounds(points: &[(u32, u32)]) -> AnnotationBounds {
    let Some(first) = points.first() else {
        return AnnotationBounds {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
        };
    };
    let (mut left, mut top, mut right, mut bottom) = (first.0, first.1, first.0, first.1);
    for point in &points[1..] {
        left = left.min(point.0);
        top = top.min(point.1);
        right = right.max(point.0);
        bottom = bottom.max(point.1);
    }
    AnnotationBounds::from_points((left, top), (right, bottom))
}

fn point_in_bounds(point: (u32, u32), bounds: AnnotationBounds, tolerance: u32) -> bool {
    point.0 >= bounds.left.saturating_sub(tolerance)
        && point.0 <= bounds.right().saturating_add(tolerance)
        && point.1 >= bounds.top.saturating_sub(tolerance)
        && point.1 <= bounds.bottom().saturating_add(tolerance)
}

fn inset_bounds(bounds: AnnotationBounds, padding: u32) -> AnnotationBounds {
    let x_padding = padding.min(bounds.width / 2);
    let y_padding = padding.min(bounds.height / 2);
    AnnotationBounds {
        left: bounds.left + x_padding,
        top: bounds.top + y_padding,
        width: bounds.width - x_padding * 2,
        height: bounds.height - y_padding * 2,
    }
}

fn map_point(point: (u32, u32), source: AnnotationBounds, target: AnnotationBounds) -> (u32, u32) {
    let map = |value: u32, origin: u32, size: u32, destination: u32, destination_size: u32| {
        let proportion = if size == 0 {
            0.5
        } else {
            (value as f64 - origin as f64) / size as f64
        };
        (destination as f64 + proportion * destination_size as f64)
            .round()
            .clamp(0.0, u32::MAX as f64) as u32
    };
    (
        map(
            point.0,
            source.left,
            source.width,
            target.left,
            target.width,
        ),
        map(
            point.1,
            source.top,
            source.height,
            target.top,
            target.height,
        ),
    )
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

fn outline_hit_test(
    shape: OutlineShape,
    start: (u32, u32),
    end: (u32, u32),
    style: DrawStyle,
    point: (u32, u32),
    tolerance: f64,
) -> bool {
    paths_hit_test(
        &outline_polylines(shape, start, end, style),
        point,
        tolerance,
    )
}

fn paths_hit_test(paths: &[Vec<(u32, u32)>], point: (u32, u32), tolerance: f64) -> bool {
    paths.iter().any(|path| {
        path.windows(2)
            .any(|pair| distance_to_segment(point, pair[0], pair[1]) <= tolerance)
            || path
                .first()
                .is_some_and(|candidate| distance(*candidate, point) <= tolerance)
    })
}

#[cfg(test)]
mod tests {
    use super::{AnnotationCommand, AnnotationHistory, DrawStyle};
    use crate::image::CapturedImage;

    fn style() -> DrawStyle {
        DrawStyle {
            rgba: [255, 0, 0, 255],
            radius: 2,
            dashed: false,
        }
    }

    fn rectangle(history: &mut AnnotationHistory, start: (u32, u32), end: (u32, u32)) {
        history.begin(2, start, style());
        history.update(end, false, (200, 200));
        history.finish();
    }

    #[test]
    fn shift_constrains_rectangles_and_ellipses_in_all_four_directions() {
        for tool in [2, 7] {
            for (point, expected) in [
                ((80, 65), (80, 80)),
                ((20, 65), (20, 80)),
                ((80, 35), (80, 20)),
                ((20, 35), (20, 20)),
            ] {
                let mut history = AnnotationHistory::default();
                history.begin(tool, (50, 50), style());
                history.update(point, true, (100, 100));
                let bounds = history.commands[0].geometry_bounds();
                assert_eq!(bounds.width, 30);
                assert_eq!(bounds.height, 30);
                match history.commands[0] {
                    AnnotationCommand::Rectangle { end, .. }
                    | AnnotationCommand::Ellipse { end, .. } => assert_eq!(end, expected),
                    _ => panic!("expected an outline"),
                }
            }
        }
    }

    #[test]
    fn shift_clamps_both_axes_at_image_edges_without_losing_equal_sides() {
        for (start, point, expected) in [
            ((95, 10), (99, 40), (99, 14)),
            ((5, 90), (0, 60), (0, 85)),
            ((10, 95), (40, 99), (14, 99)),
            ((90, 5), (60, 0), (85, 0)),
        ] {
            let mut history = AnnotationHistory::default();
            history.begin(2, start, style());
            history.update(point, true, (100, 100));
            match history.commands[0] {
                AnnotationCommand::Rectangle { end, .. } => assert_eq!(end, expected),
                _ => unreachable!(),
            }
            let bounds = history.commands[0].geometry_bounds();
            assert_eq!(bounds.width, bounds.height);
        }
    }

    #[test]
    fn selection_hits_shape_interiors_and_edits_only_the_topmost_annotation() {
        let mut history = AnnotationHistory::default();
        rectangle(&mut history, (10, 10), (50, 50));
        rectangle(&mut history, (20, 20), (60, 60));
        let before = history.commands.clone();
        history.begin(8, (30, 30), style());
        assert_eq!(history.selected, Some(1));
        history.update((35, 40), false, (100, 100));
        history.finish();
        assert!(history.commands[0] == before[0]);
        assert!(history.commands[1] != before[1]);
        let edited = history.commands.clone();
        history.undo();
        assert!(history.commands == before);
        assert_eq!(history.selection_bounds(), None);
        history.redo();
        assert!(history.commands == edited);
        assert_eq!(history.selection_bounds(), None);
    }

    #[test]
    fn deleting_selected_annotation_preserves_other_objects_and_undo_redo() {
        let mut history = AnnotationHistory::default();
        rectangle(&mut history, (10, 10), (50, 50));
        rectangle(&mut history, (20, 20), (60, 60));
        let before = history.commands.clone();
        let undo_steps = history.undo.len();
        history.begin(8, (30, 30), style());
        history.finish();

        assert!(history.delete_selected());
        assert!(history.commands == before[..1]);
        assert_eq!(history.selection_bounds(), None);
        assert_eq!(history.undo.len(), undo_steps + 1);
        assert!(!history.delete_selected());
        assert_eq!(history.undo.len(), undo_steps + 1);

        history.undo();
        assert!(history.commands == before);
        assert!(
            !history.delete_selected(),
            "an empty selection must preserve redo"
        );
        history.redo();
        assert!(history.commands == before[..1]);

        history.undo();
        history.begin(8, (10, 10), style());
        history.finish();
        assert_eq!(history.selected, Some(0));
        assert!(history.delete_selected());
        assert!(history.commands == before[1..]);
        history.redo();
        assert!(
            history.commands == before[1..],
            "new deletion must discard the old redo branch"
        );
    }

    #[test]
    fn deletion_does_not_interrupt_an_active_annotation_edit() {
        let mut history = AnnotationHistory::default();
        rectangle(&mut history, (10, 10), (50, 50));
        let before = history.commands.clone();
        let undo_steps = history.undo.len();
        history.begin(8, (30, 30), style());
        history.update((35, 40), false, (100, 100));
        let pending = history.commands.clone();

        assert!(!history.delete_selected());
        assert!(history.commands == pending);
        assert!(history.selection_bounds().is_some());
        assert_eq!(history.undo.len(), undo_steps);

        history.finish();
        history.undo();
        assert!(history.commands == before);
    }

    #[test]
    fn move_clamps_visible_stroke_to_image_and_records_one_undo_step() {
        let mut history = AnnotationHistory::default();
        rectangle(&mut history, (10, 10), (30, 30));
        let before = history.commands.clone();
        history.begin(8, (20, 20), style());
        history.update((0, 0), false, (100, 100));
        let bounds = history.selection_bounds().unwrap();
        assert_eq!((bounds.left, bounds.top), (0, 0));
        history.update((200, 200), false, (100, 100));
        let bounds = history.selection_bounds().unwrap();
        assert_eq!((bounds.right(), bounds.bottom()), (99, 99));
        history.finish();
        assert_eq!(history.undo.len(), 2, "creation plus one complete drag");
        history.undo();
        assert!(history.commands == before);
    }

    #[test]
    fn each_resize_handle_keeps_its_opposite_corner_and_is_undoable() {
        for handle in 0..4 {
            let mut history = AnnotationHistory::default();
            rectangle(&mut history, (20, 20), (60, 40));
            let before = history.commands.clone();
            history.begin(8, (30, 30), style());
            history.finish();
            let original = history.selection_bounds().unwrap();
            let corner = original.corner(handle);
            let next = (
                if handle % 2 == 0 {
                    corner.0 - 10
                } else {
                    corner.0 + 10
                },
                if handle < 2 {
                    corner.1 - 10
                } else {
                    corner.1 + 10
                },
            );
            history.begin_edit(corner, handle);
            history.update(next, false, (100, 100));
            history.finish();
            assert_eq!(
                history.selection_bounds().unwrap().corner(3 - handle),
                original.corner(3 - handle)
            );
            let edited = history.commands.clone();
            assert!(edited != before);
            history.undo();
            assert!(history.commands == before);
            history.redo();
            assert!(history.commands == edited);
        }
    }

    #[test]
    fn selection_and_returning_to_drag_origin_do_not_add_undo_steps() {
        let mut history = AnnotationHistory::default();
        rectangle(&mut history, (10, 10), (50, 50));
        let before = history.commands.clone();
        history.begin(8, (30, 30), style());
        history.finish();
        assert_eq!(history.undo.len(), 1);
        let corner = history.selection_bounds().unwrap().corner(3);
        history.begin_edit(corner, 3);
        history.update(corner, false, (100, 100));
        history.finish();
        assert_eq!(history.undo.len(), 1);
        history.begin_edit((30, 30), -1);
        history.update((40, 40), false, (100, 100));
        history.update((30, 30), false, (100, 100));
        history.finish();
        assert!(history.commands == before);
        assert_eq!(history.undo.len(), 1);
    }

    #[test]
    fn cancellation_rolls_back_drawing_erasing_and_editing_gestures() {
        let mut history = AnnotationHistory::default();
        rectangle(&mut history, (10, 10), (50, 50));
        let before = history.commands.clone();
        history.begin(7, (20, 20), style());
        history.update((80, 80), false, (100, 100));
        history.cancel_edit();
        assert!(history.commands == before);
        history.begin(5, (30, 10), style());
        assert!(history.commands.is_empty());
        history.cancel_edit();
        assert!(history.commands == before);
        history.begin(8, (30, 30), style());
        history.update((40, 40), false, (100, 100));
        history.cancel_edit();
        assert!(history.commands == before);
        assert!(history.selection_bounds().is_some());
        assert_eq!(history.undo.len(), 1);
        history.clear_selection();
        assert_eq!(history.selection_bounds(), None);
    }

    #[test]
    fn all_annotation_kinds_resize_and_text_uses_a_scaled_font_size() {
        for command in [
            AnnotationCommand::Pen {
                points: vec![(20, 20), (40, 40)],
                style: style(),
            },
            AnnotationCommand::Arrow {
                start: (20, 20),
                end: (40, 40),
                style: style(),
            },
            AnnotationCommand::Ellipse {
                start: (20, 20),
                end: (40, 40),
                style: style(),
            },
            AnnotationCommand::Text {
                position: (20, 20),
                text: "Test".into(),
                style: style(),
                font_size: 12,
                font: Default::default(),
            },
            AnnotationCommand::Mosaic {
                points: vec![(20, 20), (40, 40)],
                radius: 5,
                block_size: 6,
            },
        ] {
            let mut history = AnnotationHistory::default();
            history.commands.push(command.clone());
            history.selected = Some(0);
            let corner = history.selection_bounds().unwrap().corner(3);
            history.begin_edit(corner, 3);
            history.update((corner.0 + 20, corner.1 + 20), false, (100, 100));
            history.finish();
            assert!(history.commands[0] != command);
            let bounds = history.selection_bounds().unwrap();
            assert!(bounds.right() < 100 && bounds.bottom() < 100);
            if let AnnotationCommand::Text { font_size, .. } = history.commands[0] {
                assert!(font_size > 12);
            }
            let edited = history.commands.clone();
            history.undo();
            assert!(history.commands[0] == command);
            history.redo();
            assert!(history.commands == edited);
        }
    }

    #[test]
    fn edge_annotations_are_unchanged_when_an_edit_has_no_pointer_movement() {
        for command in [
            AnnotationCommand::Rectangle {
                start: (0, 0),
                end: (30, 20),
                style: style(),
            },
            AnnotationCommand::Ellipse {
                start: (30, 20),
                end: (0, 0),
                style: style(),
            },
            AnnotationCommand::Arrow {
                start: (0, 0),
                end: (30, 0),
                style: style(),
            },
            AnnotationCommand::Pen {
                points: vec![(0, 0)],
                style: style(),
            },
            AnnotationCommand::Text {
                position: (0, 0),
                text: "Test".into(),
                style: style(),
                font_size: 12,
                font: Default::default(),
            },
            AnnotationCommand::Mosaic {
                points: vec![(0, 0)],
                radius: 5,
                block_size: 6,
            },
        ] {
            for handle in -1..4 {
                let mut history = AnnotationHistory::default();
                history.commands.push(command.clone());
                history.selected = Some(0);
                let point = if handle < 0 {
                    (0, 0)
                } else {
                    command.bounds().corner(handle)
                };
                history.begin_edit(point, handle);
                history.update(point, false, (100, 100));
                history.finish();
                assert!(history.commands[0] == command);
                assert!(history.undo.is_empty());
            }
        }
    }

    #[test]
    fn resizing_can_expand_point_and_line_outlines_without_changing_drag_direction() {
        for command in [
            AnnotationCommand::Rectangle {
                start: (20, 20),
                end: (20, 20),
                style: style(),
            },
            AnnotationCommand::Ellipse {
                start: (20, 20),
                end: (40, 20),
                style: style(),
            },
            AnnotationCommand::Rectangle {
                start: (40, 40),
                end: (20, 20),
                style: style(),
            },
        ] {
            let mut history = AnnotationHistory::default();
            let reversed = match command {
                AnnotationCommand::Rectangle { start, end, .. } => start.0 > end.0,
                _ => false,
            };
            history.commands.push(command);
            history.selected = Some(0);
            let corner = history.selection_bounds().unwrap().corner(3);
            history.begin_edit(corner, 3);
            history.update((corner.0 + 20, corner.1 + 20), false, (100, 100));
            let geometry = history.commands[0].geometry_bounds();
            assert!(geometry.width > 0 && geometry.height > 0);
            if reversed && let AnnotationCommand::Rectangle { start, end, .. } = history.commands[0]
            {
                assert!(start.0 > end.0 && start.1 > end.1);
            }
        }
    }

    #[test]
    fn crossing_a_resize_anchor_is_stable_and_returning_restores_original_geometry() {
        let mut history = AnnotationHistory::default();
        rectangle(&mut history, (20, 20), (40, 40));
        let before = history.commands.clone();
        history.begin(8, (30, 30), style());
        history.finish();
        let corner = history.selection_bounds().unwrap().corner(3);
        history.begin_edit(corner, 3);
        history.update((10, 10), true, (100, 100));
        let first = history.commands.clone();
        history.update((10, 10), true, (100, 100));
        assert!(history.commands == first);
        let bounds = history.selection_bounds().unwrap();
        assert_eq!(bounds.width, bounds.height);
        history.update(corner, false, (100, 100));
        history.finish();
        assert!(history.commands == before);
        assert_eq!(history.undo.len(), 1);
    }

    #[test]
    fn undo_and_redo_preserve_commands() {
        let mut history = AnnotationHistory::default();
        history.begin(
            2,
            (10, 20),
            DrawStyle {
                rgba: [255, 0, 0, 255],
                radius: 2,
                dashed: false,
            },
        );
        history.update((30, 40), false, (100, 100));
        history.finish();
        assert_eq!(history.commands.len(), 1);
        history.undo();
        assert!(history.commands.is_empty());
        history.redo();
        assert_eq!(history.commands.len(), 1);
    }

    #[test]
    fn text_annotations_keep_independent_font_choices_through_undo_and_resize() {
        let mut history = AnnotationHistory::default();
        let chosen = crate::image::TextFont {
            weight: 700,
            italic: true,
            ..Default::default()
        };
        history.add_text((10, 10), "One", style(), 20, chosen.clone());
        history.add_text((10, 50), "Two", style(), 12, Default::default());
        history.undo();
        assert!(
            matches!(&history.commands[0], AnnotationCommand::Text { font, font_size: 20, .. } if *font == chosen)
        );
        history.redo();
        assert!(
            matches!(&history.commands[1], AnnotationCommand::Text { font, .. } if *font == Default::default())
        );
        history.commands[0].resize_to(
            super::AnnotationBounds {
                left: 10,
                top: 10,
                width: 120,
                height: 70,
            },
            3,
        );
        assert!(
            matches!(&history.commands[0], AnnotationCommand::Text { font, .. } if *font == chosen)
        );
    }

    #[test]
    fn pen_and_arrow_dash_gaps_match_erasure_and_survive_undo() {
        let base = CapturedImage::from_rgba(0, 0, 100, 64, &[0; 100 * 64 * 4]).unwrap();
        for tool in [1, 3] {
            let mut history = AnnotationHistory::default();
            history.begin(
                tool,
                (8, 20),
                DrawStyle {
                    radius: 2,
                    dashed: true,
                    ..style()
                },
            );
            history.update((80, 20), false, (100, 64));
            history.finish();
            let drawn = history.render(&base).unwrap().rgba_bytes();
            assert_eq!(&drawn[(20 * 100 + 24) * 4..(20 * 100 + 25) * 4], &[0; 4]);
            history.begin(
                5,
                (24, 20),
                DrawStyle {
                    radius: 1,
                    ..style()
                },
            );
            history.finish();
            assert_eq!(
                history.commands.len(),
                1,
                "eraser in a gap must not hit tool {tool}"
            );
            history.begin(
                5,
                (12, 20),
                DrawStyle {
                    radius: 1,
                    ..style()
                },
            );
            history.finish();
            assert!(history.commands.is_empty());
            history.undo();
            assert_eq!(history.render(&base).unwrap().rgba_bytes(), drawn);
            history.redo();
            assert!(history.commands.is_empty());
        }
    }

    #[test]
    fn preview_and_output_preserve_dash_gaps() {
        let base = CapturedImage::from_rgba(0, 0, 64, 48, &[0; 64 * 48 * 4]).unwrap();
        let mut history = AnnotationHistory::default();
        history.begin(
            2,
            (8, 8),
            DrawStyle {
                rgba: [255, 0, 0, 255],
                radius: 1,
                dashed: true,
            },
        );
        history.update((60, 36), false, (64, 48));
        history.finish();

        let output = history.render(&base).unwrap().rgba_bytes();
        let offset = |x: usize, y: usize| (y * 64 + x) * 4;
        assert_eq!(&output[offset(11, 8)..offset(11, 8) + 4], &[255, 0, 0, 255]);
        assert_eq!(&output[offset(16, 8)..offset(16, 8) + 4], &[0; 4]);
    }

    #[test]
    fn ellipse_styles_survive_undo_redo_and_outline_erasure() {
        let base = CapturedImage::from_rgba(0, 0, 64, 64, &[0; 64 * 64 * 4]).unwrap();
        let mut history = AnnotationHistory::default();
        let style = DrawStyle {
            rgba: [0, 128, 255, 255],
            radius: 1,
            dashed: true,
        };
        history.begin(7, (56, 48), style);
        history.update((8, 8), false, (64, 64));
        history.finish();
        assert!(matches!(
            history.commands[0],
            AnnotationCommand::Ellipse { .. }
        ));
        let pixels = history.render(&base).unwrap().rgba_bytes();

        history.undo();
        assert!(history.commands.is_empty());
        assert_eq!(
            history.render(&base).unwrap().rgba_bytes(),
            base.rgba_bytes()
        );
        history.redo();
        assert_eq!(history.render(&base).unwrap().rgba_bytes(), pixels);

        history.begin(5, (32, 28), style);
        history.finish();
        assert_eq!(
            history.commands.len(),
            1,
            "ellipse center is not its outline"
        );
        history.begin(5, (32, 8), style);
        history.finish();
        assert!(history.commands.is_empty());
        history.undo();
        assert_eq!(history.render(&base).unwrap().rgba_bytes(), pixels);
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
                dashed: false,
            },
        );
        history.update((2, 2), false, (4, 4));
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
            dashed: false,
        };
        history.begin(1, (1, 1), style);
        history.update((20, 1), false, (100, 100));
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
                dashed: false,
            },
        );
        history.update((9, 2), false, (12, 4));
        history.finish();

        assert_ne!(history.preview_base(&base).rgba_bytes(), original);
        assert_eq!(
            history.render(&base).unwrap().rgba_bytes(),
            history.preview_base(&base).rgba_bytes()
        );

        history.undo();
        assert_eq!(history.render(&base).unwrap().rgba_bytes(), original);
    }

    #[test]
    fn eraser_keeps_selected_radius_through_drag_and_covers_between_events() {
        for (radius, remaining) in [(2, 1), (20, 0)] {
            let mut history = AnnotationHistory::default();
            let pen = DrawStyle {
                radius: 1,
                ..style()
            };
            history.begin(1, (50, 50), pen);
            history.finish();
            // The line passes 15px away; endpoints are both far from the mark.
            history.begin(5, (10, 65), DrawStyle { radius, ..pen });
            history.update((90, 65), false, (100, 100));
            history.finish();
            assert_eq!(history.commands.len(), remaining);
            if remaining == 0 {
                history.undo();
                assert_eq!(history.commands.len(), 1);
                history.redo();
                assert!(history.commands.is_empty());
            }
        }
    }

    #[test]
    fn mosaic_brush_size_changes_coverage_without_changing_grain() {
        let rgba = (0..128 * 128)
            .flat_map(|i| [(i % 251) as u8, 0, 0, 255])
            .collect::<Vec<_>>();
        let base = CapturedImage::from_rgba(0, 0, 128, 128, &rgba).unwrap();
        let mut coverage = Vec::new();
        for radius in [2, 18, 32, 64] {
            let mut history = AnnotationHistory::default();
            history.begin(6, (64, 64), DrawStyle { radius, ..style() });
            history.finish();
            assert!(
                matches!(history.commands[0], AnnotationCommand::Mosaic { radius: r, block_size: 10, .. } if r == radius as u32)
            );
            let output = history.render(&base).unwrap().rgba_bytes();
            let mut changed = 0;
            for (index, (before, after)) in
                rgba.chunks_exact(4).zip(output.chunks_exact(4)).enumerate()
            {
                if before != after {
                    changed += 1;
                    let dx = index as i32 % 128 - 64;
                    let dy = index as i32 / 128 - 64;
                    assert!(dx * dx + dy * dy <= radius * radius);
                }
            }
            coverage.push(changed);
        }
        assert!(coverage.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
