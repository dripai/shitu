use super::*;
use gpui_kit::component::toolbar::Toolbar;
use std::{cell::Cell, rc::Rc};

const CACHE_COUNT: usize = 10;
const CACHE_BYTES: usize = 100_000_000;

#[derive(Clone)]
struct CachedPicture {
    image: Arc<RenderImage>,
    width: u32,
    height: u32,
}
impl CachedPicture {
    fn bytes(&self) -> usize {
        self.image.as_bytes(0).map_or(0, <[u8]>::len)
    }
}

// CPU pixels only. Leaving a picture always removes its GPU atlas entry even
// when this LRU retains the pixels for a later visit.
#[derive(Default)]
struct PictureCache {
    entries: VecDeque<(PathBuf, CachedPicture)>,
    bytes: usize,
}
impl PictureCache {
    fn get(&mut self, path: &Path) -> Option<CachedPicture> {
        let index = self.entries.iter().position(|(p, _)| p == path)?;
        let entry = self.entries.remove(index)?;
        let result = entry.1.clone();
        self.entries.push_back(entry);
        Some(result)
    }
    fn insert(&mut self, path: PathBuf, picture: CachedPicture) {
        self.insert_with_limits(path, picture, CACHE_COUNT, CACHE_BYTES);
    }
    fn insert_with_limits(
        &mut self,
        path: PathBuf,
        picture: CachedPicture,
        count: usize,
        bytes: usize,
    ) {
        if let Some(index) = self.entries.iter().position(|(p, _)| *p == path) {
            self.bytes -= self.entries.remove(index).unwrap().1.bytes();
        }
        // An individual oversized image may be displayed, but is never cached.
        if picture.bytes() <= bytes && count > 0 {
            self.bytes += picture.bytes();
            self.entries.push_back((path, picture));
        }
        while self.entries.len() > count || self.bytes > bytes {
            self.bytes -= self.entries.pop_front().unwrap().1.bytes();
        }
    }
    fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

#[derive(Clone, Copy, Default)]
struct ViewTransform {
    zoom: Option<f32>,
    pan: (f32, f32),
}
impl ViewTransform {
    fn scale(self, viewport: (f32, f32), original: (u32, u32), dpi: f32) -> f32 {
        self.zoom.unwrap_or_else(|| {
            (viewport.0 * dpi / original.0.max(1) as f32)
                .min(viewport.1 * dpi / original.1.max(1) as f32)
                .min(1.)
        })
    }
    fn geometry(
        self,
        viewport: (f32, f32),
        original: (u32, u32),
        dpi: f32,
    ) -> (f32, f32, f32, f32) {
        let scale = self.scale(viewport, original, dpi);
        let width = original.0 as f32 * scale / dpi;
        let height = original.1 as f32 * scale / dpi;
        let pan_x = self.pan.0.clamp(
            -((width - viewport.0) / 2.).max(0.),
            ((width - viewport.0) / 2.).max(0.),
        );
        let pan_y = self.pan.1.clamp(
            -((height - viewport.1) / 2.).max(0.),
            ((height - viewport.1) / 2.).max(0.),
        );
        (
            (viewport.0 - width) / 2. + pan_x,
            (viewport.1 - height) / 2. + pan_y,
            width,
            height,
        )
    }
    fn clamp_pan(&mut self, viewport: (f32, f32), original: (u32, u32), dpi: f32) {
        let (x, y, width, height) = self.geometry(viewport, original, dpi);
        self.pan = (
            x - (viewport.0 - width) / 2.,
            y - (viewport.1 - height) / 2.,
        );
    }
    fn zoom_by(
        &mut self,
        factor: f32,
        anchor: (f32, f32),
        viewport: (f32, f32),
        original: (u32, u32),
        dpi: f32,
    ) {
        let before = self.scale(viewport, original, dpi).max(0.0001);
        self.clamp_pan(viewport, original, dpi);
        let after = (before * factor).clamp(0.001, 8.);
        let ratio = after / before;
        self.pan = (
            (self.pan.0 - (anchor.0 - viewport.0 / 2.)) * ratio + anchor.0 - viewport.0 / 2.,
            (self.pan.1 - (anchor.1 - viewport.1 / 2.)) * ratio + anchor.1 - viewport.1 / 2.,
        );
        self.zoom = Some(after);
        self.clamp_pan(viewport, original, dpi);
    }
}

pub(super) struct PictureViewer {
    owner: WeakEntity<Gallery>,
    image: Option<Arc<RenderImage>>,
    base: Option<CachedPicture>,
    cache: PictureCache,
    path: PathBuf,
    name: String,
    message: String,
    previous: bool,
    next: bool,
    turns: u8,
    transform: ViewTransform,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    drag: Option<Point<Pixels>>,
    pub(super) focus: FocusHandle,
    _release: Subscription,
}
impl PictureViewer {
    pub(super) fn new(owner: WeakEntity<Gallery>, cx: &mut Context<Self>) -> Self {
        let release = cx.on_release(|viewer: &mut Self, cx| {
            if let Some(image) = viewer.image.take() {
                cx.drop_image(image, None);
            }
            viewer.base = None;
            viewer.cache.clear();
        });
        Self {
            owner,
            image: None,
            base: None,
            cache: PictureCache::default(),
            path: PathBuf::new(),
            name: String::new(),
            message: String::new(),
            previous: false,
            next: false,
            turns: 0,
            transform: ViewTransform::default(),
            bounds: Rc::new(Cell::new(Bounds::default())),
            drag: None,
            focus: cx.focus_handle(),
            _release: release,
        }
    }
    fn clear_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(image) = self.image.take() {
            cx.drop_image(image, Some(window));
        }
        self.base = None;
        self.drag = None;
    }
    pub(super) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_image(window, cx);
        self.cache.clear();
        crate::logging::info("Preview closed: pixel cache cleared");
        cx.notify();
    }
    pub(super) fn invalidate_cache(&mut self) {
        self.cache.clear();
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn prepare(
        &mut self,
        path: PathBuf,
        previous: bool,
        next: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.clear_image(window, cx);
        self.name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.path = path;
        self.previous = previous;
        self.next = next;
        self.turns = 0;
        self.transform = ViewTransform::default();
        let cached = self.cache.get(&self.path);
        let hit = cached.is_some();
        if let Some(picture) = cached {
            self.show(picture);
        } else {
            self.message = i18n::text("加载中…").into();
        }
        cx.notify();
        hit
    }
    fn show(&mut self, picture: CachedPicture) {
        self.message = format!("{} × {}", picture.width, picture.height);
        self.image = Some(picture.image.clone());
        self.base = Some(picture);
    }
    pub(super) fn accept(
        &mut self,
        result: Result<DecodedPicture>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_image(window, cx);
        match result {
            Ok((pixels, width, height)) => {
                let picture = CachedPicture {
                    image: Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])),
                    width,
                    height,
                };
                self.cache.insert(self.path.clone(), picture.clone());
                self.show(picture);
                crate::logging::info(format!(
                    "Preview cache: {} pictures, {} pixel bytes",
                    self.cache.entries.len(),
                    self.cache.bytes
                ));
            }
            Err(error) => self.message = format!("{}: {error}", i18n::text("预览失败")),
        }
        cx.notify();
    }
    fn original(&self) -> (u32, u32) {
        let Some(base) = &self.base else {
            return (1, 1);
        };
        if self.turns.is_multiple_of(2) {
            (base.width, base.height)
        } else {
            (base.height, base.width)
        }
    }
    fn zoom(
        &mut self,
        factor: f32,
        position: Option<Point<Pixels>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.image.is_none() {
            return;
        }
        let bounds = self.bounds.get();
        let viewport = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        if viewport.0 <= 0. || viewport.1 <= 0. {
            return;
        }
        let anchor = position.map_or((viewport.0 / 2., viewport.1 / 2.), |p| {
            (
                f32::from(p.x - bounds.origin.x),
                f32::from(p.y - bounds.origin.y),
            )
        });
        self.transform.zoom_by(
            factor,
            anchor,
            viewport,
            self.original(),
            window.scale_factor(),
        );
        cx.notify();
    }
    fn rotate(&mut self, delta: u8, window: &mut Window, cx: &mut Context<Self>) {
        let Some(base) = &self.base else {
            return;
        };
        self.turns = (self.turns + delta) % 4;
        let rotated = if self.turns == 0 {
            base.image.clone()
        } else {
            let dimensions = base.image.size(0);
            // Borrow cached BGRA pixels; rotate into one output buffer, without
            // duplicating the original or accumulating all four orientations.
            let source = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
                u32::from(dimensions.width),
                u32::from(dimensions.height),
                base.image.as_bytes(0).unwrap(),
            )
            .unwrap();
            let pixels = match self.turns {
                1 => image::imageops::rotate90(&source),
                2 => image::imageops::rotate180(&source),
                _ => image::imageops::rotate270(&source),
            };
            Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]))
        };
        if let Some(old) = self.image.replace(rotated) {
            cx.drop_image(old, Some(window));
        }
        self.transform = ViewTransform::default();
        self.drag = None;
        cx.notify();
    }
}

