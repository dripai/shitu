mod selection;
use selection::PictureSelection;
mod preview;
use super::SharedApp;
use crate::{
    gallery::{self, Picture, Preferences, Sort, ViewMode},
    i18n,
    image::CapturedImage,
    platform::windows::shell,
};
use anyhow::{Result, anyhow, ensure};
use gpui_kit::component::{
    button::*,
    dialog::DialogButtonProps,
    input::{Input, InputEvent, InputState},
    list::ListItem,
    native_menu::NativeMenu,
    scroll::ScrollableElement,
    tooltip::Tooltip,
    tree::{TreeEvent, TreeItem, TreeState, tree},
    *,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use preview::PictureViewer;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

enum Job {
    Scan(u64, PathBuf),
    Move(PathBuf, PathBuf),
    Recycle(Vec<PathBuf>),
    PermanentlyDelete(Vec<PathBuf>),
    Copy(PathBuf),
    Ocr(PathBuf, crate::config::OcrConfig),
}
enum Reply {
    Scan(u64, PathBuf, Result<(Vec<PathBuf>, Vec<Picture>)>),
    Changed(Result<Option<PathBuf>>),
    Removed(bool, Result<gallery::RemovalReport>),
    Copied(Result<()>),
    Text(Result<String>),
    Opened(Result<()>),
}
enum Preview {
    Loading,
    Ready(Arc<RenderImage>, u32, u32),
    Failed(String),
}
#[derive(Clone)]
struct DragPicture(PathBuf);
struct DragLabel(String);
impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().primary)
            .child(self.0.clone())
    }
}
#[derive(Clone, PartialEq, Action)]
#[action(no_json)]
struct GalleryAction {
    path: PathBuf,
    kind: usize,
}

pub(super) struct Gallery {
    shared: SharedApp,
    prefs: Preferences,
    default_folder: PathBuf,
    current: PathBuf,
    tree: Entity<TreeState>,
    tree_paths: HashMap<String, PathBuf>,
    truncated_labels: HashSet<String>,
    directories: HashMap<PathBuf, Vec<PathBuf>>,
    expanded: HashSet<PathBuf>,
    requests: HashMap<PathBuf, u64>,
    serial: u64,
    epoch: u64,
    pictures: Vec<Picture>,
    previews: HashMap<PathBuf, Preview>,
    preview_order: VecDeque<PathBuf>,
    retired_images: Vec<Arc<RenderImage>>,
    thumb_jobs: mpsc::SyncSender<Option<(u64, PathBuf)>>,
    thumb_replies: mpsc::Receiver<ThumbnailReply>,
    thumb_epoch: Arc<AtomicU64>,
    decode_control: Arc<DecodeControl>,
    visible_paths: Vec<PathBuf>,
    viewer: Option<Entity<PictureViewer>>,
    viewer_window: Option<AnyWindowHandle>,
    preview_paths: Vec<PathBuf>,
    selection: PictureSelection,
    pending_status: Option<String>,
    search: Entity<InputState>,
    visible: Vec<usize>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    left_width: f32,
    status: String,
    loading: bool,
    busy: bool,
    jobs: mpsc::Sender<Job>,
    replies: mpsc::Receiver<Reply>,
    reply_sender: mpsc::Sender<Reply>,
    opening: bool,
    _subscriptions: Vec<Subscription>,
}

