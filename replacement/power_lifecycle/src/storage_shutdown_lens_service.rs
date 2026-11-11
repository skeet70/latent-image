use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos};
use crate::storage_shutdown_lifecycle::{LensPageOperation, LensTransferPlan};
use crate::storage_shutdown_load::LensChannel;
use crate::storage_shutdown_pages::{ConfigurationAddress, PageNumber};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct LensCallback(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensServiceCommand { SelectRead, SelectWrite, ReadReady, ReadPage, WritePage, PrepareAuxiliary, Extended, Channel, PreparePrimary }
impl LensServiceCommand {
    pub const fn operation(self) -> RequestId {
        RequestId(match self {
            Self::SelectRead => 0x7259, Self::SelectWrite => 0x7258, Self::ReadReady => 0x7257,
            Self::ReadPage => 0x7254, Self::WritePage => 0x7252, Self::PrepareAuxiliary => 0x7234,
            Self::Extended => 0x7233, Self::Channel => 0x722e, Self::PreparePrimary => 0x722b,
        })
    }
    pub const fn failure_code(self) -> u32 {
        match self {
            Self::SelectRead => 0x51, Self::SelectWrite => 0x50, Self::ReadReady => 0x4f,
            Self::ReadPage => 0x4e, Self::WritePage => 0x4d, Self::PrepareAuxiliary => 0x2d,
            Self::Extended => 0x2c, Self::Channel => 0x28, Self::PreparePrimary => 0x25,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StackHalfword(pub u16);

pub trait LensPostDiagnostic {
    fn send_failed(&mut self, operation: LensServiceCommand);
}

fn post(rtos: &mut impl ShutdownRtos, command: LensServiceCommand, payload: &[u8], diagnostic: &mut impl LensPostDiagnostic) -> Result<(), KernelStatus> {
    let sender = rtos.current_task();
    let result = rtos.post(ServiceMessage { operation: command.operation(), sender, destination: QueueId(98), payload });
    if result.is_err() { diagnostic.send_failed(command); }
    result
}
fn put_word(payload: &mut [u8], offset: usize, value: u32) { payload[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
fn put_half(payload: &mut [u8], offset: usize, value: u16) { payload[offset..offset + 2].copy_from_slice(&value.to_le_bytes()); }
fn packet(payload: &mut [u8; 0x30], callback: LensCallback, command_words: u8) {
    put_word(payload, 0, callback.0);
    payload[4..4 + usize::from(command_words) * 4].fill(0);
    payload[0x2c] = command_words;
}

pub fn select(
    rtos: &mut impl ShutdownRtos,
    payload: &mut [u8; 0x30],
    plan: LensTransferPlan,
    write: bool,
    callback: LensCallback,
    diagnostic: &mut impl LensPostDiagnostic,
) -> Result<(), KernelStatus> {
    packet(payload, callback, 2);
    put_half(payload, 4, 0x600);
    put_half(payload, 6, (plan.pages & 0x7ff) | ((plan.operation.abi() as u16 & 0xf) << 11) | 0x8000);
    put_half(payload, 8, if write { 0x2480 } else { 0x2440 });
    post(rtos, if write { LensServiceCommand::SelectWrite } else { LensServiceCommand::SelectRead }, payload, diagnostic)
}

pub fn read_ready(rtos: &mut impl ShutdownRtos, payload: &mut [u8; 0x30], callback: LensCallback, diagnostic: &mut impl LensPostDiagnostic) -> Result<(), KernelStatus> {
    packet(payload, callback, 1);
    put_half(payload, 4, 0x2400);
    post(rtos, LensServiceCommand::ReadReady, payload, diagnostic)
}

pub fn read_page(
    rtos: &mut impl ShutdownRtos,
    payload: &mut [u8; 0x14],
    page: PageNumber,
    stack_halfword: StackHalfword,
    operation: LensPageOperation,
    callback: LensCallback,
    diagnostic: &mut impl LensPostDiagnostic,
) -> Result<(), KernelStatus> {
    put_half(payload, 0, page.0);
    put_half(payload, 2, stack_halfword.0);
    put_word(payload, 4, operation.abi());
    put_word(payload, 8, 1);
    put_word(payload, 12, 1);
    put_word(payload, 16, callback.0);
    post(rtos, LensServiceCommand::ReadPage, payload, diagnostic)
}

pub fn write_page(
    rtos: &mut impl ShutdownRtos,
    payload: &mut [u8; 0x14],
    page: PageNumber,
    stack_halfword: StackHalfword,
    address: ConfigurationAddress,
    operation: LensPageOperation,
    callback: LensCallback,
    diagnostic: &mut impl LensPostDiagnostic,
) -> Result<(), KernelStatus> {
    put_half(payload, 0, page.0);
    put_half(payload, 2, stack_halfword.0);
    put_word(payload, 4, address.0);
    put_word(payload, 8, operation.abi());
    put_word(payload, 12, 1);
    put_word(payload, 16, callback.0);
    post(rtos, LensServiceCommand::WritePage, payload, diagnostic)
}

pub fn prepare_auxiliary(rtos: &mut impl ShutdownRtos, payload: &mut [u8; 0x30], callback: LensCallback, diagnostic: &mut impl LensPostDiagnostic) -> Result<(), KernelStatus> {
    packet(payload, callback, 1);
    put_half(payload, 4, 0x4540);
    post(rtos, LensServiceCommand::PrepareAuxiliary, payload, diagnostic)
}

pub fn extended(rtos: &mut impl ShutdownRtos, payload: &mut [u8; 0x30], mode: u8, callback: LensCallback, diagnostic: &mut impl LensPostDiagnostic) -> Result<(), KernelStatus> {
    packet(payload, callback, 1);
    put_half(payload, 4, 0x4500);
    payload[6] = mode & 0xf;
    post(rtos, LensServiceCommand::Extended, payload, diagnostic)
}

pub fn channel(rtos: &mut impl ShutdownRtos, payload: &mut [u8; 0x30], channel: LensChannel, callback: LensCallback, diagnostic: &mut impl LensPostDiagnostic) -> Result<(), KernelStatus> {
    packet(payload, callback, 1);
    put_half(payload, 4, 0x3000);
    payload[6] = channel.abi() & 0xf;
    post(rtos, LensServiceCommand::Channel, payload, diagnostic)
}

pub fn prepare_primary(rtos: &mut impl ShutdownRtos, payload: &mut [u8; 0x30], callback: LensCallback, diagnostic: &mut impl LensPostDiagnostic) -> Result<(), KernelStatus> {
    packet(payload, callback, 1);
    put_half(payload, 4, 0x2800);
    payload[6] = 0x10;
    payload[7] = 0x80;
    post(rtos, LensServiceCommand::PreparePrimary, payload, diagnostic)
}

#[cfg(test)]
#[path = "storage_shutdown_lens_service_tests.rs"]
mod tests;
