mod recording_state;

#[cfg(any(target_os = "android", feature = "android-jni-check"))]
mod android;

pub use recording_state::{RecordingState, RecordingStatus};