impl Gallery {
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn new(
        shared: SharedApp,
        mut prefs: Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let default_folder = shared.borrow().config.capture.save_directory.clone();
        // A changed capture directory removes the former default entry. Only
        // restore remembered locations that still belong to a gallery root.
        let current = prefs
            .current
            .clone()
            .filter(|path| {
                path.starts_with(&default_folder)
                    || prefs.folders.iter().any(|root| path.starts_with(root))
            })
            .unwrap_or_else(|| default_folder.clone());
        prefs.current = Some(current.clone());
        let (jobs, receiver) = mpsc::channel();
        let (sender, replies) = mpsc::channel();
        let reply_sender = sender.clone();
        std::thread::spawn(move || {
            while let Ok(job) = receiver.recv() {
                let reply = match job {
                    Job::Scan(id, path) => {
                        let result = gallery::scan(&path);
                        Reply::Scan(id, path, result)
                    }
                    Job::Move(source, target) => Reply::Changed(
                        gallery::move_picture(&source, &target).map(|_| Some(target)),
                    ),
                    Job::Recycle(paths) => Reply::Removed(false, gallery::recycle(&paths)),
                    Job::PermanentlyDelete(paths) => {
                        Reply::Removed(true, gallery::permanently_delete(&paths))
                    }
                    Job::Copy(path) => Reply::Copied(
                        CapturedImage::from_file(&path, 0, 0)
                            .and_then(|image| crate::capture::copy_to_clipboard(&image)),
                    ),
                    Job::Ocr(path, config) => {
                        Reply::Text(CapturedImage::from_file(&path, 0, 0).and_then(|image| {
                            crate::platform::ocr::recognize(&image, &config)
                                .map_err(|error| anyhow!(error.message()))
                        }))
                    }
                };
                if sender.send(reply).is_err() {
                    break;
                }
            }
        });
        // Separate and bound preview work so decoding cannot starve directory
        // scans or file operations. Old generations are discarded before decode.
        let (thumb_jobs, thumb_receiver) = mpsc::sync_channel(32);
        let (thumb_sender, thumb_replies) = mpsc::sync_channel(16);
        let thumb_epoch = Arc::new(AtomicU64::new(0));
        let generation = thumb_epoch.clone();
        let decode_control = Arc::new(DecodeControl::default());
        let control = decode_control.clone();
        std::thread::spawn(move || {
            thumbnail_worker(thumb_receiver, thumb_sender, generation, control)
        });
        let tree = cx.new(|cx| TreeState::new(cx));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(i18n::text("搜索文件名")));
        let mut subscriptions = vec![
            cx.observe(&tree, |this, tree, cx| {
                let path = tree
                    .read(cx)
                    .selected_item()
                    .and_then(|item| this.tree_paths.get(item.id.as_ref()))
                    .cloned();
                if let Some(path) = path
                    && path != this.current
                    && !this.busy
                {
                    this.navigate(path, cx);
                }
            }),
            cx.subscribe(&tree, |this, _, event, cx| {
                let (id, expanded) = match event {
                    TreeEvent::Expanded(id) => (id, true),
                    TreeEvent::Collapsed(id) => (id, false),
                };
                if let Some(path) = this.tree_paths.get(id.as_ref()).cloned() {
                    if expanded {
                        this.expanded.insert(path.clone());
                        if !this.directories.contains_key(&path) {
                            this.scan(path);
                        }
                    } else {
                        this.expanded.remove(&path);
                    }
                }
                cx.notify();
            }),
            cx.subscribe(&search, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.filter(cx);
                    cx.notify();
                }
            }),
        ];
        subscriptions.push(cx.on_release(|this, cx| {
            this.decode_control.preview.lock().unwrap().cancel();
            if let Some(handle) = this.viewer_window.take() {
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                });
            }
            for (_, preview) in this.previews.drain() {
                if let Preview::Ready(image, _, _) = preview {
                    cx.drop_image(image, None);
                }
            }
            for image in this.retired_images.drain(..) {
                cx.drop_image(image, None);
            }
        }));
        let mut this = Self {
            shared,
            prefs,
            default_folder,
            current,
            tree,
            tree_paths: HashMap::new(),
            truncated_labels: HashSet::new(),
            directories: HashMap::new(),
            expanded: HashSet::new(),
            requests: HashMap::new(),
            serial: 0,
            epoch: 0,
            pictures: Vec::new(),
            previews: HashMap::new(),
            preview_order: VecDeque::new(),
            retired_images: Vec::new(),
            thumb_jobs,
            thumb_replies,
            thumb_epoch,
            decode_control,
            visible_paths: Vec::new(),
            viewer: None,
            viewer_window: None,
            preview_paths: Vec::new(),
            selection: PictureSelection::default(),
            pending_status: None,
            search,
            visible: Vec::new(),
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            left_width: 220.,
            status: String::new(),
            loading: false,
            busy: false,
            jobs,
            replies,
            reply_sender,
            opening: false,
            _subscriptions: subscriptions,
        };
        this.refresh(cx);
        cx.spawn_in(window, async move |this, cx| {
            let mut polling = super::polling::Polling::default();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(60))
                    .await;
                let result =
                    cx.update(|window, cx| this.update(cx, |this, cx| this.poll(window, cx)));
                if !polling.proceed(result, "Gallery") {
                    break;
                }
            }
        })
        .detach();
        this
    }

    pub fn activate(&mut self, cx: &mut Context<Self>) {
        let path = self.shared.borrow().config.capture.save_directory.clone();
        if path != self.default_folder {
            let current = if self.current.starts_with(&path)
                || self
                    .prefs
                    .folders
                    .iter()
                    .any(|root| self.current.starts_with(root))
            {
                self.current.clone()
            } else {
                path.clone()
            };
            let mut prefs = self.prefs.clone();
            prefs.current = Some(current.clone());
            if !self.persist(prefs) {
                cx.notify();
                return;
            }
            self.current = current;
            self.default_folder = path;
            let roots = self.roots();
            self.expanded
                .retain(|path| roots.iter().any(|root| path.starts_with(root)));
            self.directories.clear();
        }
        if !self.busy {
            self.refresh(cx);
        }
    }
    fn report(&mut self, error: impl std::fmt::Display) {
        self.status = format!("{}: {error}", i18n::text("操作失败"));
    }
    fn cache_preview(&mut self, path: PathBuf, preview: Preview) {
        if !self.previews.contains_key(&path) {
            while self.previews.len() >= self.visible_paths.len().max(64) {
                let oldest = self
                    .preview_order
                    .pop_front()
                    .expect("every cached preview has an LRU entry");
                if let Some(Preview::Ready(image, _, _)) = self.previews.remove(&oldest) {
                    self.retired_images.push(image);
                }
            }
        }
        self.preview_order.retain(|entry| entry != &path);
        self.preview_order.push_back(path.clone());
        if let Some(Preview::Ready(image, _, _)) = self.previews.insert(path, preview) {
            self.retired_images.push(image);
        }
    }
    fn persist(&mut self, prefs: Preferences) -> bool {
        match prefs.save() {
            Ok(()) => {
                self.prefs = prefs;
                true
            }
            Err(error) => {
                self.report(error);
                false
            }
        }
    }
    fn roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.default_folder.clone()];
        for path in &self.prefs.folders {
            if !roots.contains(path) {
                roots.push(path.clone());
            }
        }
        roots
    }
    fn scan(&mut self, path: PathBuf) {
        self.serial += 1;
        self.requests.insert(path.clone(), self.serial);
        if let Err(e) = self.jobs.send(Job::Scan(self.serial, path)) {
            self.report(e);
        }
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.pending_status = None;
        if let Some(viewer) = &self.viewer {
            viewer.update(cx, |viewer, _| viewer.invalidate_cache());
        }
        // Restore the visible ancestor chain for the remembered current folder.
        for root in self.roots() {
            if self.current.starts_with(&root) {
                let mut ancestor = self.current.parent();
                while let Some(path) = ancestor {
                    if !path.starts_with(&root) {
                        break;
                    }
                    self.expanded.insert(path.to_owned());
                    ancestor = path.parent();
                }
            }
        }
        self.epoch += 1;
        self.thumb_epoch.store(self.epoch, Ordering::Release);
        self.visible_paths.clear();
        self.decode_control.wanted.lock().unwrap().clear();
        for (_, preview) in self.previews.drain() {
            if let Preview::Ready(image, _, _) = preview {
                self.retired_images.push(image);
            }
        }
        self.preview_order.clear();
        self.pictures.clear();
        self.visible.clear();
        self.loading = true;
        self.scan(self.current.clone());
        for path in self.expanded.clone() {
            if path != self.current {
                self.scan(path);
            }
        }
        self.rebuild_tree(cx);
        cx.notify();
    }
    fn navigate(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let mut prefs = self.prefs.clone();
        prefs.current = Some(path.clone());
        if self.persist(prefs) {
            self.current = path;
            self.selection.clear();
            self.refresh(cx);
        }
        cx.notify();
    }
    fn rebuild_tree(&mut self, cx: &mut Context<Self>) {
        fn item(this: &mut Gallery, path: &Path, root: usize, depth: usize) -> TreeItem {
            let id = format!("{root}:{}", path.display());
            this.tree_paths.insert(id.clone(), path.to_owned());
            let label = if depth == 0 {
                path.display().to_string()
            } else {
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            };
            let children = this.directories.get(path).cloned();
            let mut result =
                TreeItem::new(id.clone(), label).expanded(this.expanded.contains(path));
            if depth < 64 {
                result = if let Some(children) = children {
                    result.children(
                        children
                            .iter()
                            .map(|child| item(this, child, root, depth + 1))
                            .collect::<Vec<_>>(),
                    )
                } else {
                    result.child(
                        TreeItem::new(format!("pending:{id}"), i18n::text("加载中…"))
                            .disabled(true),
                    )
                };
            }
            result
        }
        self.tree_paths.clear();
        let roots = self.roots();
        let items: Vec<_> = roots
            .iter()
            .enumerate()
            .map(|(ix, path)| item(self, path, ix, 0))
            .collect();
        self.truncated_labels
            .retain(|id| self.tree_paths.contains_key(id));
        let selected = self
            .tree_paths
            .iter()
            .find(|(_, path)| **path == self.current)
            .map(|(id, _)| SharedString::from(id.clone()));
        self.tree.update(cx, |tree, cx| {
            tree.set_items(items, cx);
            let index = selected.and_then(|id| tree.index_of(&id));
            tree.set_selected_index(index, cx);
        });
    }
    fn ordered_paths(&self) -> Vec<PathBuf> {
        self.visible
            .iter()
            .map(|ix| self.pictures[*ix].path.clone())
            .collect()
    }
    fn selection_status(&mut self) {
        self.status = format!(
            "{} {} · {} {}",
            self.visible.len(),
            i18n::text("张图片"),
            i18n::text("已选择"),
            self.selection.paths.len()
        );
    }
    fn filter(&mut self, cx: &App) {
        // Empty search results do not render a list decoration. Cancel old
        // viewport work here too, so hidden rows cannot keep being scheduled.
        self.visible_paths.clear();
        self.decode_control.wanted.lock().unwrap().clear();
        self.previews
            .retain(|_, preview| !matches!(preview, Preview::Loading));
        self.preview_order
            .retain(|path| self.previews.contains_key(path));
        let query = self.search.read(cx).value().to_lowercase();
        self.visible = self
            .pictures
            .iter()
            .enumerate()
            .filter(|(_, pic)| pic.name.to_lowercase().contains(&query))
            .map(|(ix, _)| ix)
            .collect();
        self.selection.retain(&self.ordered_paths());
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for image in self.retired_images.drain(..) {
            cx.drop_image(image, Some(window));
        }
        // Limit UI work per tick; replies and pending decode work are bounded.
        for _ in 0..16 {
            let Ok(ThumbnailReply(id, path, result)) = self.thumb_replies.try_recv() else {
                break;
            };
            if id != self.epoch || !self.previews.contains_key(&path) {
                continue;
            }
            let preview = match result {
                Ok((image, width, height)) => Preview::Ready(
                    Arc::new(RenderImage::new(vec![image::Frame::new(image)])),
                    width,
                    height,
                ),
                Err(error) => Preview::Failed(error.to_string()),
            };
            self.cache_preview(path, preview);
            cx.notify();
        }
        let preview_result = self.decode_control.preview.lock().unwrap().result.take();
        if let Some(result) = preview_result
            && let Some(viewer) = &self.viewer
        {
            viewer.update(cx, |viewer, cx| viewer.accept(result, window, cx));
        }
        self.queue_visible();
        while let Ok(reply) = self.replies.try_recv() {
            match reply {
                Reply::Scan(id, path, result) => {
                    if self.requests.get(&path) != Some(&id) {
                        continue;
                    }
                    self.requests.remove(&path);
                    match result {
                        Ok((folders, mut pictures)) => {
                            self.directories.insert(path.clone(), folders);
                            if path == self.current {
                                gallery::sort_pictures(&mut pictures, self.prefs.sort);
                                self.pictures = pictures;
                                self.filter(cx);
                                self.loading = false;
                                self.status = self.pending_status.take().unwrap_or_else(|| {
                                    format!("{} {}", self.pictures.len(), i18n::text("张图片"))
                                });
                            }
                        }
                        Err(error) => {
                            if path == self.current {
                                self.loading = false;
                            }
                            self.report(error);
                        }
                    }
                    self.rebuild_tree(cx);
                }
                Reply::Changed(result) => {
                    self.busy = false;
                    match result {
                        Ok(path) => {
                            self.selection.clear();
                            if let Some(path) = path {
                                self.selection.single(path);
                            }
                            self.refresh(cx);
                        }
                        Err(e) => self.report(e),
                    }
                }
                Reply::Removed(permanent, result) => {
                    self.busy = false;
                    match result {
                        Ok(report) => {
                            self.selection.paths = report.remaining.iter().cloned().collect();
                            self.selection.current = report.remaining.first().cloned();
                            let label = if permanent {
                                i18n::text("已永久删除")
                            } else {
                                i18n::text("已移入回收站")
                            };
                            let mut status = format!("{label}: {}", report.removed.len());
                            if let Some(error) = report.error {
                                let names = report
                                    .remaining
                                    .iter()
                                    .take(3)
                                    .map(|p| p.file_name().unwrap_or_default().to_string_lossy())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                status = format!(
                                    "{status}; {}: {} ({names}) — {error}",
                                    i18n::text("未完成"),
                                    report.remaining.len()
                                );
                            }
                            crate::logging::info(format!(
                                "Remove (permanent={permanent}): {status}; remaining={:?}",
                                report.remaining
                            ));
                            self.refresh(cx);
                            self.status = status.clone();
                            self.pending_status = Some(status);
                        }
                        Err(error) => {
                            self.report(&error);
                            crate::logging::error(format!(
                                "Remove (permanent={permanent}): {error:#}"
                            ));
                        }
                    }
                }
                Reply::Copied(result) => {
                    self.busy = false;
                    match result {
                        Ok(()) => self.status = i18n::text("已复制").to_owned(),
                        Err(e) => self.report(e),
                    }
                }
                Reply::Text(result) => {
                    self.busy = false;
                    self.status = i18n::text("就绪").to_owned();
                    if let Err(error) = result.and_then(|text| super::open_result(text, cx)) {
                        self.report(error);
                    }
                }
                Reply::Opened(result) => {
                    self.opening = false;
                    if let Err(error) = result {
                        self.report(error);
                    }
                }
            }
            cx.notify();
        }
    }
    fn start(&mut self, job: Job, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match self.jobs.send(job) {
            Ok(()) => {
                self.busy = true;
                self.status = i18n::text("处理中…").to_owned();
            }
            Err(e) => self.report(e),
        }
        cx.notify();
    }
    fn add_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = rfd::FileDialog::new().set_parent(window).pick_folder() {
            let result = (|| -> Result<PathBuf> {
                ensure!(path.is_dir(), "{}", i18n::text("目录不存在"));
                let canonical = path.canonicalize()?;
                for root in self.roots() {
                    if root.canonicalize().is_ok_and(|p| p == canonical) {
                        return Err(anyhow!(i18n::text("目录已添加")));
                    }
                }
                Ok(path)
            })();
            match result {
                Ok(path) => {
                    let mut prefs = self.prefs.clone();
                    prefs.folders.push(path.clone());
                    prefs.current = Some(path.clone());
                    if self.persist(prefs) {
                        self.current = path;
                        self.refresh(cx);
                    }
                }
                Err(e) => self.report(e),
            }
        }
        cx.notify();
    }
    fn remove_folder(&mut self, path: &Path, cx: &mut Context<Self>) {
        let mut prefs = self.prefs.clone();
        prefs.folders.retain(|p| p != path);
        if self.current.starts_with(path) {
            prefs.current = Some(self.default_folder.clone());
        }
        if self.persist(prefs) {
            self.expanded.retain(|folder| !folder.starts_with(path));
            self.directories
                .retain(|folder, _| !folder.starts_with(path));
            self.current = self
                .prefs
                .current
                .clone()
                .unwrap_or_else(|| self.default_folder.clone());
            self.refresh(cx);
        }
        cx.notify();
    }
    fn move_to(&mut self, source: PathBuf, folder: PathBuf, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let result = (|| -> Result<PathBuf> {
            let source = source.canonicalize()?;
            let folder = folder.canonicalize()?;
            ensure!(folder.is_dir(), "{}", i18n::text("目录不存在"));
            // Unavailable unrelated roots must not block another folder's move.
            // Both endpoints still need a successfully resolved gallery root.
            let roots: Vec<_> = self
                .roots()
                .iter()
                .filter_map(|p| p.canonicalize().ok())
                .collect();
            ensure!(
                roots.iter().any(|p| source.starts_with(p))
                    && roots.iter().any(|p| folder.starts_with(p)),
                "Invalid gallery drag target"
            );
            Ok(folder.join(
                source
                    .file_name()
                    .ok_or_else(|| anyhow!("Missing filename"))?,
            ))
        })();
        match result {
            Ok(target) => {
                if source.canonicalize().is_ok_and(|source| source == target) {
                    return;
                }
                self.start(Job::Move(source, target), cx);
            }
            Err(e) => {
                self.report(e);
                cx.notify();
            }
        }
    }
    fn action(&mut self, action: &GalleryAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let path = action.path.clone();
        match action.kind {
            0 => self.open_preview(path, window, cx),
            1 | 12 => {
                if !self.opening {
                    self.opening = true;
                    let sender = self.reply_sender.clone();
                    let reveal = action.kind == 1;
                    std::thread::spawn(move || {
                        crate::logging::info("Gallery shell open started");
                        let result = shell::open_on_worker(&path, reveal);
                        crate::logging::info(format!(
                            "Gallery shell open finished: {}",
                            result.is_ok()
                        ));
                        let _ = sender.send(Reply::Opened(result));
                    });
                }
            }
            2 => self.rename(path, window, cx),
            3 => self.delete(false, window, cx),
            13 => self.delete(true, window, cx),
            4 => self.start(Job::Copy(path), cx),
            6 => self.remove_folder(&path, cx),
            7..=9 | 11 => {
                let mut prefs = self.prefs.clone();
                prefs.sort = match action.kind {
                    7 => Sort::Newest,
                    8 => Sort::Name,
                    9 => Sort::Size,
                    _ => Sort::Type,
                };
                if self.persist(prefs) {
                    gallery::sort_pictures(&mut self.pictures, self.prefs.sort);
                    self.filter(cx);
                }
            }
            10 => {
                let config = self.shared.borrow().config.ocr.clone();
                self.start(Job::Ocr(path, config), cx);
            }
            _ => {}
        }
        cx.notify();
    }
    fn open_preview(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.viewer_window.is_none() {
            let owner = cx.weak_entity();
            let display = window.display(cx);
            let display_id = display.as_ref().map(|display| display.id());
            let mut initial_size = size(px(1000.), px(720.));
            if let Some(display) = display {
                let available = display.bounds().size;
                initial_size.width = initial_size
                    .width
                    .min((available.width - px(96.)).max(px(540.)));
                initial_size.height = initial_size
                    .height
                    .min((available.height - px(96.)).max(px(360.)));
            }
            // Component Dialog is an in-window overlay without an OS caption.
            // GPUI's normal window supplies native maximize/resize/close behavior.
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(i18n::text("图片预览").into()),
                    ..Default::default()
                }),
                is_resizable: true,
                is_minimizable: true,
                display_id,
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    display_id,
                    initial_size,
                    cx,
                ))),
                window_min_size: Some(size(px(540.), px(360.))),
                ..Default::default()
            };
            match gpui_kit::open_window(options, cx, move |window, cx| {
                let close_owner = owner.clone();
                window.on_window_should_close(cx, move |window, cx| {
                    let _ = close_owner.update(cx, |gallery, cx| {
                        gallery.close_preview(window, cx);
                    });
                    true
                });
                cx.new(|cx| PictureViewer::new(owner, cx))
            }) {
                Ok((handle, viewer)) => {
                    self.viewer_window = Some(handle);
                    self.viewer = Some(viewer);
                }
                Err(error) => {
                    self.report(error);
                    return;
                }
            }
        }
        // Keep this viewing session independent of later gallery selection,
        // directory and sort changes. Opening another image replaces the snapshot.
        self.preview_paths = self.ordered_paths();
        self.load_preview(path, window, cx);
        if let (Some(handle), Some(viewer)) = (self.viewer_window, &self.viewer) {
            let focus = viewer.read(cx).focus.clone();
            if let Err(error) = handle.update(cx, |_, window, cx| {
                window.activate_window();
                focus.focus(window, cx);
            }) {
                self.close_preview(window, cx);
                self.report(error);
            }
        }
    }
    fn close_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.decode_control.preview.lock().unwrap().cancel();
        self.viewer_window = None;
        self.preview_paths.clear();
        if let Some(viewer) = self.viewer.take() {
            viewer.update(cx, |viewer, cx| viewer.close(window, cx));
        }
        cx.notify();
    }
    fn load_preview(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let index = self
            .preview_paths
            .iter()
            .position(|candidate| *candidate == path);
        self.decode_control.preview.lock().unwrap().cancel();
        if let Some(viewer) = &self.viewer {
            let hit = viewer.update(cx, |viewer, cx| {
                viewer.prepare(
                    path.clone(),
                    index.is_some_and(|ix| ix > 0),
                    index.is_some_and(|ix| ix + 1 < self.preview_paths.len()),
                    window,
                    cx,
                )
            });
            if hit {
                cx.notify();
                return;
            }
        }
        self.decode_control.preview.lock().unwrap().request(path);
        // Wake an idle worker. A full queue is already awake and will service
        // the latest preview request before its next thumbnail.
        if let Err(mpsc::TrySendError::Disconnected(_)) = self.thumb_jobs.try_send(None) {
            self.decode_control.preview.lock().unwrap().result =
                Some(Err(anyhow!("Image worker disconnected")));
        }
        cx.notify();
    }
    fn step_preview(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(viewer) = &self.viewer else {
            return;
        };
        let Some(index) = self
            .preview_paths
            .iter()
            .position(|path| path == viewer.read(cx).path())
        else {
            return;
        };
        let next = index as isize + delta;
        if next >= 0
            && let Some(path) = self.preview_paths.get(next as usize)
        {
            self.load_preview(path.clone(), window, cx);
        }
    }
    fn rename(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let input = super::input(
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            window,
            cx,
        );
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let field = input.clone();
            let input = input.clone();
            let path = path.clone();
            let weak = weak.clone();
            dialog
                .confirm()
                .title(i18n::text("重命名"))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(i18n::text("确定"))
                        .cancel_text(i18n::text("取消")),
                )
                .content(move |content, _, _| content.child(super::text_input(&field)))
                .on_ok(move |_, _, cx| {
                    let name = input.read(cx).value().to_string();
                    weak.update(cx, |this, cx| match gallery::rename_target(&path, &name) {
                        Ok(target) => {
                            this.start(Job::Move(path.clone(), target), cx);
                            true
                        }
                        Err(e) => {
                            this.report(e);
                            cx.notify();
                            false
                        }
                    })
                    .unwrap_or(true)
                })
        });
    }
    fn delete(&mut self, permanent: bool, window: &mut Window, cx: &mut Context<Self>) {
        let paths = self.selection.ordered(&self.ordered_paths());
        if paths.is_empty() {
            return;
        }
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let paths = paths.clone();
            let mut description = if paths.len() == 1 {
                paths[0]
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            } else {
                format!("{} {}", paths.len(), i18n::text("张图片"))
            };
            if permanent {
                description = format!(
                    "{}\n{description}",
                    i18n::text("将永久删除所选图片，不会移入回收站，无法从回收站恢复。")
                );
            }
            let weak = weak.clone();
            dialog
                .confirm()
                .title(if permanent {
                    i18n::text("永久删除")
                } else {
                    i18n::text("移入回收站")
                })
                .description(description)
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(i18n::text("确定"))
                        .cancel_text(i18n::text("取消")),
                )
                .on_ok(move |_, _, cx| {
                    weak.update(cx, |this, cx| {
                        if this.busy {
                            return false;
                        }
                        let job = if permanent {
                            Job::PermanentlyDelete(paths.clone())
                        } else {
                            Job::Recycle(paths.clone())
                        };
                        this.start(job, cx);
                        this.busy
                    })
                    .unwrap_or(true)
                })
        });
    }
    fn queue_visible(&mut self) {
        for path in self.visible_paths.clone() {
            if self.previews.contains_key(&path) {
                continue;
            }
            match self.thumb_jobs.try_send(Some((self.epoch, path.clone()))) {
                Ok(()) => {
                    self.cache_preview(path, Preview::Loading);
                }
                Err(mpsc::TrySendError::Full(_)) => break, // Retry on the next poll, never block UI.
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.cache_preview(
                        path,
                        Preview::Failed("Thumbnail worker disconnected".into()),
                    );
                }
            }
        }
    }
    fn card(&mut self, ix: usize, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let pic = self.pictures[ix].clone();
        if self.previews.contains_key(&pic.path) {
            self.preview_order.retain(|path| path != &pic.path);
            self.preview_order.push_back(pic.path.clone());
        }
        let details = match self.previews.get(&pic.path) {
            Some(Preview::Ready(_, w, h)) => format!("{w} × {h}"),
            Some(Preview::Failed(_)) => i18n::text("预览失败").into(),
            _ => "…".into(),
        };
        let is_list = self.prefs.view == ViewMode::List;
        let thumb = div()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .flex_shrink_0()
            .w(px(if is_list { 48. } else { width - 16. }))
            .h(px(if is_list { 40. } else { 110. }))
            .child(match self.previews.get(&pic.path) {
                Some(Preview::Ready(image, _, _)) => img(image.clone())
                    .size_full()
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
                Some(Preview::Failed(_)) => div()
                    .text_xs()
                    .child(i18n::text("预览失败"))
                    .into_any_element(),
                _ => div().text_xs().child("…").into_any_element(),
            });
        let click_path = pic.path.clone();
        let menu_path = pic.path.clone();
        let drag_path = pic.path.clone();
        let menu_owner = cx.weak_entity();
        let focus = self.focus.clone();
        let mut card = div()
            .id(("picture", ix))
            .w(px(width))
            .p_2()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(if self.selection.paths.contains(&pic.path) {
                cx.theme().primary
            } else {
                cx.theme().border
            })
            .bg(if self.selection.paths.contains(&pic.path) {
                cx.theme().accent
            } else {
                cx.theme().background
            })
            .overflow_hidden()
            .when(is_list, |d| d.h_flex().h(px(58.)))
            .when(!is_list, |d| d.v_flex().h(px(164.)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if this.busy {
                        return;
                    }
                    let ordered = this.ordered_paths();
                    this.selection.click(
                        click_path.clone(),
                        event.modifiers.control,
                        event.modifiers.shift,
                        &ordered,
                    );
                    this.selection_status();
                    if let Some(Preview::Failed(error)) = this.previews.get(&click_path) {
                        this.status = error.clone();
                    }
                    this.focus.focus(window, cx);
                    cx.notify();
                }),
            )
            .on_click(cx.listener({
                let path = pic.path.clone();
                move |this, event: &ClickEvent, window, cx| {
                    if event.click_count() == 2
                        && !event.modifiers().control
                        && !event.modifiers().shift
                    {
                        this.action(
                            &GalleryAction {
                                path: path.clone(),
                                kind: 0,
                            },
                            window,
                            cx,
                        );
                    }
                }
            }))
            .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                let Ok(count) = menu_owner.update(cx, |this, cx| {
                    if this.busy {
                        return 0;
                    }
                    this.selection.context(menu_path.clone());
                    this.selection_status();
                    cx.notify();
                    this.selection.paths.len()
                }) else {
                    return;
                };
                if count == 0 {
                    return;
                }
                focus.focus(window, cx);
                let mut menu = NativeMenu::new();
                for (kind, label) in [
                    (0, i18n::text("图片预览")),
                    (1, i18n::text("在文件夹中显示")),
                    (2, i18n::text("重命名")),
                    (3, i18n::text("移入回收站")),
                    (13, i18n::text("删除")),
                    (4, i18n::text("复制")),
                    (10, i18n::text("OCR 识别")),
                ] {
                    if count > 1 && !matches!(kind, 3 | 13) {
                        continue;
                    }
                    menu = menu.menu(
                        label,
                        Box::new(GalleryAction {
                            path: menu_path.clone(),
                            kind,
                        }),
                    );
                }
                menu.show(event.position, window, cx);
            })
            .when(!self.busy && self.selection.paths.len() <= 1, |d| {
                d.on_drag(DragPicture(drag_path), |value, _, _, cx| {
                    cx.new(|_| {
                        DragLabel(
                            value
                                .0
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                        )
                    })
                })
            })
            .child(thumb)
            .child(
                div()
                    .min_w_0()
                    .when(is_list, |d| d.flex_1())
                    .when(!is_list, |d| d.w_full().h(px(26.)).flex_shrink_0())
                    .line_height(px(24.))
                    .truncate()
                    .child(pic.name),
            );
        if is_list {
            let modified = pic
                .modified
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            card = card
                .child(div().w(px(85.)).text_xs().child(details))
                .child(
                    div()
                        .w(px(76.))
                        .text_xs()
                        .child(format!("{:.1} KB", pic.bytes as f64 / 1024.)),
                )
                .child(
                    div()
                        .w(px(130.))
                        .text_xs()
                        .child(crate::platform::clock::format_gallery_time(modified)),
                );
        }
        card.into_any_element()
    }
}

