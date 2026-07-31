use jni::{
    Env, JavaVM, NativeMethod, jni_str, native_method,
    objects::JClass,
    sys::{JNI_ERR, JNI_VERSION_1_6, jint, jlong},
};
use std::ffi::c_void;

use crate::{RecordingState, recording_state};

const RUST_BRIDGE_NATIVE_METHODS: &[NativeMethod] = &[
    native_method! {
        static fn native_update_state(state: jint, elapsed_ms: jlong),
    },
    native_method! {
        static fn native_current_state() -> jint,
    },
    native_method! {
        static fn native_elapsed_ms() -> jlong,
    },
];

#[unsafe(no_mangle)]
pub unsafe extern "system" fn JNI_OnLoad(
    vm: *mut jni::sys::JavaVM,
    _reserved: *mut c_void,
) -> jint {
    // SAFETY: Android passes a valid process-wide JavaVM pointer to JNI_OnLoad.
    let vm = unsafe { JavaVM::from_raw(vm) };
    let registered = vm.attach_current_thread(|env| {
        let class = env.find_class(jni_str!("com/dripai/shiping/RustBridge"))?;
        // SAFETY: native_method! validates the Java signatures against the Rust ABI.
        unsafe { env.register_native_methods(class, RUST_BRIDGE_NATIVE_METHODS) }
    });

    if registered.is_ok() {
        JNI_VERSION_1_6
    } else {
        JNI_ERR
    }
}

fn native_update_state<'local>(
    _env: &mut Env<'local>,
    _class: JClass<'local>,
    state: jint,
    elapsed_ms: jlong,
) -> jni::errors::Result<()> {
    recording_state::update(
        RecordingState::from_code(state.clamp(0, u8::MAX as jint) as u8),
        elapsed_ms.max(0) as u64,
    );
    Ok(())
}

fn native_current_state<'local>(
    _env: &mut Env<'local>,
    _class: JClass<'local>,
) -> jni::errors::Result<jint> {
    Ok(recording_state::current().state.code() as jint)
}

fn native_elapsed_ms<'local>(
    _env: &mut Env<'local>,
    _class: JClass<'local>,
) -> jni::errors::Result<jlong> {
    Ok(recording_state::current().elapsed_ms.min(jlong::MAX as u64) as jlong)
}
