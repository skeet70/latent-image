#![forbid(unsafe_code)]

use crate::{adjustment, capture, records, stock, synchronous};
use xm1_saclac_interface::{stock::StockCallbackWord, ClientCallback, CompletionStatus};

pub enum Request<Buffer> {
    StartRecord { callback: ClientCallback, buffer: Buffer, record: stock::RecordStart, completion: StockCallbackWord },
    StartCapture { callback: ClientCallback, buffers: [Buffer; 2], capture: capture::Capture, completions: capture::Callbacks },
    UpdateRecord { callback: ClientCallback, record: stock::RecordUpdate, limits: stock::RecordLimits, completion: StockCallbackWord },
    Reserve(ClientCallback),
    Lens { callback: ClientCallback, request: adjustment::LensRequest },
    ConfigureLens { callback: ClientCallback, selection: adjustment::Selection, limit: adjustment::SelectionLimit, value: adjustment::DriveAmount },
    ConfigureDefault(ClientCallback),
    LensPosition,
    LensMotion,
    Notify(Option<ClientCallback>),
    ReleaseParameters,
    PrimaryImageStatus,
    SecondaryImageStatus,
    CaptureValue,
    MovieStatus(synchronous::MoviePhase),
    SelectionRejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Value {
    None,
    Position(synchronous::LensPositionCode),
    Motion(adjustment::LensPosition),
    Image(synchronous::ImageStatus),
    Capture(records::RecordValue),
    Movie(synchronous::Readiness),
    SelectionRejected(bool),
}

pub trait Port<Buffer>: synchronous::Queries + synchronous::Effects + adjustment::Effects<ClientCallback> + records::Ports<Buffer> + capture::Port {
    fn start_record(&mut self, packet: &[u8; 68]);
    fn update_record(&mut self, packet: &[u8; 20]);
    fn release_parameters(&mut self);
    fn capture_value(&mut self) -> records::RecordValue;
}

pub enum Completion<Sample> {
    Lens(adjustment::Completion),
    Primary { status: CompletionStatus, values: [records::RecordPair; 3] },
    CaptureSample(Sample),
    CaptureAuxiliary(records::RecordPair),
    Capture(CompletionStatus),
    Update(CompletionStatus),
}

pub struct Exports<Buffer> {
    clients: synchronous::Clients,
    records: records::Records<Buffer>,
}

impl<Buffer: Copy> Default for Exports<Buffer> {
    fn default() -> Self { Self::new() }
}

impl<Buffer: Copy> Exports<Buffer> {
    pub const fn new() -> Self {
        Self { clients: synchronous::Clients::new(), records: records::Records::new() }
    }

    pub fn request(&mut self, request: Request<Buffer>, port: &mut impl Port<Buffer>) -> Value {
        match request {
            Request::StartRecord { callback, buffer, record, completion } => {
                self.records.start_primary(callback, buffer);
                port.start_record(&stock::record_start_payload(record, completion));
            }
            Request::StartCapture { callback, buffers, capture, completions } => {
                self.records.start_capture(callback, buffers);
                capture::start(capture, completions, port);
            }
            Request::UpdateRecord { callback, record, limits, completion } => {
                self.records.start_update(callback);
                port.update_record(&stock::record_update_payload(record, limits, completion));
            }
            Request::Reserve(callback) => { self.clients.reserve(callback, port); }
            Request::Lens { callback, request } => self.clients.begin_lens(callback, request, port),
            Request::ConfigureLens { callback, selection, limit, value } => self.clients.configure_lens(callback, selection, limit, value, port),
            Request::ConfigureDefault(callback) => self.clients.configure_default_lens(callback, port),
            Request::LensPosition => return Value::Position(synchronous::lens_position(port)),
            Request::LensMotion => return Value::Motion(synchronous::lens_motion(port)),
            Request::Notify(callback) => synchronous::notify(callback, port),
            Request::ReleaseParameters => port.release_parameters(),
            Request::PrimaryImageStatus => return Value::Image(port.image_primary()),
            Request::SecondaryImageStatus => return Value::Image(port.image_secondary()),
            Request::CaptureValue => return Value::Capture(port.capture_value()),
            Request::MovieStatus(phase) => return Value::Movie(synchronous::movie_status(phase, port)),
            Request::SelectionRejected => return Value::SelectionRejected(self.clients.selection_rejected()),
        }
        Value::None
    }

    pub fn completed<P: Port<Buffer>>(&mut self, completion: Completion<P::Sample>, port: &mut P) -> Result<(), records::Error> {
        match completion {
            Completion::Lens(completion) => self.clients.complete_lens(completion, port),
            Completion::Primary { status, values } => self.records.primary_completed(status, values, port)?,
            Completion::CaptureSample(sample) => self.records.capture_sample(sample, port)?,
            Completion::CaptureAuxiliary(value) => self.records.capture_auxiliary(value),
            Completion::Capture(status) => self.records.capture_completed(status, port)?,
            Completion::Update(status) => self.records.update_completed(status, port)?,
        }
        Ok(())
    }
}
