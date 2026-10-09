use crate::{config::Config, hotkey::HotkeyState, i18n, platform::windows::startup};
use anyhow::{Result, anyhow};

pub fn apply_transaction(old: &Config, candidate: &Config, hotkey: &mut HotkeyState) -> Result<()> {
    startup::set_enabled(candidate.launch_at_startup, candidate.start_minimized)
        .map_err(|error| anyhow!("{}: {error}", i18n::text("开机启动设置失败")))?;

    if let Err(error) = hotkey.set_binding(candidate.hotkey.as_deref()) {
        let rollback = startup::set_enabled(old.launch_at_startup, old.start_minimized).err();
        return Err(with_rollback(
            format!("{}: {}", i18n::text("快捷键设置失败"), error.message()),
            rollback.map(|error| format!("{}: {error}", i18n::text("恢复开机启动失败"))),
        ));
    }

    if let Err(error) = candidate.save() {
        let mut rollback_errors = Vec::new();
        if let Err(error) = startup::set_enabled(old.launch_at_startup, old.start_minimized) {
            rollback_errors.push(format!("{}: {error}", i18n::text("恢复开机启动失败")));
        }
        if let Err(error) = hotkey.set_binding(old.hotkey.as_deref()) {
            rollback_errors.push(format!(
                "{}: {}",
                i18n::text("恢复快捷键失败"),
                error.message()
            ));
        }
        let rollback = (!rollback_errors.is_empty()).then(|| rollback_errors.join("；"));
        return Err(with_rollback(
            format!("{}: {error}", i18n::text("配置保存失败")),
            rollback,
        ));
    }

    Ok(())
}

fn with_rollback(message: String, rollback: Option<String>) -> anyhow::Error {
    match rollback {
        Some(rollback) => anyhow!("{message}{}{rollback}", i18n::text("；回滚失败：")),
        None => anyhow!(message),
    }
}

#[cfg(test)]
mod tests {
    use super::with_rollback;

    #[test]
    fn rollback_error_keeps_both_failures() {
        let error = with_rollback("apply failed".into(), Some("restore failed".into())).to_string();
        assert!(error.starts_with("apply failed"));
        assert!(error.ends_with("restore failed"));
        assert_eq!(
            with_rollback("apply failed".into(), None).to_string(),
            "apply failed"
        );
    }
}
