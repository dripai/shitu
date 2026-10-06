use std::{cell::RefCell, rc::Rc};

use slint::ComponentHandle;

use super::{AppController, AppTheme, CaptureTray, MainWindow};

pub(super) fn bind(main: &MainWindow, tray: &CaptureTray, state: Rc<RefCell<AppController>>) {
    tray.global::<AppTheme>().set_mode(main.get_theme_mode());
    synchronize(main.get_theme_mode(), &state.borrow());
    let tray = tray.as_weak();
    main.on_theme_changed(move |mode| {
        if let Some(tray) = tray.upgrade() {
            tray.global::<AppTheme>().set_mode(mode);
        }
        synchronize(mode, &state.borrow());
    });
}

fn synchronize(mode: i32, state: &AppController) {
    // Slint globals are per top-level component, not process-wide singletons.
    // Copy the preview mode; each Palette still handles live system changes.
    if let Some(result) = state.ocr_result.upgrade() {
        result.global::<AppTheme>().set_mode(mode);
    }
    if let Some(session) = &state.session {
        session.set_theme_mode(mode);
    }
    state.pins.borrow().set_theme_mode(mode);
}
