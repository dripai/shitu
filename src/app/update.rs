use super::*;
use crate::updater;

pub(crate) enum Event {
    Checked(bool, Result<updater::Check>),
    Progress(u8),
    Prepared(Result<updater::Launched>),
}

#[derive(Default)]
pub(super) struct UpdateState {
    checking: bool,
    installing: bool,
    info: Option<updater::Check>,
}
impl UpdateState {
    fn checked(&mut self, manual: bool, result: Result<updater::Check>) -> Option<String> {
        self.checking = false;
        match result {
            Ok(info) => {
                logging::info(format!("Latest published version: {}", info.latest));
                let status = manual.then(|| {
                    info.update.as_ref().map_or_else(
                        || i18n::text("暂无可用更新").to_owned(),
                        |release| format!("{} v{}", i18n::text("发现新版本"), release.version),
                    )
                });
                self.info = Some(info);
                status
            }
            Err(error) => {
                // Automatic failures are invisible; preserve the last success.
                logging::info(format!("Update check (manual={manual}): {error:#}"));
                manual.then(|| format!("{}: {error:#}", i18n::text("检查更新失败")))
            }
        }
    }
}

impl Panel {
    pub(super) fn check_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.update.checking || self.update.installing {
            return;
        }
        self.update.checking = true;
        if manual {
            self.status = i18n::text("正在检查更新…").to_owned();
        }
        let sender = self.shared.borrow().sender.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(updater::check)
                .unwrap_or_else(|_| Err(anyhow!("Update check worker panicked")));
            let _ = sender.send(Message::Update(Event::Checked(manual, result)));
        });
        cx.notify();
    }
    fn install_update(&mut self, cx: &mut Context<Self>) {
        if self.update.checking
            || self.update.installing
            || self.capturing
            || self.capture_due.is_some()
        {
            return;
        }
        let Some(release) = self
            .update
            .info
            .as_ref()
            .filter(|info| info.portable)
            .and_then(|info| info.update.clone())
        else {
            return;
        };
        self.update.installing = true;
        self.status = format!("{} 0%", i18n::text("下载并校验更新…"));
        let sender = self.shared.borrow().sender.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                let prepared = updater::prepare(&release, |progress| {
                    let _ = sender.send(Message::Update(Event::Progress(progress)));
                })?;
                updater::launch(prepared)
            })
            .unwrap_or_else(|_| Err(anyhow!("Update download worker panicked")));
            let _ = sender.send(Message::Update(Event::Prepared(result)));
        });
        cx.notify();
    }
    pub(super) fn update_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Checked(manual, result) => {
                if let Some(status) = self.update.checked(manual, result) {
                    self.status = status;
                }
            }
            Event::Progress(progress) => {
                self.status = format!("{} {progress}%", i18n::text("下载并校验更新…"));
            }
            Event::Prepared(result) => {
                self.update.installing = false;
                let result = result.and_then(|launched| {
                    if self.capturing || self.capture_due.is_some() {
                        return Err(anyhow!(i18n::text("请先结束截图，再重新更新。")));
                    }
                    launched.commit()
                });
                match result {
                    Ok(()) => cx.quit(),
                    Err(error) => {
                        logging::error(format!("Update: {error:#}"));
                        self.status = format!("{}: {error:#}", i18n::text("更新失败"));
                    }
                }
            }
        }
        cx.notify();
    }
    pub(super) fn load_update_notice(&mut self) {
        match updater::startup_notice() {
            Ok(Some(notice)) => {
                self.tab = 5;
                self.status = format!(
                    "{}: {}",
                    if notice.success {
                        i18n::text("更新完成")
                    } else {
                        i18n::text("更新失败")
                    },
                    notice.message
                );
            }
            Ok(None) => {}
            Err(error) => logging::error(format!("Read update result: {error:#}")),
        }
    }
    pub(super) fn update_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let release = self
            .update
            .info
            .as_ref()
            .and_then(|info| info.update.as_ref());
        let portable = self.update.info.as_ref().is_some_and(|info| info.portable);
        div()
            .h_flex()
            .flex_shrink_0()
            .ml_3()
            .gap_2()
            .child(
                Button::new("check-update")
                    .label(i18n::text("检查更新"))
                    .loading(self.update.checking)
                    .disabled(self.update.checking || self.update.installing)
                    .on_click(cx.listener(|this, _, _, cx| this.check_updates(true, cx))),
            )
            .when_some(release, |row, release| {
                row.child(format!("v{}", release.version))
            })
            .when(release.is_some() && portable, |row| {
                row.child(
                    Button::new("install-update")
                        .primary()
                        .label(i18n::text("更新并重启"))
                        .loading(self.update.installing)
                        .disabled(
                            self.update.checking
                                || self.update.installing
                                || self.capturing
                                || self.capture_due.is_some(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.install_update(cx))),
                )
            })
            .into_any_element()
    }

    pub(super) fn update_details(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let release = self
            .update
            .info
            .as_ref()
            .and_then(|info| info.update.as_ref())?;
        let portable = self.update.info.as_ref().is_some_and(|info| info.portable);
        let url = release.page_url();
        Some(
            div()
                .v_flex()
                .gap_2()
                .child(
                    div().h_flex().child(
                        Button::new("release-details")
                            .link()
                            .label(i18n::text("发行说明"))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    ),
                )
                .when(!release.notes.is_empty(), |column| {
                    column.child(
                        div()
                            .id("release-notes")
                            .max_h(px(140.))
                            .overflow_y_scrollbar()
                            .text_sm()
                            .child(release.notes.clone()),
                    )
                })
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if portable {
                            i18n::text("点击更新将关闭并重启软件，请先保存未完成的内容。")
                        } else {
                            i18n::text("请通过原安装渠道更新此版本。")
                        }),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::UpdateState;
    use crate::updater;
    use anyhow::anyhow;
    #[test]
    fn automatic_failure_is_silent_and_keeps_last_success() {
        let mut state = UpdateState {
            checking: true,
            info: Some(updater::Check {
                latest: "0.3.0".into(),
                update: None,
                portable: true,
            }),
            ..Default::default()
        };
        let status = state.checked(false, Err(anyhow!("offline")));
        assert!(!state.checking && status.is_none());
        assert_eq!(state.info.unwrap().latest, "0.3.0");
    }
    #[test]
    fn manual_failure_returns_status_and_automatic_success_is_silent() {
        let mut state = UpdateState::default();
        let status = state.checked(true, Err(anyhow!("offline")));
        assert!(status.unwrap().contains("offline"));
        let status = state.checked(
            false,
            Ok(updater::Check {
                latest: "0.3.0".into(),
                update: None,
                portable: true,
            }),
        );
        assert!(status.is_none());
    }
    #[test]
    fn manual_no_update_returns_brief_status_without_older_release() {
        let mut state = UpdateState::default();
        let status = state.checked(
            true,
            Ok(updater::Check {
                latest: "0.3.0".into(),
                update: None,
                portable: true,
            }),
        );
        assert_eq!(status.as_deref(), Some(crate::i18n::text("暂无可用更新")));
        assert!(!status.unwrap().contains("0.3.0"));
    }
}
