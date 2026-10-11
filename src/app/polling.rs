//! GPUI 0.3.8 AsyncApp::update_window uses try_borrow_mut. Windows shell/COM
//! message pumping can re-enter a timer while App is borrowed. This temporary
//! conflict must not permanently stop hotkeys, editor commands or gallery work.
use anyhow::Result;

#[derive(Default)]
pub(super) struct Polling {
    waiting: bool,
}

impl Polling {
    pub fn proceed(&mut self, result: Result<Result<()>>, name: &str) -> bool {
        match result {
            Ok(Ok(())) => {
                if self.waiting {
                    crate::logging::info(format!("{name} polling resumed"));
                }
                self.waiting = false;
                true
            }
            Err(error) if error.is::<std::cell::BorrowMutError>() => {
                if !self.waiting {
                    crate::logging::info(format!("{name} polling deferred: {error}"));
                }
                self.waiting = true;
                true
            }
            Ok(Err(_)) => false, // The owning entity was released.
            Err(error) => {
                crate::logging::info(format!("{name} polling ended: {error:#}"));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reentrant_borrow_does_not_end_polling_but_released_owner_does() {
        let cell = std::cell::RefCell::new(());
        let borrowed = cell.borrow_mut();
        let error = cell.try_borrow_mut().unwrap_err();
        let mut state = Polling::default();
        assert!(state.proceed(Err(error.into()), "test"));
        drop(borrowed);
        assert!(state.proceed(Ok(Ok(())), "test"));
        assert!(!state.proceed(Ok(Err(anyhow::anyhow!("released"))), "test"));
        assert!(!state.proceed(Err(anyhow::anyhow!("window not found")), "test"));
    }
}
