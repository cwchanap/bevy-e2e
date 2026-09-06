use std::{
    thread,
    time::{Duration, Instant},
};

use serde_json::json;

use crate::{Error, Result, game::Game};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const DIAGNOSTICS_METHOD: &str = "brp_extras/get_diagnostics";

impl Game {
    /// Poll until `id` resolves to exactly one entity (or timeout / ambiguity).
    pub fn wait_for(&self, id: &str) -> Result<()> {
        let timeout = self.operation_timeout();
        let deadline = Instant::now() + timeout;
        loop {
            match self.exists(id) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(Error::AmbiguousSelector(_)) => {
                    return Err(Error::AmbiguousSelector(id.to_owned()));
                }
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout {
                    operation: format!("wait_for({id})"),
                    timeout,
                });
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Poll until `id` matches zero entities (ambiguity is still an error).
    pub fn wait_for_gone(&self, id: &str) -> Result<()> {
        let timeout = self.operation_timeout();
        let deadline = Instant::now() + timeout;
        loop {
            match self.exists(id) {
                Ok(false) => return Ok(()),
                Ok(true) => {}
                Err(Error::AmbiguousSelector(_)) => {
                    return Err(Error::AmbiguousSelector(id.to_owned()));
                }
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout {
                    operation: format!("wait_for_gone({id})"),
                    timeout,
                });
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// Advance until diagnostics `frame_count` grows by at least `frames`.
    ///
    /// If the initial count is null, polls within the operation timeout until a
    /// numeric baseline is available.
    pub fn wait_frames(&self, frames: u64) -> Result<()> {
        let timeout = self.operation_timeout();
        let deadline = Instant::now() + timeout;

        let baseline = loop {
            match self.diagnostics_frame_count()? {
                Some(count) => break count,
                None => {
                    if Instant::now() >= deadline {
                        return Err(Error::Timeout {
                            operation: "wait_frames(baseline frame_count)".to_owned(),
                            timeout,
                        });
                    }
                    thread::sleep(POLL_INTERVAL);
                }
            }
        };

        let target = baseline.saturating_add(frames);
        loop {
            match self.diagnostics_frame_count()? {
                Some(current) if current >= target => return Ok(()),
                Some(_) | None => {
                    if Instant::now() >= deadline {
                        return Err(Error::Timeout {
                            operation: format!("wait_frames({frames})"),
                            timeout,
                        });
                    }
                    thread::sleep(POLL_INTERVAL);
                }
            }
        }
    }

    /// Wall-clock sleep (not frame-based).
    pub fn wait(&self, duration: Duration) -> Result<()> {
        let _ = self;
        thread::sleep(duration);
        Ok(())
    }

    /// Poll `predicate` until it returns `Ok(true)` or `timeout` elapses.
    pub fn wait_until<F>(&self, timeout: Duration, mut predicate: F) -> Result<()>
    where
        F: FnMut(&Self) -> Result<bool>,
    {
        let deadline = Instant::now() + timeout;
        loop {
            if predicate(self)? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout {
                    operation: "wait_until".to_owned(),
                    timeout,
                });
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn diagnostics_frame_count(&self) -> Result<Option<u64>> {
        let diagnostics = self.brp(DIAGNOSTICS_METHOD, json!({}))?;
        Ok(diagnostics.get("frame_count").and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().map(|n| n as u64))
                .or_else(|| value.as_f64().map(|n| n as u64))
        }))
    }
}
