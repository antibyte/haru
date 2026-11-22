//! Pure transition table of the application state (Task 1.8).

use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;
use crate::ipc::AppState;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// (Re)boot requested.
    Boot,
    Downloading,
    Loading,
    /// Worker is ready.
    Loaded,
    /// Start requested (state stays as it is until `Started`).
    Start,
    Started,
    Stop,
    Stopped,
    Fail,
    WorkerLost { attempts_left: bool },
}

fn busy() -> HaruError {
    HaruError::new(ErrorCode::Busy, MsgKey::Busy)
}

pub fn transition(from: AppState, a: &Action) -> Result<AppState, HaruError> {
    use Action as A;
    use AppState as S;
    let next = match (from, a) {
        (S::Booting | S::Ready | S::Error, A::Boot) => S::Booting,
        (S::Booting, A::Downloading) => S::Downloading,
        // Loading also follows a backend switch while ready (start fallback) or talking (crash).
        (S::Booting | S::Downloading | S::Ready | S::Talking | S::Error, A::Loading) => S::Loading,
        (S::Loading, A::Loaded) => S::Ready,
        (S::Ready, A::Start) => S::Ready,
        (S::Ready, A::Started) => S::Talking,
        (S::Talking, A::Stop) => S::Talking,
        (S::Talking, A::Stopped) => S::Ready,
        (S::Booting | S::Downloading | S::Loading | S::Ready | S::Talking | S::Error, A::Fail) => S::Error,
        (S::Ready | S::Talking | S::Loading, A::WorkerLost { attempts_left: true }) => S::Loading,
        (S::Ready | S::Talking | S::Loading, A::WorkerLost { attempts_left: false }) => S::Error,
        _ => return Err(busy()),
    };
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action as A;
    use AppState as S;

    #[test]
    fn happy_path() {
        let mut s = S::Booting;
        for (a, want) in [
            (A::Downloading, S::Downloading),
            (A::Loading, S::Loading),
            (A::Loaded, S::Ready),
            (A::Start, S::Ready),
            (A::Started, S::Talking),
            (A::Stop, S::Talking),
            (A::Stopped, S::Ready),
        ] {
            s = transition(s, &a).unwrap();
            assert_eq!(s, want);
        }
    }

    #[test]
    fn boot_allowed_in_booting_ready_error_only() {
        for s in [S::Booting, S::Ready, S::Error] {
            assert_eq!(transition(s, &A::Boot).unwrap(), S::Booting);
        }
        for s in [S::Talking, S::Downloading, S::Loading] {
            assert_eq!(transition(s, &A::Boot).unwrap_err().code, ErrorCode::Busy);
        }
    }

    #[test]
    fn start_requires_ready() {
        for s in [S::Booting, S::Downloading, S::Loading, S::Talking, S::Error] {
            assert_eq!(transition(s, &A::Start).unwrap_err().code, ErrorCode::Busy);
        }
    }

    #[test]
    fn stop_only_while_talking() {
        assert_eq!(transition(S::Ready, &A::Stop).unwrap_err().code, ErrorCode::Busy);
    }

    #[test]
    fn worker_lost_switches_or_fails() {
        assert_eq!(transition(S::Talking, &A::WorkerLost { attempts_left: true }).unwrap(), S::Loading);
        assert_eq!(transition(S::Ready, &A::WorkerLost { attempts_left: false }).unwrap(), S::Error);
        assert_eq!(transition(S::Booting, &A::WorkerLost { attempts_left: true }).unwrap_err().code, ErrorCode::Busy);
    }

    #[test]
    fn fail_goes_to_error() {
        for s in [S::Booting, S::Downloading, S::Loading, S::Ready, S::Talking] {
            assert_eq!(transition(s, &A::Fail).unwrap(), S::Error);
        }
    }

    #[test]
    fn loaded_only_from_loading() {
        assert!(transition(S::Ready, &A::Loaded).is_err());
    }
}
