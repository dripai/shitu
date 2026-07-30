#[cfg(target_os = "android")]
use std::sync::atomic::{AtomicU8, Ordering};

#[cfg(any(target_os = "android", test))]
const IDLE: u8 = 0;
#[cfg(any(target_os = "android", test))]
const REQUESTED: u8 = 1;
#[cfg(any(target_os = "android", test))]
const GRANTED: u8 = 2;
#[cfg(any(target_os = "android", test))]
const DENIED: u8 = 3;
#[cfg(any(target_os = "android", test))]
const ERROR: u8 = 4;

#[cfg(target_os = "android")]
static CURRENT: AtomicU8 = AtomicU8::new(IDLE);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PermissionState {
    #[default]
    Idle,
    Requested,
    Granted,
    Denied,
    Error,
}

impl PermissionState {
    pub fn message(self) -> &'static str {
        match self {
            Self::Idle => "尚未申请系统录屏权限",
            Self::Requested => "正在等待系统录屏授权",
            Self::Granted => "系统录屏权限已授权",
            Self::Denied => "系统录屏权限被拒绝",
            Self::Error => "无法启动系统录屏授权",
        }
    }

    #[cfg(any(target_os = "android", test))]
    const fn code(self) -> u8 {
        match self {
            Self::Idle => IDLE,
            Self::Requested => REQUESTED,
            Self::Granted => GRANTED,
            Self::Denied => DENIED,
            Self::Error => ERROR,
        }
    }

    #[cfg(any(target_os = "android", test))]
    const fn from_code(value: u8) -> Self {
        match value {
            REQUESTED => Self::Requested,
            GRANTED => Self::Granted,
            DENIED => Self::Denied,
            ERROR => Self::Error,
            _ => Self::Idle,
        }
    }
}

#[cfg(target_os = "android")]
pub(crate) fn current() -> PermissionState {
    PermissionState::from_code(CURRENT.load(Ordering::Acquire))
}

#[cfg(target_os = "android")]
pub(crate) fn set(state: PermissionState) {
    CURRENT.store(state.code(), Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_codes_round_trip() {
        for state in [
            PermissionState::Idle,
            PermissionState::Requested,
            PermissionState::Granted,
            PermissionState::Denied,
            PermissionState::Error,
        ] {
            assert_eq!(PermissionState::from_code(state.code()), state);
        }
    }

    #[test]
    fn unknown_permission_code_is_idle() {
        assert_eq!(PermissionState::from_code(u8::MAX), PermissionState::Idle);
    }
}
