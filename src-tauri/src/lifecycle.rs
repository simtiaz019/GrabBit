use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseDecision {
    HideWindow,
    ExitApplication,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationSource {
    Tray,
    SecondInstance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationDecision {
    RestoreAndFocus,
}

pub const fn activation_decision(_source: ActivationSource) -> ActivationDecision {
    ActivationDecision::RestoreAndFocus
}

impl CloseDecision {
    pub const fn keeps_background_services_running(self) -> bool {
        matches!(self, Self::HideWindow)
    }
}

#[derive(Default)]
pub struct LifecycleState {
    quitting: AtomicBool,
    shutdown_started: AtomicBool,
}

impl LifecycleState {
    pub fn close_decision(&self) -> CloseDecision {
        if self.quitting.load(Ordering::SeqCst) {
            CloseDecision::ExitApplication
        } else {
            CloseDecision::HideWindow
        }
    }

    pub fn request_quit(&self) {
        self.quitting.store(true, Ordering::SeqCst);
    }

    pub fn begin_shutdown(&self) -> bool {
        !self.shutdown_started.swap(true, Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActivationDecision, ActivationSource, CloseDecision, LifecycleState, activation_decision,
    };

    #[test]
    fn close_request_hides_window() {
        assert_eq!(
            LifecycleState::default().close_decision(),
            CloseDecision::HideWindow
        );
    }

    #[test]
    fn explicit_quit_exits_application() {
        let state = LifecycleState::default();
        state.request_quit();
        assert_eq!(state.close_decision(), CloseDecision::ExitApplication);
    }

    #[test]
    fn shutdown_runs_only_once_across_exit_events() {
        let state = LifecycleState::default();
        assert!(state.begin_shutdown());
        assert!(!state.begin_shutdown());
    }

    #[test]
    fn hiding_keeps_download_engine_active() {
        assert!(CloseDecision::HideWindow.keeps_background_services_running());
    }

    #[test]
    fn hiding_keeps_browser_ipc_available() {
        let ipc_stops_only_when_process_exits =
            !CloseDecision::HideWindow.keeps_background_services_running();
        assert!(!ipc_stops_only_when_process_exits);
    }

    #[test]
    fn tray_activation_restores_and_focuses_window() {
        assert_eq!(
            activation_decision(ActivationSource::Tray),
            ActivationDecision::RestoreAndFocus
        );
    }

    #[test]
    fn second_launch_restores_existing_instance() {
        assert_eq!(
            activation_decision(ActivationSource::SecondInstance),
            ActivationDecision::RestoreAndFocus
        );
    }
}