impl Render for PictureViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let measured = self.bounds.clone();
        let measure_owner = cx.weak_entity();
        let image = self.image.clone();
        let original = self.original();
        let transform = self.transform;
        let picture = canvas(
            move |bounds, _, cx| {
                if measured.replace(bounds) != bounds {
                    let owner = measure_owner.clone();
                    cx.defer(move |cx| {
                        let _ = owner.update(cx, |_, cx| cx.notify());
                    });
                }
            },
            move |bounds, _, window, _| {
                if let Some(image) = image {
                    let (x, y, width, height) = transform.geometry(
                        (f32::from(bounds.size.width), f32::from(bounds.size.height)),
                        original,
                        window.scale_factor(),
                    );
                    if let Err(error) = window.paint_image(
                        bounds,
                        Bounds::new(
                            bounds.origin + point(px(x), px(y)),
                            size(px(width), px(height)),
                        ),
                        Corners::default(),
                        image,
                        0,
                        false,
                    ) {
                        crate::logging::error(format!("Preview paint: {error}"));
                    }
                }
            },
        )
        .size_full();
        let previous = self.owner.clone();
        let next = self.owner.clone();
        let keys = self.owner.clone();
        let key_focus = self.focus.clone();
        let previous_focus = self.focus.clone();
        let next_focus = self.focus.clone();
        let disabled = self.image.is_none();
        let viewport = self.bounds.get().size;
        let ratio = self.transform.scale(
            (f32::from(viewport.width), f32::from(viewport.height)),
            original,
            window.scale_factor(),
        );
        let toolbar = Toolbar::new("picture-tools")
            .content(super::icon_hint(
                "picture-zoom-out-hint",
                i18n::text("缩小"),
                Button::new("picture-zoom-out")
                    .icon(IconName::Minus)
                    .disabled(disabled)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.zoom(1. / 1.2, None, window, cx)),
                    ),
            ))
            .content(div().w(px(60.)).text_center().text_sm().child(if disabled {
                "—".into()
            } else {
                format!("{:.0}%", ratio * 100.)
            }))
            .content(super::icon_hint(
                "picture-zoom-in-hint",
                i18n::text("放大"),
                Button::new("picture-zoom-in")
                    .icon(IconName::Plus)
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, window, cx| this.zoom(1.2, None, window, cx))),
            ))
            .content(super::icon_hint(
                "picture-fit-hint",
                i18n::text("适应窗口"),
                Button::new("picture-fit")
                    .icon(IconName::Maximize)
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.transform = ViewTransform::default();
                        cx.notify();
                    })),
            ))
            .content(super::icon_hint(
                "picture-left-hint",
                i18n::text("向左旋转 90°"),
                Button::new("picture-left")
                    .icon(
                        Icon::new(IconName::RotateCw)
                            .transform(Transformation::scale(size(-1., 1.))),
                    )
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, window, cx| this.rotate(3, window, cx))),
            ))
            .content(super::icon_hint(
                "picture-right-hint",
                i18n::text("向右旋转 90°"),
                Button::new("picture-right")
                    .icon(IconName::RotateCw)
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, window, cx| this.rotate(1, window, cx))),
            ));

        div()
            .id("picture-viewer")
            .v_flex()
            .gap_2()
            .size_full()
            .p_3()
            .bg(cx.theme().background)
            .on_key_down(move |event, window, cx| {
                if event.keystroke.key == "escape" {
                    let _ = keys.update(cx, |gallery, cx| gallery.close_preview(window, cx));
                    window.remove_window();
                    cx.stop_propagation();
                    return;
                }
                // Toolbar retains its native arrow-key focus navigation. Image
                // navigation applies only while the image surface has focus.
                if key_focus.is_focused(window) {
                    let delta = match event.keystroke.key.as_str() {
                        "left" => -1,
                        "right" => 1,
                        _ => return,
                    };
                    let _ = keys.update(cx, |gallery, cx| gallery.step_preview(delta, window, cx));
                    cx.stop_propagation();
                }
            })
            .child(
                div()
                    .flex_shrink_0()
                    .text_sm()
                    .truncate()
                    .child(self.name.clone()),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .bg(cx.theme().muted)
                    .rounded_md()
                    .child(
                        div()
                            .id("picture-surface")
                            .size_full()
                            .track_focus(&self.focus)
                            .tab_stop(true)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    this.focus.focus(window, cx);
                                    this.drag = Some(event.position);
                                }),
                            )
                            .on_mouse_move(cx.listener(
                                |this, event: &MouseMoveEvent, window, cx| {
                                    if !event.dragging() {
                                        this.drag = None;
                                        return;
                                    }
                                    if let Some(previous) = this.drag {
                                        this.drag = Some(event.position);
                                        this.transform.pan.0 +=
                                            f32::from(event.position.x - previous.x);
                                        this.transform.pan.1 +=
                                            f32::from(event.position.y - previous.y);
                                        let bounds = this.bounds.get();
                                        this.transform.clamp_pan(
                                            (
                                                f32::from(bounds.size.width),
                                                f32::from(bounds.size.height),
                                            ),
                                            this.original(),
                                            window.scale_factor(),
                                        );
                                        cx.notify();
                                    }
                                },
                            ))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| this.drag = None),
                            )
                            .on_mouse_up_out(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| this.drag = None),
                            )
                            .on_scroll_wheel(cx.listener(
                                |this, event: &ScrollWheelEvent, window, cx| {
                                    let delta = f32::from(event.delta.pixel_delta(px(16.)).y);
                                    if delta != 0. {
                                        this.zoom(
                                            1.2_f32.powf((delta / 120.).clamp(-3., 3.)),
                                            Some(event.position),
                                            window,
                                            cx,
                                        );
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .child(picture),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_2()
                            .top(relative(0.5))
                            .child(super::icon_hint(
                                "picture-previous-hint",
                                i18n::text("上一张"),
                                Button::new("picture-previous")
                                    .icon(IconName::ChevronLeft)
                                    .disabled(!self.previous)
                                    .on_click(move |_, window, cx| {
                                        let _ = previous.update(cx, |gallery, cx| {
                                            gallery.step_preview(-1, window, cx)
                                        });
                                        previous_focus.focus(window, cx);
                                    }),
                            )),
                    )
                    .child(
                        div()
                            .absolute()
                            .right_2()
                            .top(relative(0.5))
                            .child(super::icon_hint(
                                "picture-next-hint",
                                i18n::text("下一张"),
                                Button::new("picture-next")
                                    .icon(IconName::ChevronRight)
                                    .disabled(!self.next)
                                    .on_click(move |_, window, cx| {
                                        let _ = next.update(cx, |gallery, cx| {
                                            gallery.step_preview(1, window, cx)
                                        });
                                        next_focus.focus(window, cx);
                                    }),
                            )),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .h_flex()
                    .justify_center()
                    .child(toolbar),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_center()
                    .child(self.message.clone()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{CACHE_BYTES, CACHE_COUNT, CachedPicture, PictureCache, ViewTransform};
    use gpui_kit::RenderImage;
    use std::{path::Path, sync::Arc};

    fn picture(width: u32, height: u32) -> CachedPicture {
        CachedPicture {
            image: Arc::new(RenderImage::new(vec![image::Frame::new(
                image::RgbaImage::new(width, height),
            )])),
            width,
            height,
        }
    }

    #[test]
    fn cache_hits_refresh_recency_and_eviction_drops_pixels() {
        let mut cache = PictureCache::default();
        let first = picture(2, 2);
        let second = picture(2, 2);
        let first_weak = Arc::downgrade(&first.image);
        let second_weak = Arc::downgrade(&second.image);
        cache.insert_with_limits("first".into(), first, 2, 1000);
        cache.insert_with_limits("second".into(), second, 2, 1000);
        drop(cache.get(Path::new("first")));
        cache.insert_with_limits("third".into(), picture(2, 2), 2, 1000);
        assert!(first_weak.upgrade().is_some());
        assert!(second_weak.upgrade().is_none());
        assert_eq!(cache.bytes, 32);
        cache.clear();
        assert!(first_weak.upgrade().is_none());
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn repeated_cache_replacement_stays_within_both_limits_and_releases_all_pixels() {
        assert_eq!((CACHE_COUNT, CACHE_BYTES), (10, 100_000_000));
        let mut cache = PictureCache::default();
        let mut weak = Vec::new();
        for index in 0..500 {
            let image = picture(index % 12 + 1, 4);
            weak.push(Arc::downgrade(&image.image));
            cache.insert_with_limits(format!("image-{}", index % 20).into(), image, 10, 160);
            assert!(cache.bytes <= 160);
            assert!(cache.entries.len() <= 10);
            assert_eq!(
                cache.bytes,
                cache
                    .entries
                    .iter()
                    .map(|(_, image)| image.bytes())
                    .sum::<usize>()
            );
            assert_eq!(
                weak.iter().filter(|w| w.strong_count() > 0).count(),
                cache.entries.len()
            );
        }
        cache.clear();
        assert!(weak.iter().all(|w| w.upgrade().is_none()));
    }

    #[test]
    fn replacing_same_path_does_not_accumulate_entries() {
        let mut cache = PictureCache::default();
        for _ in 0..50 {
            cache.insert("same".into(), picture(4, 4));
        }
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.bytes, 64);
        cache.insert_with_limits("same".into(), picture(20, 20), 10, 100);
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn fit_geometry_respects_dpi_and_drag_cannot_lose_the_image() {
        for dpi in [1., 1.25, 1.5, 2.] {
            let mut transform = ViewTransform::default();
            let viewport = (800., 600.);
            let source = (3200, 1600);
            let (x, y, w, h) = transform.geometry(viewport, source, dpi);
            assert_eq!((x, y, w, h), (0., 100., 800., 400.));
            transform.pan = (10000., -10000.);
            transform.clamp_pan(viewport, source, dpi);
            assert_eq!(transform.pan, (0., 0.));
            transform.zoom_by(2., (400., 300.), viewport, source, dpi);
            transform.pan = (10000., -10000.);
            let (x, y, w, h) = transform.geometry(viewport, source, dpi);
            assert!(x <= 0. && y <= 0. && x + w >= viewport.0 && y + h >= viewport.1);
        }
    }

    #[test]
    fn zoom_keeps_pointer_on_same_pixel_and_has_finite_limits() {
        let viewport = (800., 600.);
        let source = (3200, 2400);
        let anchor = (300., 200.);
        let mut transform = ViewTransform {
            zoom: Some(0.5),
            pan: (0., 0.),
        };
        let (x, y, width, height) = transform.geometry(viewport, source, 1.);
        let pixel = ((anchor.0 - x) / width, (anchor.1 - y) / height);
        transform.zoom_by(1.2, anchor, viewport, source, 1.);
        let (x, y, width, height) = transform.geometry(viewport, source, 1.);
        assert!(((anchor.0 - x) / width - pixel.0).abs() < 0.00001);
        assert!(((anchor.1 - y) / height - pixel.1).abs() < 0.00001);
        for _ in 0..100 {
            transform.zoom_by(1.2, anchor, viewport, source, 1.);
        }
        assert_eq!(transform.zoom, Some(8.));
        for _ in 0..100 {
            transform.zoom_by(0.5, anchor, viewport, source, 1.);
        }
        assert_eq!(transform.zoom, Some(0.001));
    }
}
