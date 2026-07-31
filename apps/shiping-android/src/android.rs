use std::time::Duration;

use jni::{
    Env, JavaVM, NativeMethod, jni_sig, jni_str, native_method, objects::JObject, refs::Global,
    sys::jboolean,
};
use slint::{ComponentHandle, Timer, TimerMode};

use crate::{
    MobileWindow,
    permission::{self, PermissionState},
};

const MAIN_ACTIVITY_NATIVE_METHODS: &[NativeMethod] = &[native_method! {
    fn native_on_screen_capture_permission_result(granted: jboolean),
}];

#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    if let Err(error) = run(app) {
        eprintln!("ShiPing Android failed to start: {error}");
    }
}

fn run(app: slint::android::AndroidApp) -> Result<(), Box<dyn std::error::Error>> {
    slint::android::init(app.clone())?;
    register_main_activity_native_methods(&app)?;
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

fn register_main_activity_native_methods(
    app: &slint::android::AndroidApp,
) -> jni::errors::Result<()> {
    // SAFETY: AndroidApp owns the process-wide JavaVM pointer for the lifetime of this call.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw_activity = app.activity_as_ptr() as jni::sys::jobject;

    vm.attach_current_thread(|env| {
        // SAFETY: AndroidApp exposes an unowned global Activity reference. The Cast wrapper
        // prevents this function from deleting it and does not outlive AndroidApp.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        let class = env.get_object_class(&activity)?;
        // SAFETY: native_method! verifies the Java signature and Rust ABI at compile time.
        unsafe { env.register_native_methods(class, MAIN_ACTIVITY_NATIVE_METHODS) }
    })
}

fn native_on_screen_capture_permission_result<'local>(
    _env: &mut Env<'local>,
    _activity: JObject<'local>,
    granted: jboolean,
) -> jni::errors::Result<()> {
    permission::set(if granted {
        PermissionState::Granted
    } else {
        PermissionState::Denied
    });
    Ok(())
}
