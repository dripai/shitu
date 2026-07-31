#[cfg(any(target_os = "android", feature = "android-jni-check", test))]
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

#[cfg(any(target_os = "android", feature = "android-jni-check", test))]
static STATE: AtomicU8 = AtomicU8::new(RecordingState::Idle as u8);
#[cfg(any(target_os = "android", feature = "android-jni-check", test))]
static ELAPSED_MS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordingState {
    #[default]
    Idle = 0,
    Authorizing = 1,
    Recording = 2,
    Finalizing = 3,
    Completed = 4,
    Failed = 5,
}

impl RecordingState {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(value: u8) -> Self {
        match value {
            1 => Self::Authorizing,
            2 => Self::Recording,
            3 => Self::Finalizing,
            4 => Self::Completed,
            5 => Self::Failed,
            _ => Self::Idle,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordingStatus {
    pub state: RecordingState,
    pub elapsed_ms: u64,
}

#[cfg(any(target_os = "android", feature = "android-jni-check", test))]
pub(crate) fn update(state: RecordingState, elapsed_ms: u64) {
    ELAPSED_MS.store(elapsed_ms, Ordering::Release);
    STATE.store(state.code(), Ordering::Release);
}

#[cfg(any(target_os = "android", feature = "android-jni-check", test))]
pub(crate) fn current() -> RecordingStatus {
    RecordingStatus {
        state: RecordingState::from_code(STATE.load(Ordering::Acquire)),
        elapsed_ms: ELAPSED_MS.load(Ordering::Acquire),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_codes_round_trip() {
        for state in [
            RecordingState::Idle,
            RecordingState::Authorizing,
            RecordingState::Recording,
            RecordingState::Finalizing,
            RecordingState::Completed,
            RecordingState::Failed,
        ] {
            assert_eq!(RecordingState::from_code(state.code()), state);
        }
    }

    #[test]
    fn unknown_state_code_is_idle() {
        assert_eq!(RecordingState::from_code(u8::MAX), RecordingState::Idle);
    }

    #[test]
    fn status_update_is_atomic_enough_for_snapshots() {
        update(RecordingState::Recording, 1_234);
        assert_eq!(
            current(),
            RecordingStatus {
                state: RecordingState::Recording,
                elapsed_ms: 1_234,
            }
        );
    }
}
