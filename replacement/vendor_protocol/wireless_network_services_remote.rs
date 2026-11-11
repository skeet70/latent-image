//! Remote selftest orchestration grounded in 0x01405d00..0x01406360.
use super::selftest::*;
use super::super::vendor_wireless_network::{lan::LanRuntime, transport::{TcpPort, TcpSequence}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteResult { Pages, Transferred { duration: u32 }, Reset, Identity { kind: u8, status: u8 }, Status(u8) }
#[derive(Clone, Copy)]
pub enum RemoteRequest<'a> { Pages, Transfer(&'a [u8]), Reset, Identity, Status }
#[derive(Clone, Copy)]
enum Step { ReadPage(u8), SetMode, Begin, Describe, Block, Finish, Status, Reset, Identity, QueryStatus, ErrorReset }
pub struct RemoteTest<'a> {
    exchange: NetworkSelftest, identity: TestIdentity, argument: TestArgument, port: TcpPort, sequence: TcpSequence,
    request: Option<RemoteRequest<'a>>, step: Step, waiting: bool, delay: TestTime, mode: bool,
    pages: [u8; 1440], block_size: usize, offset: usize, block: u32, retries: u8,
    failure: Option<TestError>, result: Option<Result<RemoteResult, TestError>>,
}
impl<'a> RemoteTest<'a> {
    pub fn new(identity: TestIdentity, argument: TestArgument, port: TcpPort, sequence: TcpSequence) -> Self {
        Self { exchange: NetworkSelftest::new(), identity, argument, port, sequence, request: None, step: Step::QueryStatus, waiting: false, delay: TestTime(0), mode: false, pages: [0; 1440], block_size: 0, offset: 0, block: 0, retries: 60, failure: None, result: None }
    }
    pub fn start(&mut self, request: RemoteRequest<'a>) -> Result<(), TestError> {
        if self.request.is_some() { return Err(TestError::Busy); }
        if let RemoteRequest::Transfer(bytes) = &request { if bytes.len() > u32::MAX as usize { return Err(TestError::Capacity); } }
        self.step = match request { RemoteRequest::Pages => Step::ReadPage(0), RemoteRequest::Transfer(_) => if self.mode { Step::Begin } else { Step::SetMode }, RemoteRequest::Reset => Step::Reset, RemoteRequest::Identity => Step::Identity, RemoteRequest::Status => Step::QueryStatus };
        self.request = Some(request); self.waiting = false; self.delay = TestTime(0); self.offset = 0; self.block = 0; self.retries = 60; self.failure = None; self.result = None;
        self.exchange.retain_session();
        Ok(())
    }
    pub fn result(&self) -> Option<Result<RemoteResult, TestError>> { self.result }
    pub fn pages(&self) -> Option<&[u8; 1440]> { if self.result == Some(Ok(RemoteResult::Pages)) { Some(&self.pages) } else { None } }
    pub fn poll(&mut self, lan: &mut LanRuntime<'_>, now: TestTime) -> bool {
        if self.request.is_none() || now.0 < self.delay.0 { return false; }
        if !self.waiting {
            if matches!(self.step, Step::Reset) && !self.mode {
                if let Err(error) = self.exchange.connect(lan, self.port, self.sequence, now) { return self.fail(lan, error); }
                self.waiting = true;
                return false;
            }
            let mut payload = [0u8; 1028];
            let (command, length) = match self.step {
                Step::ReadPage(index) => { payload[..4].copy_from_slice(&u32::from(index).to_be_bytes()); (TestCommand::ReadPage, 4) }
                Step::SetMode => { payload[..4].copy_from_slice(&1u32.to_le_bytes()); (TestCommand::SetMode, 4) }
                Step::Reset | Step::ErrorReset => (TestCommand::SetMode, 4),
                Step::Begin => (TestCommand::BeginTransfer, 0),
                Step::Describe => {
                    let length = match self.request { Some(RemoteRequest::Transfer(bytes)) => bytes.len() as u32, _ => 0 };
                    payload[0] = 2; payload[2..6].copy_from_slice(&length.to_be_bytes());
                    (TestCommand::DescribeTransfer, 12)
                }
                Step::Block => {
                    payload[..4].copy_from_slice(&self.block.to_be_bytes());
                    if let Some(RemoteRequest::Transfer(bytes)) = self.request {
                        let count = self.block_size.min(bytes.len() - self.offset);
                        payload[4..4 + count].copy_from_slice(&bytes[self.offset..self.offset + count]);
                    }
                    (TestCommand::WriteBlock, self.block_size + 4)
                }
                Step::Finish => (TestCommand::FinishTransfer, 0),
                Step::Status => (TestCommand::TransferStatus, 0),
                Step::Identity => (TestCommand::QueryIdentity, 0),
                Step::QueryStatus => (TestCommand::QueryStatus, 0),
            };
            if let Err(error) = self.exchange.begin(lan, self.port, self.sequence, self.identity, self.argument, command, &payload[..length], now) { return self.fail(lan, error); }
            self.waiting = true;
            return false;
        }
        if !self.exchange.poll(lan, now) { return false; }
        self.waiting = false;
        let result = match self.exchange.result() { Some(Ok(result)) => result, Some(Err(error)) => return self.fail(lan, error), None => return false };
        let response = self.exchange.response().unwrap_or(&[]);
        if matches!(self.step, Step::QueryStatus) { return self.finish(lan, Ok(RemoteResult::Status(result.status))); }
        if matches!(self.step, Step::Identity) {
            if response.len() < 16 { return self.fail(lan, TestError::Capacity); }
            let kind = (u32::from_be_bytes(response[12..16].try_into().unwrap()) & 15) as u8;
            return self.finish(lan, Ok(RemoteResult::Identity { kind: if kind <= 10 { kind } else { 0 }, status: result.status }));
        }
        if matches!(self.step, Step::ErrorReset) {
            if result.status <= 1 { self.mode = false; }
            let error = self.failure.take().unwrap_or(TestError::Closed);
            return self.finish(lan, Err(error));
        }
        if matches!(self.step, Step::Status) && matches!(result.status, 0xa3 | 0xa4) {
            self.retries -= 1;
            if self.retries == 0 { return self.fail(lan, TestError::Timeout); }
            if let Err(error) = self.exchange.close_session(lan) { return self.fail(lan, error); }
            self.advance_connection();
            self.exchange.retain_session(); self.delay = TestTime(now.0.saturating_add(1000));
            return false;
        }
        if result.status > 1 { return self.fail(lan, TestError::RemoteStatus(result.status)); }
        match self.step {
            Step::ReadPage(index) => {
                if response.len() < 500 { return self.fail(lan, TestError::Capacity); }
                let start = usize::from(index) * 480;
                self.pages[start..start + 480].copy_from_slice(&response[20..500]);
                if index == 2 { return self.finish(lan, Ok(RemoteResult::Pages)); }
                self.step = Step::ReadPage(index + 1);
            }
            Step::SetMode => { self.mode = true; self.step = Step::Begin; }
            Step::Begin => self.step = Step::Describe,
            Step::Describe => {
                if response.len() < 20 { return self.fail(lan, TestError::Capacity); }
                self.block_size = usize::from(u16::from_be_bytes([response[18], response[19]]));
                if self.block_size == 0 || self.block_size > 1024 { return self.fail(lan, TestError::Capacity); }
                self.step = if matches!(self.request, Some(RemoteRequest::Transfer(bytes)) if bytes.is_empty()) { Step::Finish } else { Step::Block };
            }
            Step::Block => {
                if let Some(RemoteRequest::Transfer(bytes)) = self.request {
                    self.offset += self.block_size.min(bytes.len() - self.offset);
                    self.block += 1;
                    if self.offset == bytes.len() { self.step = Step::Finish; }
                }
            }
            Step::Finish => { self.step = Step::Status; self.delay = TestTime(now.0.saturating_add(1000)); }
            Step::Status => {
                if response.len() < 20 { return self.fail(lan, TestError::Capacity); }
                let duration = u32::from_be_bytes(response[16..20].try_into().unwrap()).wrapping_mul(100);
                return self.finish(lan, Ok(RemoteResult::Transferred { duration }));
            }
            Step::Reset => { self.mode = false; return self.finish(lan, Ok(RemoteResult::Reset)); }
            Step::Identity | Step::QueryStatus | Step::ErrorReset => unreachable!(),
        }
        false
    }
    fn fail(&mut self, lan: &mut LanRuntime<'_>, error: TestError) -> bool {
        if matches!(self.request, Some(RemoteRequest::Transfer(_))) && !matches!(self.step, Step::ErrorReset) {
            self.failure = Some(error); self.step = Step::ErrorReset; self.waiting = false; self.delay = TestTime(0); false
        } else { self.finish(lan, Err(error)) }
    }
    fn finish(&mut self, lan: &mut LanRuntime<'_>, result: Result<RemoteResult, TestError>) -> bool {
        let close = self.exchange.close_session(lan);
        self.result = Some(match result { Err(error) => Err(error), Ok(value) => close.map(|()| value) });
        self.request = None; self.waiting = false;
        self.advance_connection();
        true
    }
    fn advance_connection(&mut self) {
        let next = self.port.get().wrapping_add(1).max(1024);
        if let Some(port) = TcpPort::parse(next) { self.port = port; }
        self.sequence = TcpSequence(self.sequence.0.wrapping_add(0x10000));
    }
    pub fn cancel(&mut self, lan: &mut LanRuntime<'_>) -> Result<(), TestError> {
        self.exchange.cancel(lan)?; self.request = None; self.result = Some(Err(TestError::Closed)); self.waiting = false; Ok(())
    }
}
