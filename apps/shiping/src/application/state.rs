use std::path::PathBuf;

use crate::{config::Config, platform::target::RecordingTarget};

use super::{RecorderHandle, RecordingOptions};

/// 录制业务的唯一可变状态；Slint 组件和平台窗口对象不进入此结构。
pub(crate) struct ApplicationState {
    pub(crate) config: Config,
    pub(crate) target: Option<RecordingTarget>,
    pub(crate) recorder: Option<RecorderHandle>,
    pub(crate) pending_options: Option<RecordingOptions>,
    pub(crate) countdown_token: u64,
    pub(crate) last_output: Option<PathBuf>,
}

impl ApplicationState {
    pub(crate) fn new(config: Config) -> Self {
        Self {
            config,
            target: None,
            recorder: None,
            pending_options: None,
            countdown_token: 0,
            last_output: None,
        }
    }
}
