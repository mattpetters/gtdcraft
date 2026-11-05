//! The sync server's subsystems. Sync is the core and must keep answering whatever happens to the others; decisions
//! (`/v1/systemone`, forwarded to Clef) and agents (`/v1/agent/…`) each keep their own health. Requests run on
//! separate workers, a decision call has a short timeout and a cap on calls in flight, and while the background probe
//! finds decisions down they answer 503 at once instead of tying up a worker.
use crate::model::{now, Dt};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// How often the background probe checks the decision upstream.
pub const PROBE_SECS: u64 = 30;
const PROBE_TIMEOUT_SECS: u64 = 3;
/// A forwarded decision's limit (a client's own call allows longer; here a worker is held).
pub const FORWARD_SECS: u64 = 30;
/// Decision calls in flight at once: Clef answers one at a time, so more would only wait in its queue holding workers.
pub const MAX_INFLIGHT: usize = 2;
/// What a 503 tells the caller to wait before trying again.
pub const RETRY_AFTER_SECS: u64 = 30;

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Not set up on this server.
    Off,
    /// Not probed yet.
    Unknown,
    Up,
    Down(String),
}

pub struct Decisions {
    upstream: String,
    state: Mutex<(State, Option<Dt>)>,
    inflight: AtomicUsize,
}

/// Decrements the in-flight count however the call ends.
struct Slot<'a>(&'a AtomicUsize);
impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Decisions {
    pub fn new(upstream: &str) -> Self {
        let upstream = upstream.trim().to_string();
        let state = if upstream.is_empty() { State::Off } else { State::Unknown };
        Decisions { upstream, state: Mutex::new((state, None)), inflight: AtomicUsize::new(0) }
    }

    pub fn state(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).0.clone()
    }

    fn mark(&self, s: State) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = (s, Some(now()));
    }

    /// `GET <upstream>/health` (the Clef server's own health check), with a short timeout.
    pub fn probe(&self) {
        if self.upstream.is_empty() {
            return;
        }
        let base = crate::systemone::endpoint_url(&self.upstream);
        let base = base.strip_suffix("/v1/systemone").unwrap_or(&base);
        let r = ureq::get(&format!("{base}/health")).timeout(std::time::Duration::from_secs(PROBE_TIMEOUT_SECS)).call();
        self.mark(match r {
            Ok(_) => State::Up,
            Err(ureq::Error::Status(code, _)) => State::Down(format!("health check answered {code}")),
            Err(e) => State::Down(format!("unreachable: {e}")),
        });
    }

    /// `POST /v1/systemone`: forward to Clef, or say at once why not.
    pub fn forward(&self, body: &str) -> Result<String, (u16, String)> {
        match self.state() {
            State::Off => return Err((503, "decisions aren't set up on this sync server (--clef-url)".into())),
            State::Down(e) => return Err((503, format!("decisions are down ({e}); sync is unaffected"))),
            State::Unknown | State::Up => {}
        }
        if self.inflight.fetch_add(1, Ordering::SeqCst) >= MAX_INFLIGHT {
            self.inflight.fetch_sub(1, Ordering::SeqCst);
            return Err((503, "decisions are busy; try again shortly".into()));
        }
        let _slot = Slot(&self.inflight);
        let r = crate::systemone::forward(&self.upstream, body, FORWARD_SECS);
        match &r {
            Ok(_) => self.mark(State::Up),
            // the upstream couldn't be reached or timed out: stop sending it work until the probe sees it back
            Err((502, e)) => self.mark(State::Down(e.clone())),
            Err(_) => {} // the upstream answered (a 4xx about the request): it's up
        }
        r
    }

    pub fn status(&self) -> Value {
        let (s, checked) = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let (state, error) = match s {
            State::Off => ("off", String::new()),
            State::Unknown => ("unknown", String::new()),
            State::Up => ("up", String::new()),
            State::Down(e) => ("down", e),
        };
        json!({"state": state, "error": error, "checked": checked.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()), "in_flight": self.inflight.load(Ordering::SeqCst)})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_without_upstream_and_down_when_unreachable() {
        let d = Decisions::new("");
        assert_eq!(d.state(), State::Off);
        assert_eq!(d.forward("{}").unwrap_err().0, 503);
        let d = Decisions::new("http://127.0.0.1:9");
        assert_eq!(d.state(), State::Unknown);
        assert_eq!(d.forward("{}").unwrap_err().0, 502);
        assert!(matches!(d.state(), State::Down(_)));
        // tripped: the next call is refused at once, without trying the upstream
        let (code, msg) = d.forward("{}").unwrap_err();
        assert!(code == 503 && msg.contains("sync is unaffected"), "{msg}");
        d.probe();
        assert!(matches!(d.state(), State::Down(_)));
    }

    #[test]
    fn probe_brings_it_back() {
        let (url, _) = crate::systemone::tests::fake_clef();
        let d = Decisions::new(&url);
        d.mark(State::Down("was down".into()));
        d.probe();
        assert_eq!(d.state(), State::Up);
        assert!(d.forward(r#"{"model":"m","state":"x","questions":{"q":{"type":"noul"}}}"#).unwrap().contains("0.75"));
        assert_eq!(d.status()["state"], "up");
    }
}
