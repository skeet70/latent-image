#![no_std]
#![forbid(unsafe_code)]

pub mod exposure;
pub mod stock;
pub mod lens;
pub mod round;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientCallback(pub(crate) u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordHandle(pub(crate) u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParameterValue(pub u32);

/// Parameters whose meaning remains with the retained consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Parameters<'a>(pub(crate) &'a [u8]);
impl<'a> Parameters<'a> {
    pub fn bytes(self) -> &'a [u8] { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind {
    Start,
    Stop,
    Command02,
    Command03,
    LensBringUp,
    LensStop,
    HalfPress,
    Command07,
    Command08,
    Command09,
    Command0a,
    HalfRelease,
    Command0c,
    Autofocus,
    LensDrive,
    Cancel,
    Command10,
    Command11,
    Command13,
    Command14,
    Command15,
    Command16,
    Command17,
    Prepare,
    Command1d,
    Command1e,
    Command1f,
    Command20,
    Command21,
    Command22,
    Command23,
    Command24,
    Command25,
    Prepare26,
    Command27,
    Command28,
    Command29,
    Command2b,
    Command2c,
    Command2d,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Command<'a> {
    pub kind: CommandKind,
    pub parameters: Parameters<'a>,
}

/// A completion code is distinct from the stock code word used to deliver it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion {
    Lens2f,
    Lens30,
    Lens31,
    Lens32,
    Lens33,
    Lens34,
    Lens35,
    Lens36,
    Lens37,
    Lens38,
    Lens39,
    Lens3a,
    Lens3c,
    Lens3d,
    Image3f,
    Image40,
    Image41,
    Image42,
    Image43,
    Image44,
    Image45,
    Image46,
    Image47,
    Accessor4a,
    Accessor4b,
    Image4c,
    Image4d,
    Image4e,
    Image4f,
    Image50,
    Image51,
    Image52,
    Image53,
    Image57,
    Image58,
    Image59,
    Image5a,
    Image5b,
    Image5c,
    Image5d,
    Image5e,
    Image5f,
    Image60,
    Image61,
    Image62,
    Image63,
    Image64,
    Image65,
    Image66,
    Statistics69,
    Statistics6b,
    Statistics6c,
    Statistics6d,
    Statistics6e,
    Statistics6f,
    Statistics70,
    Statistics71,
    Statistics72,
    Statistics73,
    Statistics75,
    Statistics76,
    Statistics77,
    Statistics78,
    Statistics79,
    Statistics7a,
    Statistics7b,
    Statistics7c,
    Statistics7d,
    Statistics7e,
    Statistics7f,
    Statistics80,
    Statistics82,
    Statistics83,
    Statistics85,
    Calculation87,
    Calculation88,
    Calculation89,
    Calculation8a,
    Calculation8b,
    Calculation8d,
    Calculation8e,
    Calculation8f,
    Calculation90,
    Calculation91,
    Calculation92,
    Calculation93,
    Calculation94,
    Calculation95,
    Calculation97,
    Calculation98,
    Calculation99,
    Calculation9a,
    Calculation9b,
    Calculation9c,
    Calculation9d,
    Filea1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Completed<'a> {
    pub completion: Completion,
    pub parameters: Parameters<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Message<'a> { Command(Command<'a>), Completed(Completed<'a>) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockingRequest { LensSynchronize, AdjustmentSynchronize, FileSynchronize }

pub trait Blocking {
    type Error;
    fn execute(&mut self, request: BlockingRequest) -> Result<(), Self::Error>;
}

pub trait ClientReplies {
    type Error;
    fn reply(&mut self, target: ReplyTarget, callback: ClientCallback, status: CompletionStatus, values: Parameters<'_>) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionStatus(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientNotification { Notification12, Notification18, Notification19, Notification1b, Notification1c }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyTarget { Command(CommandKind), Notification(ClientNotification) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateState { Disabled, Enabled }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleRequest<'a> {
    ReserveAccessor,
    StartRecordingBuffers,
    StopRecordingBuffers,
    ResetImageBuffer,
    AuxiliaryGate(GateState),
    PublishParameters(Parameters<'a>),
}

pub trait Lifecycle {
    type Error;
    fn request(&mut self, request: LifecycleRequest<'_>) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensDcOperation {
    Request00,
    Request01,
    Request02,
    Request03,
    Request04,
    Request05,
    Request06,
    Request07,
    Request08,
    Request09,
    Request0b,
    Request0c,
    Request11,
    Request12,
    Request13,
    Request14,
    Request15,
    Request16,
    Request17,
    Request18,
    Request19,
    Request1a,
    Request1c,
    Request1d,
    Request1e,
    Request20,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensDcRequest<'a> {
    pub operation: LensDcOperation,
    pub parameters: Parameters<'a>,
    pub completions: &'a [Completion],
}

pub trait LensDc {
    type Error;
    fn request(&mut self, request: LensDcRequest<'_>) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImgDcOperation {
    Request00,
    Request01,
    Request13,
    Request16,
    Request18,
    Request1a,
    Request1b,
    Request1d,
    Request1e,
    Request1f,
    Request20,
    Request21,
    Request22,
    Request23,
    Request26,
    Request27,
    Request28,
    Request29,
    Request2a,
    Request2b,
    Request2c,
    Request2d,
    Request2e,
    Request2f,
    Request30,
    Request31,
    Request32,
    Request33,
    /// 20 bytes from 0xfea7dc (0xfea7f0..0xfea810): the completion word, then four words
    /// in argument order. The live view's two senders (0xf31548, 0xf315bc) pass completion
    /// 0xf2f684, which posts 0x63.
    Request34,
    Request3a,
    Request3b,
    Request42,
    Request43,
    Request44,
    Request45,
    Request46,
    Request47,
    Request49,
    Request4a,
    Request4e,
    Request4f,
    Request56,
    Request57,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImgDcRequest<'a> {
    pub operation: ImgDcOperation,
    pub parameters: Parameters<'a>,
    pub completions: &'a [Completion],
}

pub trait ImgDc {
    type Error;
    fn request(&mut self, request: ImgDcRequest<'_>) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalcOperation {
    Request00,
    Request01,
    Request02,
    Request03,
    Request04,
    Request05,
    Request09,
    Request0a,
    Request0c,
    Request0d,
    Request0e,
    Request0f,
    Request10,
    Request11,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalcRequest<'a> {
    pub operation: CalcOperation,
    pub parameters: Parameters<'a>,
    pub completions: &'a [Completion],
}

pub trait Calc {
    type Error;
    fn request(&mut self, request: CalcRequest<'_>) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Calc1Operation {
    Request00,
    Request01,
    Request02,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Calc1Request<'a> {
    pub operation: Calc1Operation,
    pub parameters: Parameters<'a>,
    pub completions: &'a [Completion],
}

pub trait Calc1 {
    type Error;
    fn request(&mut self, request: Calc1Request<'_>) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatisticsOperation {
    Request00,
    Request01,
    Request02,
    Request03,
    Request04,
    Request05,
    Request06,
    Request07,
    Request08,
    Request09,
    Request0a,
    Request0b,
    Request0c,
    Request0e,
    Request0f,
    Request10,
    Request11,
    Request12,
    Request13,
    Request14,
    Request17,
    Request2a,
    Request2c,
    Request2e,
    Request41,
    Request43,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatisticsRequest<'a> {
    pub operation: StatisticsOperation,
    pub parameters: Parameters<'a>,
    pub completions: &'a [Completion],
}

pub trait Statistics {
    type Error;
    fn request(&mut self, request: StatisticsRequest<'_>) -> Result<(), Self::Error>;
}

pub trait SharedParameters {
    type Error;
    fn read(&self, command: CommandKind, output: &mut [u8]) -> Result<usize, Self::Error>;
    fn write(&mut self, command: CommandKind, values: Parameters<'_>) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlashOperation { Request0d, Request0e }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlashRequest<'a> {
    pub operation: FlashOperation,
    pub parameters: Parameters<'a>,
    pub completions: &'a [Completion],
}

pub trait Flash {
    type Error;
    fn request(&mut self, request: FlashRequest<'_>) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerOperation {
    LensDc(LensDcOperation),
    ImgDc(ImgDcOperation),
    Calc(CalcOperation),
    Calc1(Calc1Operation),
    Statistics(StatisticsOperation),
    Flash(FlashOperation),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensEffect { NoLensDrive, DirectDrive, DriveThroughImageBridge, Unclassified }
impl LensEffect {
    pub const fn requires_lens_safety_contract(self) -> bool { !matches!(self, Self::NoLensDrive) }
}

pub const fn lens_effect(operation: PeerOperation) -> LensEffect {
    match operation {
        PeerOperation::LensDc(LensDcOperation::Request07 | LensDcOperation::Request08 | LensDcOperation::Request0c) => LensEffect::DirectDrive,
        PeerOperation::ImgDc(ImgDcOperation::Request2e) => LensEffect::DriveThroughImageBridge,
        PeerOperation::Calc(_) | PeerOperation::Calc1(_) | PeerOperation::Statistics(_) => LensEffect::NoLensDrive,
        PeerOperation::LensDc(_) | PeerOperation::ImgDc(_) | PeerOperation::Flash(_) => LensEffect::Unclassified,
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod exposure_tests;