// Button's managed tooltip has a fixed delay and instant grace-period switching.
// GPUI's per-element tooltip supports 600ms on every target and owns cancellation,
// dismissal and positioning. Keep this gallery hint in the main window overlay.
fn icon_hint(id: &'static str, label: &'static str, button: Button) -> impl IntoElement {
    div()
        .id(id)
        .flex_shrink_0()
        .tooltip_show_delay(Duration::from_millis(600))
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
        .child(button.accessibility_label(label))
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.weak_entity();
        let paths = self.tree_paths.clone();
        let focus = self.focus.clone();
        let extra = self.prefs.folders.clone();
        let truncated_labels = self.truncated_labels.clone();
        let busy = self.busy;
        let folders = tree(&self.tree, move |_, entry, selected, _, cx| {
            let path = paths.get(entry.item().id.as_ref()).cloned();
            let weak = weak.clone();
            let focus = focus.clone();
            let id = entry.item().id.to_string();
            let clipped = truncated_labels.contains(&id);
            let text = StyledText::new(entry.item().label.clone());
            let layout = text.layout().clone();
            let label = entry.item().label.clone();
            let label_owner = weak.clone();
            let mut row = ListItem::new(SharedString::from(format!("folder-{}", entry.item().id)))
                .h(px(34.))
                .rounded_md()
                .text_sm()
                .pl(px(entry.depth() as f32 * 16. + 4.))
                .pr_2()
                .overflow_hidden()
                .accessibility_label(entry.item().label.clone())
                // ListItem 0.7.1 applies its selected background after custom
                // styles. Add decoration only; Tree keeps selection and input.
                .when(selected, |row| {
                    row.child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded_md()
                            .bg(cx.theme().blue.opacity(if cx.theme().is_dark() {
                                0.28
                            } else {
                                0.18
                            }))
                            .border_1()
                            .border_color(cx.theme().blue.opacity(0.7)),
                    )
                })
                // ListItem wraps its children in a block: explicitly compose
                // one flex row so the disclosure and path never split lines.
                .child(
                    div()
                        .relative()
                        .h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_2()
                        .child(
                            div()
                                .w(px(12.))
                                .flex_shrink_0()
                                .when(entry.is_folder(), |d| {
                                    d.child(
                                        Icon::new(if entry.is_expanded() {
                                            IconName::ChevronDown
                                        } else {
                                            IconName::ChevronRight
                                        })
                                        .size(px(12.))
                                        .text_color(cx.theme().muted_foreground),
                                    )
                                }),
                        )
                        .child(
                            Icon::new(if entry.is_expanded() {
                                IconName::FolderOpen
                            } else {
                                IconName::FolderClosed
                            })
                            .size(px(16.))
                            .flex_shrink_0()
                            .text_color(if selected {
                                cx.theme().blue
                            } else {
                                cx.theme().muted_foreground
                            }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .relative()
                                .truncate()
                                .child(text)
                                .child(
                                    canvas(
                                        move |_, _, cx| {
                                            // StyledText exposes the actual shaped/truncated text.
                                            // Measure after its prepaint; do not guess from path length.
                                            let is_clipped = layout.text() != label.as_ref();
                                            if is_clipped != clipped {
                                                let owner = label_owner.clone();
                                                let id = id.clone();
                                                cx.defer(move |cx| {
                                                    let _ = owner.update(cx, |this, cx| {
                                                        let changed = if is_clipped {
                                                            this.truncated_labels.insert(id)
                                                        } else {
                                                            this.truncated_labels.remove(&id)
                                                        };
                                                        if changed {
                                                            cx.notify();
                                                        }
                                                    });
                                                });
                                            }
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .size_full()
                                    .top_0()
                                    .left_0(),
                                ),
                        ),
                );
            if let Some(path) = path {
                let target = path.clone();
                let full_path = path.display().to_string();
                let remove = extra.contains(&path);
                row = row
                    .when(clipped, |row| {
                        row.tooltip_show_delay(Duration::from_millis(600)).tooltip(
                            move |window, cx| {
                                Tooltip::new(full_path.clone())
                                    .max_w(px(600.))
                                    .build(window, cx)
                            },
                        )
                    })
                    .drag_over::<DragPicture>(|style, _, _, cx| style.bg(cx.theme().accent))
                    .on_drop(move |drag: &DragPicture, _, cx| {
                        if !busy {
                            let _ = weak.update(cx, |this, cx| {
                                this.move_to(drag.0.clone(), target.clone(), cx)
                            });
                        }
                    })
                    .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                        focus.focus(window, cx);
                        let mut menu = NativeMenu::new().menu(
                            i18n::text("打开文件夹"),
                            Box::new(GalleryAction {
                                path: path.clone(),
                                kind: 12,
                            }),
                        );
                        if remove {
                            menu = menu.menu(
                                i18n::text("从图库移除"),
                                Box::new(GalleryAction {
                                    path: path.clone(),
                                    kind: 6,
                                }),
                            );
                        }
                        menu.show(event.position, window, cx);
                    });
            }
            row
        })
        .size_full();
        let left = div()
            .v_flex()
            .size_full()
            .gap_2()
            .p_2()
            .bg(cx.theme().sidebar)
            .child(
                Button::new("add-gallery-folder")
                    .small()
                    .icon(IconName::Plus)
                    .label(i18n::text("添加文件夹"))
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.add_folder(window, cx))),
            )
            .child(div().flex_1().min_h_0().child(folders));
        let available = (f32::from(window.viewport_size().width) - self.left_width - 42.).max(200.);
        let columns = if self.prefs.view == ViewMode::List {
            1
        } else {
            (available / 180.).floor().max(1.) as usize
        };
        let width = (available / columns as f32 - 8.).max(100.);
        let count = self.visible.len().div_ceil(columns);
        let list = uniform_list(
            "gallery-pictures",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|row| {
                        let indices = this
                            .visible
                            .iter()
                            .skip(row * columns)
                            .take(columns)
                            .copied()
                            .collect::<Vec<_>>();
                        div().h_flex().gap_2().pb_2().children(
                            indices
                                .into_iter()
                                .map(|ix| this.card(ix, width, cx))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .size_full()
        .with_decoration(VisiblePictures {
            gallery: cx.weak_entity(),
            columns,
        })
        .track_scroll(&self.scroll);
        let right = div()
            .v_flex()
            .size_full()
            .min_w_0()
            .gap_2()
            .p_2()
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.search).small()),
                    )
                    .children(
                        [
                            (
                                ViewMode::Thumbnails,
                                i18n::text("缩略图"),
                                IconName::LayoutDashboard,
                                "gallery-thumbnails-tip",
                            ),
                            (
                                ViewMode::List,
                                i18n::text("列表"),
                                IconName::Menu,
                                "gallery-list-tip",
                            ),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(ix, (view, label, icon, hint_id))| {
                            icon_hint(
                                hint_id,
                                label,
                                Button::new(("gallery-view", ix))
                                    .small()
                                    .ghost()
                                    .icon(icon)
                                    .selected(self.prefs.view == view)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let mut prefs = this.prefs.clone();
                                        prefs.view = view;
                                        this.persist(prefs);
                                        cx.notify();
                                    })),
                            )
                        }),
                    )
                    .child(icon_hint(
                        "gallery-sort-tip",
                        i18n::text("排序"),
                        Button::new("gallery-sort")
                            .small()
                            .ghost()
                            .icon(IconName::SortDescending)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                                this.focus.focus(window, cx);
                                let mut menu = NativeMenu::new();
                                for (kind, order, label) in [
                                    (7, Sort::Newest, i18n::text("时间")),
                                    (8, Sort::Name, i18n::text("名称")),
                                    (11, Sort::Type, i18n::text("类型")),
                                    (9, Sort::Size, i18n::text("大小")),
                                ] {
                                    menu = menu.menu_with_check(
                                        label,
                                        this.prefs.sort == order,
                                        Box::new(GalleryAction {
                                            path: this.current.clone(),
                                            kind,
                                        }),
                                    );
                                }
                                menu.show(event.position(), window, cx);
                            })),
                    ))
                    .child(icon_hint(
                        "gallery-refresh-tip",
                        i18n::text("刷新"),
                        Button::new("gallery-refresh")
                            .small()
                            .ghost()
                            .icon(IconName::RefreshCw)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    )),
            )
            .when(self.prefs.view == ViewMode::List, |right| {
                right.child(
                    div()
                        .h_flex()
                        .px_2()
                        .gap_2()
                        .text_xs()
                        .child(div().w(px(48.)))
                        .child(div().flex_1().child(i18n::text("文件名")))
                        .child(div().w(px(85.)).child(i18n::text("尺寸")))
                        .child(div().w(px(76.)).child(i18n::text("文件大小")))
                        .child(div().w(px(130.)).child(i18n::text("修改时间"))),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when(count == 0, |d| {
                        d.child(if self.loading {
                            i18n::text("加载中…")
                        } else {
                            i18n::text("暂无图片")
                        })
                    })
                    .when(count > 0, |d| {
                        d.child(list).vertical_scrollbar(&self.scroll)
                    }),
            );
        div()
            .id("gallery-focus")
            .size_full()
            .track_focus(&self.focus)
            .tab_stop(true)
            .on_action(cx.listener(Self::action))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if this.focus.is_focused(window) && !this.busy && !this.visible.is_empty() {
                    let delta = match event.keystroke.key.as_str() {
                        "left" => -1,
                        "right" => 1,
                        "up" => -(columns as isize),
                        "down" => columns as isize,
                        _ => 0,
                    };
                    if delta != 0 {
                        let current = this.visible.iter().position(|ix| {
                            Some(&this.pictures[*ix].path) == this.selection.current.as_ref()
                        });
                        let next = current.map_or(0, |ix| {
                            (ix as isize + delta).clamp(0, this.visible.len() as isize - 1) as usize
                        });
                        let path = this.pictures[this.visible[next]].path.clone();
                        let ordered = this.ordered_paths();
                        this.selection.click(
                            path,
                            event.keystroke.modifiers.control,
                            event.keystroke.modifiers.shift,
                            &ordered,
                        );
                        this.selection_status();
                        this.scroll
                            .scroll_to_item(next / columns, ScrollStrategy::Center);
                        cx.notify();
                        cx.stop_propagation();
                        return;
                    }
                }
                if this.focus.is_focused(window)
                    && !this.busy
                    && let Some(path) = this.selection.current.clone()
                {
                    let kind = match event.keystroke.key.as_str() {
                        "f2" => 2,
                        "delete" => 3,
                        "enter" => 0,
                        _ => return,
                    };
                    if kind != 3 && this.selection.paths.len() != 1 {
                        return;
                    }
                    this.action(&GalleryAction { path, kind }, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                h_resizable("gallery-split")
                    .child(
                        resizable_panel()
                            .size(px(220.))
                            .size_range(px(160.)..px(420.))
                            .child(left),
                    )
                    .child(resizable_panel().child(right))
                    .on_resize(cx.listener(|this, state: &Entity<ResizableState>, _, cx| {
                        if let Some(width) = state.read(cx).sizes().first() {
                            this.left_width = f32::from(*width);
                            cx.notify();
                        }
                    })),
            )
    }
}

struct VisiblePictures {
    gallery: WeakEntity<Gallery>,
    columns: usize,
}
impl UniformListDecoration for VisiblePictures {
    fn compute(
        &self,
        range: std::ops::Range<usize>,
        _: Bounds<Pixels>,
        _: Point<Pixels>,
        _: Pixels,
        _: usize,
        _: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        // Official visible_range excludes the extra row rendered only to measure
        // uniform_list. No image jobs are scheduled from card construction.
        let _ = self.gallery.update(cx, |gallery, _| {
            let range = picture_range(range, self.columns, gallery.visible.len());
            let paths = gallery.visible[range]
                .iter()
                .map(|ix| gallery.pictures[*ix].path.clone())
                .collect::<Vec<_>>();
            if paths != gallery.visible_paths {
                let wanted: HashSet<_> = paths.iter().cloned().collect();
                gallery.previews.retain(|path, preview| {
                    !matches!(preview, Preview::Loading) || wanted.contains(path)
                });
                gallery
                    .preview_order
                    .retain(|path| gallery.previews.contains_key(path));
                *gallery.decode_control.wanted.lock().unwrap() = wanted;
                gallery.visible_paths = paths;
            }
            gallery.queue_visible();
        });
        div().into_any_element()
    }
}
fn picture_range(
    rows: std::ops::Range<usize>,
    columns: usize,
    count: usize,
) -> std::ops::Range<usize> {
    rows.start.saturating_mul(columns).min(count)..rows.end.saturating_mul(columns).min(count)
}

type DecodedPicture = (image::RgbaImage, u32, u32);
struct ThumbnailReply(u64, PathBuf, Result<DecodedPicture>);
#[derive(Default)]
struct DecodeControl {
    wanted: Mutex<HashSet<PathBuf>>,
    preview: Mutex<PreviewWork>,
}
#[derive(Default)]
struct PreviewWork {
    version: u64,
    pending: Option<PathBuf>,
    result: Option<Result<DecodedPicture>>,
}
impl PreviewWork {
    fn cancel(&mut self) {
        self.version += 1;
        self.pending = None;
        self.result = None;
    }
    fn request(&mut self, path: PathBuf) {
        self.cancel();
        self.pending = Some(path);
    }
    fn complete(&mut self, version: u64, result: Result<DecodedPicture>) {
        if version == self.version {
            self.result = Some(result);
        }
    }
}

fn thumbnail_worker(
    jobs: mpsc::Receiver<Option<(u64, PathBuf)>>,
    replies: mpsc::SyncSender<ThumbnailReply>,
    generation: Arc<AtomicU64>,
    control: Arc<DecodeControl>,
) {
    loop {
        let request = {
            let mut preview = control.preview.lock().unwrap();
            preview.pending.take().map(|path| (preview.version, path))
        };
        if let Some((version, path)) = request {
            let result = decode_picture(&path, (2048, 1536));
            control.preview.lock().unwrap().complete(version, result);
            continue;
        }
        let Ok(job) = jobs.recv() else {
            break;
        };
        let Some((epoch, path)) = job else {
            continue;
        };
        if generation.load(Ordering::Acquire) != epoch
            || !control.wanted.lock().unwrap().contains(&path)
        {
            continue;
        }
        let result = thumbnail(&path);
        if generation.load(Ordering::Acquire) != epoch
            || !control.wanted.lock().unwrap().contains(&path)
        {
            continue;
        }
        if replies.send(ThumbnailReply(epoch, path, result)).is_err() {
            break;
        }
    }
}

fn thumbnail(path: &Path) -> Result<(image::RgbaImage, u32, u32)> {
    decode_picture(path, (320, 240))
}
fn decode_picture(path: &Path, bounds: (u32, u32)) -> Result<DecodedPicture> {
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode()?;
    let (width, height) = (image.width(), image.height());
    let mut thumb = image
        .thumbnail(bounds.0.min(width), bounds.1.min(height))
        .to_rgba8();
    // RenderImage accepts BGRA, avoiding PNG re-encoding and a second decode
    // in GPUI's global asset cache. Ownership and GPU eviction stay explicit.
    for pixel in thumb.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok((thumb, width, height))
}

#[cfg(test)]
mod tests {
    use super::{
        DecodeControl, PreviewWork, ThumbnailReply, decode_picture, picture_range, thumbnail,
        thumbnail_worker,
    };
    use anyhow::Result;
    #[test]
    fn stale_preview_queue_is_bounded_and_skipped_before_decoding() {
        use std::{
            sync::{Arc, atomic::AtomicU64, mpsc},
            time::Duration,
        };
        let (jobs, receiver) = mpsc::sync_channel(32);
        let (sender, replies) = mpsc::sync_channel(16);
        let missing = std::env::temp_dir().join("shitu-nonexistent-preview-test.png");
        for _ in 0..32 {
            jobs.try_send(Some((1, missing.clone()))).unwrap();
        }
        assert!(matches!(
            jobs.try_send(Some((1, missing.clone()))),
            Err(mpsc::TrySendError::Full(_))
        ));
        let control = Arc::new(DecodeControl::default());
        control.wanted.lock().unwrap().insert(missing.clone());
        let worker = std::thread::spawn(move || {
            thumbnail_worker(receiver, sender, Arc::new(AtomicU64::new(2)), control)
        });
        jobs.send(Some((2, missing.with_file_name("offscreen.png"))))
            .unwrap();
        jobs.send(Some((2, missing))).unwrap();
        let ThumbnailReply(epoch, _, result) =
            replies.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(epoch, 2); // All 32 stale requests were skipped, not decoded/replied.
        assert!(result.is_err()); // A failed image still produces a terminal result.
        drop(jobs);
        worker.join().unwrap();
        assert!(replies.try_recv().is_err());
    }

    #[test]
    fn visible_rows_map_only_to_the_viewport_even_in_large_folders() {
        assert_eq!(picture_range(100..105, 4, 100_000), 400..420);
        assert_eq!(picture_range(24..26, 4, 99), 96..99);
        assert_eq!(picture_range(100..105, 1, 100_000), 100..105);
    }

    #[test]
    fn preview_switch_and_close_discard_previous_results() {
        let mut preview = PreviewWork::default();
        preview.request("first.png".into());
        let first = preview.version;
        preview.request("second.png".into());
        preview.complete(first, Err(anyhow::anyhow!("stale")));
        assert!(preview.result.is_none());
        assert_eq!(
            preview.pending.as_deref(),
            Some(std::path::Path::new("second.png"))
        );
        let second = preview.version;
        preview.complete(second, Err(anyhow::anyhow!("current")));
        assert!(preview.result.is_some());
        preview.cancel();
        preview.complete(second, Err(anyhow::anyhow!("closed")));
        assert!(preview.result.is_none());
        assert!(preview.pending.is_none());
    }

    #[test]
    fn internal_preview_is_serviced_before_queued_thumbnails() {
        use std::{
            sync::{Arc, atomic::AtomicU64, mpsc},
            time::Duration,
        };
        let control = Arc::new(DecodeControl::default());
        let missing = std::env::temp_dir().join("shitu-missing-preview-priority.png");
        control.preview.lock().unwrap().request(missing.clone());
        control.wanted.lock().unwrap().insert(missing.clone());
        let (jobs, receiver) = mpsc::sync_channel(32);
        let (sender, replies) = mpsc::sync_channel(16);
        jobs.send(Some((0, missing))).unwrap();
        let worker_control = control.clone();
        let worker = std::thread::spawn(move || {
            thumbnail_worker(
                receiver,
                sender,
                Arc::new(AtomicU64::new(0)),
                worker_control,
            )
        });
        let ThumbnailReply(_, _, result) = replies.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(result.is_err());
        assert!(
            control
                .preview
                .lock()
                .unwrap()
                .result
                .as_ref()
                .unwrap()
                .is_err()
        );
        drop(jobs);
        worker.join().unwrap();
    }

    #[test]
    fn thumbnails_keep_aspect_ratio_and_invalid_images_report_errors() -> Result<()> {
        let path = std::env::temp_dir().join(format!("shitu-thumb-{}.png", std::process::id()));
        let result = (|| -> Result<()> {
            image::RgbaImage::from_pixel(800, 400, image::Rgba([10, 20, 30, 255])).save(&path)?;
            let (decoded, width, height) = thumbnail(&path)?;
            assert_eq!((width, height), (800, 400));
            assert_eq!((decoded.width(), decoded.height()), (320, 160));
            assert_eq!(decoded.get_pixel(0, 0).0, [30, 20, 10, 255]);
            let (preview, _, _) = decode_picture(&path, (2048, 1536))?;
            assert_eq!(preview.dimensions(), (800, 400)); // Do not enlarge small originals.
            image::RgbaImage::new(2400, 1200).save(&path)?;
            let (preview, width, height) = decode_picture(&path, (2048, 1536))?;
            assert_eq!((width, height), (2400, 1200));
            assert_eq!(preview.dimensions(), (2048, 1024));
            std::fs::write(&path, b"invalid image")?;
            assert!(thumbnail(&path).is_err());
            Ok(())
        })();
        std::fs::remove_file(path)?;
        result
    }
}
