use super::crypto::random::{RandomError, RtosClock};
use super::handshake::{EapolFrame, HandshakeError, RsnSupplicant};
use xm1_rtos_own_first::{Machine, SystemTime};
use crate::vendor_wireless_station_driver::association::MicFailure;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityError { Clock(RandomError), Handshake(HandshakeError) }
#[derive(Debug)]
pub enum SecurityAction { None, Report(EapolFrame), ReportAndStart(EapolFrame), Disconnect, Resume }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CountermeasureDriver { Unsupported }
#[derive(Clone, Copy)]
enum State { Idle, Drain(SystemTime), Blocking(SystemTime) }

pub struct TkipCountermeasures { last_failure: Option<SystemTime>, state: State }
impl Default for TkipCountermeasures { fn default() -> Self { Self::new() } }
impl TkipCountermeasures {
    pub const fn new() -> Self { Self { last_failure: None, state: State::Idle } }
    pub const fn active(&self) -> bool { !matches!(self.state, State::Idle) }
    pub const fn driver_capability(&self) -> CountermeasureDriver { CountermeasureDriver::Unsupported }
    pub fn reset(&mut self) { self.last_failure = None; self.state = State::Idle; }
    pub fn failure<M: Machine>(&mut self, event: MicFailure, clock: &RtosClock<'_, M>, handshake: &mut RsnSupplicant) -> Result<SecurityAction, SecurityError> {
        if self.active() { return Ok(SecurityAction::None); }
        let now = clock.sample().map_err(SecurityError::Clock)?;
        let report = handshake.michael_failure_request(event).map_err(SecurityError::Handshake)?;
        let repeated = self.last_failure.is_some_and(|last| {
            let elapsed = now.0.wrapping_sub(last.0) as u64;
            last.0 / 1000 != 0 && (elapsed + (last.0 % 1000) as u64) / 1000 <= 60
        });
        self.last_failure = Some(now);
        if repeated {
            self.state = State::Drain(now);
            Ok(SecurityAction::ReportAndStart(report))
        } else { Ok(SecurityAction::Report(report)) }
    }
    pub fn poll<M: Machine>(&mut self, clock: &RtosClock<'_, M>) -> Result<SecurityAction, SecurityError> {
        if !self.active() { return Ok(SecurityAction::None); }
        let now = clock.sample().map_err(SecurityError::Clock)?;
        match self.state {
            State::Drain(start) if now.0.wrapping_sub(start.0) >= 10 => {
                self.state = State::Blocking(now);
                Ok(SecurityAction::Disconnect)
            }
            State::Blocking(start) if now.0.wrapping_sub(start.0) >= 60_000 => {
                self.state = State::Idle;
                Ok(SecurityAction::Resume)
            }
            _ => Ok(SecurityAction::None),
        }
    }
}

#[cfg(test)]
#[path = "wireless_station_security_tests.rs"]
mod tests;
