#![forbid(unsafe_code)]

use xm1_saclac_interface::{ClientCallback, CompletionStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordValue(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordPair(pub [RecordValue; 2]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Primary,
    Secondary,
    Tertiary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reply<Buffer> {
    Primary { callback: ClientCallback, status: CompletionStatus, buffer: Buffer, values: [RecordPair; 3] },
    Capture { callback: ClientCallback, status: CompletionStatus, buffers: [Buffer; 2], values: [RecordPair; 3] },
    Updated { callback: ClientCallback, status: CompletionStatus },
}

pub trait Ports<Buffer> {
    type Sample;

    fn read_sample(&mut self, sample: Self::Sample, destination: Buffer) -> [RecordPair; 2];
    fn finish_primary(&mut self, buffer: Buffer);
    fn finish_capture(&mut self);
    fn release(&mut self, source: Source);
    fn reply(&mut self, reply: Reply<Buffer>);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    NoPrimaryClient,
    NoCaptureClient,
    NoUpdateClient,
    NoBuffer,
}

pub struct Records<Buffer> {
    primary: Option<ClientCallback>,
    capture: Option<ClientCallback>,
    update: Option<ClientCallback>,
    first_buffer: Option<Buffer>,
    second_buffer: Option<Buffer>,
    samples: u8,
    values: [RecordPair; 3],
}

impl<Buffer: Copy> Default for Records<Buffer> {
    fn default() -> Self { Self::new() }
}

impl<Buffer: Copy> Records<Buffer> {
    pub const fn new() -> Self {
        Self {
            primary: None, capture: None, update: None,
            first_buffer: None, second_buffer: None, samples: 0,
            values: [RecordPair([RecordValue(0); 2]); 3],
        }
    }

    pub fn start_primary(&mut self, callback: ClientCallback, buffer: Buffer) {
        *self = Self::new();
        self.primary = Some(callback);
        self.first_buffer = Some(buffer);
    }

    pub fn start_capture(&mut self, callback: ClientCallback, buffers: [Buffer; 2]) {
        self.capture = Some(callback);
        self.first_buffer = Some(buffers[0]);
        self.second_buffer = Some(buffers[1]);
        self.samples = 0;
    }

    pub fn start_update(&mut self, callback: ClientCallback) { self.update = Some(callback); }

    pub fn primary_completed(&mut self, status: CompletionStatus, values: [RecordPair; 3], ports: &mut impl Ports<Buffer>) -> Result<(), Error> {
        let callback = self.primary.ok_or(Error::NoPrimaryClient)?;
        let buffer = self.first_buffer.ok_or(Error::NoBuffer)?;
        self.values = values;
        ports.finish_primary(buffer);
        ports.reply(Reply::Primary { callback, status, buffer, values });
        Ok(())
    }

    pub fn capture_sample<P: Ports<Buffer>>(&mut self, sample: P::Sample, ports: &mut P) -> Result<(), Error> {
        self.capture.ok_or(Error::NoCaptureClient)?;
        self.samples = self.samples.wrapping_add(1);
        let second = self.samples == 2;
        let buffer = if second { self.second_buffer } else { self.first_buffer }.ok_or(Error::NoBuffer)?;
        let values = ports.read_sample(sample, buffer);
        if !second { self.values[..2].copy_from_slice(&values); }
        Ok(())
    }

    pub fn capture_auxiliary(&mut self, value: RecordPair) { self.values[2] = value; }

    pub fn capture_completed(&mut self, status: CompletionStatus, ports: &mut impl Ports<Buffer>) -> Result<(), Error> {
        let callback = self.capture.ok_or(Error::NoCaptureClient)?;
        let buffers = [self.first_buffer.ok_or(Error::NoBuffer)?, self.second_buffer.ok_or(Error::NoBuffer)?];
        ports.finish_capture();
        for source in [Source::Primary, Source::Secondary, Source::Tertiary] { ports.release(source); }
        ports.reply(Reply::Capture { callback, status, buffers, values: self.values });
        Ok(())
    }

    pub fn update_completed(&mut self, status: CompletionStatus, ports: &mut impl Ports<Buffer>) -> Result<(), Error> {
        let callback = self.update.ok_or(Error::NoUpdateClient)?;
        ports.reply(Reply::Updated { callback, status });
        Ok(())
    }
}
