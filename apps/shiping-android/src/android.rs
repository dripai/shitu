use std::time::Duration;

use jni::{
    EnvUnowned, JavaVM, errors::LogErrorAndDefault, jni_sig, jni_str, objects::JObject,
    refs::Global, sys::jboolean,
};
use slint::{ComponentHandle, Timer, TimerMode};

use crate::{
    MobileWindow,
    permission::{self, PermissionState},
};

#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    if let Err(error) = run(app) {
        eprintln!("ShiPing Android failed to start: {error}");
    }
}

fn run(app: slint::android::AndroidApp) -> Result<(), Box<dyn std::error::Error>> {
    slint::android::init(app.clone())?;
    permission::set(PermissionState::Idle);

    let window = MobileWindow::new()?;
    apply_permission_state(&window, permission::current());

    let permission_app = app.clone();
    window.on_request_screen_capture(move || {
        permission::set(PermissionState::Requested);
        if let Err(error) = request_screen_capture_permission(&permission_app) {
            eprintln!("Failed to request Android screen capture permission: {error}");
            permission::set(PermissionState::Error);
        }
    });

    let window_weak = window.as_weak();
    let timer = Timer::default();
    let mut last_state = permission::current();
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        let state = permission::current();
        if state == last_state {
            return;
        }
        last_state = state;
        if let Some(window) = window_weak.upgrade() {
            apply_permission_state(&window, state);
        }
    });

    window.run()?;
    Ok(())
}

fn apply_permission_state(window: &MobileWindow, state: PermissionState) {
    window.set_permission_status(state.message().into());
    window.set_permission_granted(state == PermissionState::Granted);
}

fn request_screen_capture_permission(app: &slint::android::AndroidApp) -> jni::errors::Result<()> {
    // SAFETY: AndroidApp owns the process-wide JavaVM pointer for the lifetime of this call.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw_activity = app.activity_as_ptr() as jni::sys::jobject;

    vm.attach_current_thread(|env| {
        // SAFETY: AndroidApp exposes an unowned global Activity reference. The Cast wrapper
        // prevents this function from deleting it and does not outlive AndroidApp.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        env.call_method(
            activity,
            jni_str!("requestScreenCapturePermission"),
            jni_sig!("()V"),
            &[],
        )?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_dripai_shiping_MainActivity_nativeOnScreenCapturePermissionResult<
    'caller,
>(
    mut unowned_env: EnvUnowned<'caller>,
    _activity: JObject<'caller>,
    granted: jboolean,
) {
    unowned_env
        .with_env(|_env| -> jni::errors::Result<()> {
            permission::set(if granted != 0 {
                PermissionState::Granted
            } else {
                PermissionState::Denied
            });
            Ok(())
        })
        .resolve::<LogErrorAndDefault>();
}
