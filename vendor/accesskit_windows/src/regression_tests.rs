// ShiTu regression coverage for UIA requests that outlive a native window.
// Tests create hidden windows only; they do not automate the desktop.
use super::{context::ActionHandlerWrapper, context::Context, util::client_top_left};
use accesskit::{ActionHandler, ActionRequest, Node, NodeId, Rect, Role, TreeUpdate};
use std::sync::Arc;
use windows::{
    Win32::{Foundation::HWND, UI::Accessibility::*, UI::WindowsAndMessaging::*},
    core::{Interface, w},
};

struct HiddenWindow(HWND);

impl HiddenWindow {
    fn new() -> Self {
        Self(unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("ShiTu AccessKit regression"),
                WS_POPUP,
                120,
                140,
                100,
                80,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        })
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0) }.unwrap();
    }
}

struct NoActions;

impl ActionHandler for NoActions {
    fn do_action(&mut self, _: ActionRequest) {}
}

fn context(hwnd: HWND) -> Arc<Context> {
    let id = NodeId(1);
    let mut node = Node::new(Role::Window);
    node.set_bounds(Rect::new(0.0, 0.0, 100.0, 80.0));
    let tree = accesskit_consumer::Tree::new(
        TreeUpdate {
            nodes: vec![(id, node)],
            tree: Some(accesskit::Tree::new(id)),
            tree_id: accesskit::TreeId::ROOT,
            focus: id,
        },
        true,
    );
    Context::new(
        hwnd.into(),
        tree,
        Arc::new(ActionHandlerWrapper::new(NoActions)),
        false,
    )
}

#[test]
fn invalid_window_returns_element_not_available() {
    let error = client_top_left(HWND::default().into()).unwrap_err();
    assert_eq!(error.code().0, UIA_E_ELEMENTNOTAVAILABLE as i32);
}

#[test]
fn valid_window_provider_preserves_screen_coordinates() {
    let window = HiddenWindow::new();
    let context = context(window.0);
    let origin = context.client_top_left().unwrap();
    assert_eq!((origin.x, origin.y), (120.0, 140.0));
    let id = context.read_tree().state().root_id();
    let fragment: IRawElementProviderFragment =
        context.get_or_create_platform_node(id).into_interface();
    let bounds = unsafe { fragment.BoundingRectangle() }.unwrap();
    assert_eq!((bounds.left, bounds.top), (origin.x, origin.y));
    assert_eq!((bounds.width, bounds.height), (100.0, 80.0));
    let root: IRawElementProviderFragmentRoot = fragment.cast().unwrap();
    assert!(unsafe { root.ElementProviderFromPoint(origin.x + 1.0, origin.y + 1.0) }.is_ok());
}

#[test]
fn retained_provider_after_destroy_returns_error_across_com_boundary() {
    for _ in 0..32 {
        let window = HiddenWindow::new();
        let context = context(window.0);
        let id = context.read_tree().state().root_id();
        let fragment: IRawElementProviderFragment =
            context.get_or_create_platform_node(id).into_interface();
        let root: IRawElementProviderFragmentRoot = fragment.cast().unwrap();
        assert!(unsafe { fragment.BoundingRectangle() }.is_ok());
        drop(window);

        // Retain both context and COM providers, matching GPUI's delayed teardown.
        let errors = [
            context.client_top_left().unwrap_err(),
            unsafe { fragment.BoundingRectangle() }.unwrap_err(),
            unsafe { root.ElementProviderFromPoint(121.0, 141.0) }.unwrap_err(),
        ];
        for error in errors {
            assert_eq!(error.code().0, UIA_E_ELEMENTNOTAVAILABLE as i32);
        }
    }
}
