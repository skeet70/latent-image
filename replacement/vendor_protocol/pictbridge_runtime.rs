#[path = "pictbridge_xml.rs"]
pub mod xml;
use core::fmt::Write;
use xml::{XmlBuffer, XmlError, document, element, hex};
#[cfg(test)]
#[path = "pictbridge_runtime_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileId(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbortStyle { AfterCurrent, Immediate }
impl AbortStyle {
    /// Stock 0x00d9559c compares its argument with 0x90010000.
    pub const fn parse(raw: u32) -> Self { if raw == 0x9001_0000 { Self::Immediate } else { Self::AfterCurrent } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Copies { Count(u32), Mpo }
impl Copies {
    /// Stock compares the copies word with 0x0d000000 to select the MPO file type.
    pub const fn parse(raw: u32) -> Self { if raw == 0x0d00_0000 { Self::Mpo } else { Self::Count(raw) } }
}
pub struct PrintRequest<'a> { pub file: FileId, pub copies: Copies, pub date: Option<&'a [u8]> }
pub struct Identity<'a> { pub vendor: &'a str, pub product: &'a str, pub serial: &'a str }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase { Disabled, Configure, Idle, Configuring, DateCapability, TypeCapability, Start, Abort, Continue, DeviceStatus, JobStatus }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendStatus { Sent, Failed, Busy }
impl SendStatus { pub const fn status(self) -> u32 { match self { Self::Sent => 0, Self::Failed => 0x80000000, Self::Busy => 0x80000002 } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DpsFile { Request, Response }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DpsObject { pub id: FileId, pub file: DpsFile, pub enabled: bool, pub length: u32 }
pub struct CatalogFile<'a> { pub id: FileId, pub path: &'a str }
pub struct DpsTransport<'a> {
    pub objects: &'a mut [DpsObject], pub files: &'a [CatalogFile<'a>],
    send_locked: bool, transfer_active: bool, readiness: u32, pending: Option<FileId>,
}
impl<'a> DpsTransport<'a> {
    pub fn new(objects: &'a mut [DpsObject], files: &'a [CatalogFile<'a>]) -> Self { Self { objects, files, send_locked: false, transfer_active: false, readiness: 0, pending: None } }
    pub fn readiness(&self) -> u32 { self.readiness }
    pub fn observe_host_discovery(&mut self, path: &str) { if path == "Z:\\HDISCVRY.DPS" { self.readiness = 1; } }
    pub fn set_transfer_active(&mut self, value: bool) { self.transfer_active = value; }
    pub fn pending_object(&self) -> Option<FileId> { self.pending }
    pub fn complete_send(&mut self) { self.pending = None; self.send_locked = false; }
    pub fn send_path(&mut self, path: &str, length: usize) -> SendStatus {
        match path { "Z:\\DREQUEST.DPS" => self.send(DpsFile::Request, length), "Z:\\DRSPONSE.DPS" => self.send(DpsFile::Response, length), _ => SendStatus::Failed }
    }
    pub fn send(&mut self, file: DpsFile, length: usize) -> SendStatus {
        if self.send_locked { return SendStatus::Busy; }
        self.send_locked = true;
        if self.transfer_active { self.send_locked = false; return SendStatus::Busy; }
        let index = self.objects.iter().take(6).position(|object| object.enabled && object.file == file)
            .or_else(|| self.objects.iter().take(6).position(|object| !object.enabled));
        let Some(index) = index else { self.send_locked = false; return SendStatus::Failed; };
        let object = &mut self.objects[index];
        object.file = file;
        object.enabled = true;
        object.id = FileId(index as u32 + 1);
        object.length = length as u32;
        if self.pending.is_some() { self.send_locked = false; return SendStatus::Busy; }
        self.pending = Some(object.id); SendStatus::Sent
    }
    pub fn resolve(&self, path: &str) -> Option<FileId> { self.files.iter().find(|file| file.path == path).map(|file| file.id) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion { Success, Failed }
impl Completion { pub const fn status(self) -> u32 { match self { Self::Success => 0, Self::Failed => 0xffff_ff01 } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Publication {
    PrintState { completion: Completion, date: bool, jpeg: bool, vendor: [u8; 65], product: [u8; 65] },
    Capabilities { date: bool, jpeg: bool }, Start(Completion), Abort(Completion), Continue(Completion),
    Device { completion: Completion, status: [u32; 7] }, Job { completion: Completion, progress: [u8; 8], images: [u8; 4] },
}
pub struct PictBridge {
    enabled: bool, phase: Phase, initial: bool, available: bool, date_supported: bool, jpeg_supported: bool, mpo_supported: bool,
    vendor: [u8; 65], product: [u8; 65], status: [u32; 7], progress: [u8; 8], images: [u8; 4],
    file: FileId, copies: Copies, date: [u8; 16], date_length: usize, abort_style: AbortStyle,
    date_pending: bool, type_pending: bool, start_pending: bool, abort_pending: bool, continue_pending: bool, device_pending: bool, job_pending: bool,
    request: XmlBuffer, response: XmlBuffer, response_pending: bool, request_length: usize, publication: Option<Publication>,
}
impl Default for PictBridge { fn default() -> Self { Self::new() } }
impl PictBridge {
    pub const fn new() -> Self { Self {
        enabled: false, phase: Phase::Disabled, initial: false, available: false, date_supported: false, jpeg_supported: false, mpo_supported: false,
        vendor: [0; 65], product: [0; 65], status: [0; 7], progress: [0; 8], images: [0; 4], file: FileId(0), copies: Copies::Count(0), date: [0; 16], date_length: 0, abort_style: AbortStyle::AfterCurrent,
        date_pending: false, type_pending: false, start_pending: false, abort_pending: false, continue_pending: false, device_pending: false, job_pending: false,
        request: XmlBuffer::new(), response: XmlBuffer::new(), response_pending: false, request_length: 0, publication: None,
    } }
    pub fn initialize(&mut self) {
        if self.enabled { return; }
        self.enabled = true; self.available = false; self.initial = true; self.phase = Phase::Configure;
        self.response_pending = false; self.date_pending = false; self.type_pending = false; self.start_pending = false; self.abort_pending = false; self.continue_pending = false; self.device_pending = false;
        self.status = [0x70010000, 0x71010000, 0x72000000, 0x73000000, 0x74000000, 0x75000000, 0x76000000];
        self.date_supported = false; self.jpeg_supported = false; self.mpo_supported = false;
        self.vendor[0] = 0; self.product[0] = 0; self.progress[0] = 0; self.images[0] = 0;
    }
    pub fn reset_flags(&mut self) { self.enabled = false; self.phase = Phase::Disabled; }
    pub fn reset(&mut self) { self.reset_flags(); }
    pub fn phase(&self) -> Phase { self.phase }
    pub fn request_bytes(&self) -> &[u8] { self.request.bytes() }
    pub fn response_bytes(&self) -> &[u8] { self.response.bytes() }
    pub fn object_bytes(&self, file: DpsFile) -> &[u8] { match file { DpsFile::Request => self.request_bytes(), DpsFile::Response => self.response_bytes() } }
    pub fn select_buffer(&self, path: &str) -> Option<&[u8]> { match path { "Z:\\DREQUEST.DPS" => Some(self.request_bytes()), "Z:\\DRSPONSE.DPS" => Some(self.response_bytes()), _ => None } }
    pub fn dispatch_packet(&mut self, path: &str, packet: &[u8], transport: &DpsTransport<'_>) -> Result<u32, XmlError> {
        match path {
            "Z:\\HREQUEST.DPS" => self.receive_request(packet, transport)?,
            "Z:\\HRSPONSE.DPS" => self.receive_response(packet),
            _ => return Ok(0x80000000),
        }
        Ok(0)
    }
    pub fn immediate_abort_requested(&self) -> bool { self.abort_style == AbortStyle::Immediate }
    pub fn take_publication(&mut self) -> Option<Publication> { self.publication.take() }
    pub fn set_request(&mut self, request: PrintRequest<'_>) {
        self.file = request.file; self.copies = request.copies; self.date_length = 0;
        if let Some(date) = request.date { for byte in date.iter().take(15).take_while(|byte| **byte != 0) { self.date[self.date_length] = *byte; self.date_length += 1; } }
        self.date[self.date_length] = 0; self.progress[0] = 0; self.images[0] = 0;
        self.start_pending = true; self.abort_pending = false; self.continue_pending = false;
    }
    pub fn abort(&mut self, style: AbortStyle) { self.abort_style = style; self.abort_pending = true; self.start_pending = false; self.continue_pending = false; }
    pub fn continue_job(&mut self) { self.continue_pending = true; self.start_pending = false; self.abort_pending = false; }
    pub fn request_device_status(&mut self) { self.device_pending = true; }
    pub fn request_job_status(&mut self) { self.job_pending = true; }
    fn print_state(&mut self, completion: Completion) { self.publication = Some(Publication::PrintState { completion, date: self.date_supported, jpeg: self.jpeg_supported, vendor: self.vendor, product: self.product }); self.initial = false; }
    fn failed(&mut self, phase: Phase) {
        match phase { Phase::Configuring | Phase::DateCapability | Phase::TypeCapability => { self.date_pending = false; self.type_pending = false; if self.initial { self.print_state(Completion::Failed); } },
            Phase::Start => self.publication = Some(Publication::Start(Completion::Failed)), Phase::Abort => self.publication = Some(Publication::Abort(Completion::Failed)), Phase::Continue => self.publication = Some(Publication::Continue(Completion::Failed)), _ => () }
    }
    fn build(&mut self, phase: Phase, identity: &Identity<'_>) -> Result<(), XmlError> {
        let out = &mut self.request; out.begin(false)?;
        match phase {
            Phase::Configuring => { out.append("\t\t<configurePrintService>\n")?; out.field("dpsVersions", "1.0", 3)?; out.field("vendorName", identity.vendor, 3)?; out.field("vendorSpecificVersion", "1.00", 3)?; out.field("productName", identity.product, 3)?;
                let serial = identity.serial.as_bytes(); let end = serial.iter().take(30).position(|byte| *byte == 0).unwrap_or(serial.len().min(30));
                out.field("serialNo", core::str::from_utf8(&serial[..end]).map_err(|_| XmlError::Malformed)?, 3)?; out.append("\t\t</configurePrintService>\n")?; }
            Phase::DateCapability | Phase::TypeCapability => { out.append("\t\t<getCapability>\n\t\t\t<capability>\n\t\t\t\t<")?; out.append(if phase == Phase::DateCapability { "datePrints" } else { "fileTypes" })?; out.append("/>\n\t\t\t</capability>\n\t\t</getCapability>\n")?; }
            Phase::Start => { out.append("\t\t<startJob>\n\t\t\t<jobConfig>\n")?;
                if self.copies == Copies::Mpo { out.word_field("fileType", 0x53180000, 4)?; } else { out.word_field("datePrint", if self.date_supported && self.date_length != 0 { 0x54020000 } else { 0x54010000 }, 4)?; }
                out.append("\t\t\t</jobConfig>\n\t\t\t<printInfo>\n")?; out.word_field("fileID", self.file.0, 4)?;
                if let Copies::Count(count) = self.copies {
                    if self.date_supported && self.date_length != 0 { out.bytes_field("date", &self.date[..self.date_length], 4)?; }
                    if count > 1 { let count = count.min(999); self.copies = Copies::Count(count); out.append("\t\t\t\t<copies>")?; write!(out, "{count:03}").map_err(|_| XmlError::Capacity)?; out.append("</copies>\n")?; } else { self.copies = Copies::Count(1); }
                }
                out.append("\t\t\t</printInfo>\n\t\t</startJob>\n")?;
            }
            Phase::Abort => { out.append("\t\t<abortJob>\n")?; out.word_field("abortStyle", 0x90000000, 3)?; out.append("\t\t</abortJob>\n")?; }
            Phase::Continue => out.append("\t\t<continueJob/>\n")?, Phase::DeviceStatus => out.append("\t\t<getDeviceStatus/>\n")?, Phase::JobStatus => out.append("\t\t<getJobStatus/>\n")?, _ => return Err(XmlError::Malformed),
        }
        out.end(false)?; self.request_length = out.bytes().len(); Ok(())
    }
    pub fn service(&mut self, identity: &Identity<'_>, transport: &mut DpsTransport<'_>) -> Result<(), XmlError> {
        if !self.enabled { return Ok(()); }
        if self.phase == Phase::Configure {
            if transport.readiness() == 0 { return Ok(()); }
            self.build(Phase::Configuring, identity)?;
            match transport.send(DpsFile::Request, self.request_length) { SendStatus::Failed => { self.print_state(Completion::Failed); self.phase = Phase::Configuring; }, SendStatus::Sent => { self.date_pending = false; self.type_pending = false; self.phase = Phase::Configuring; }, SendStatus::Busy => () }
            return Ok(());
        }
        if self.phase != Phase::Idle { return Ok(()); }
        if self.response_pending { match transport.send(DpsFile::Response, self.response.bytes().len()) { SendStatus::Busy => (), SendStatus::Failed => self.response_pending = false, SendStatus::Sent => { self.request_length = self.response.bytes().len(); self.response_pending = false; } }; return Ok(()); }
        let phase = if self.date_pending { Phase::DateCapability } else if self.type_pending { Phase::TypeCapability } else if self.start_pending { Phase::Start } else if self.abort_pending { Phase::Abort } else if self.continue_pending { Phase::Continue } else if self.device_pending { Phase::DeviceStatus } else if self.job_pending { Phase::JobStatus } else { return Ok(()); };
        let allowed = match phase { Phase::Start => self.status[6] == 0x76010000, Phase::Abort => matches!(self.status[0], 0x70000000 | 0x70020000), Phase::Continue => self.status[0] == 0x70020000 && self.status[2] == 0x72010000, _ => true };
        if !allowed { self.clear_pending(phase); self.failed(phase); return Ok(()); }
        self.build(phase, identity)?;
        match transport.send(DpsFile::Request, self.request_length) { SendStatus::Busy => (), SendStatus::Failed => { self.clear_pending(phase); self.failed(phase); }, SendStatus::Sent => { self.clear_pending(phase); self.phase = phase; } }
        Ok(())
    }
    fn clear_pending(&mut self, phase: Phase) { match phase { Phase::DateCapability => self.date_pending = false, Phase::TypeCapability => self.type_pending = false, Phase::Start => self.start_pending = false, Phase::Abort => self.abort_pending = false, Phase::Continue => self.continue_pending = false, Phase::DeviceStatus => self.device_pending = false, Phase::JobStatus => self.job_pending = false, _ => () } }
    fn read_device(&mut self, input: &str) -> bool {
        let mut valid = true;
        for (index, tag, min, max) in [(0,"dpsPrintServiceStatus",0x70000000,0x70020000),(6,"newJobOK",0x76000000,0x76010000),(2,"errorStatus",0x72000000,0x72020000),(1,"jobEndReason",0x71000000,0x71040000),(5,"capabilityChanged",0x75000000,0x75010000),(4,"disconnectEnable",0x74000000,0x74010000)] {
            if let Some(value) = element(input, tag).and_then(hex) { let value = value & 0xffff0000; if value >= min && value <= max { self.status[index] = value; if index == 5 && value == 0x75010000 { self.date_pending = true; self.type_pending = true; } } else { valid = false; } }
        }
        self.status[3] = element(input, "errorReason").and_then(hex).unwrap_or(0); valid
    }
    fn read_job(&mut self, input: &str) { if let Some(text) = element(input, "progress") { copy_text(&mut self.progress, text); } if let Some(text) = element(input, "imagesPrinted") { copy_text(&mut self.images, text); } }
    pub fn receive_request(&mut self, packet: &[u8], transport: &DpsTransport<'_>) -> Result<(), XmlError> {
        if !self.enabled || self.phase != Phase::Idle || packet.len() > 65536 { return Ok(()); }
        let input = document(packet).ok().and_then(|text| element(text, "input"));
        let mut result = 0x10030000; let mut notification = None;
        if let Some(input) = input {
            if let Some(job) = element(input, "notifyJobStatus") { self.read_job(job); self.publication = Some(Publication::Job { completion: Completion::Success, progress: self.progress, images: self.images }); result = 0x10000000; notification = Some("notifyJobStatus"); }
            else if let Some(device) = element(input, "notifyDeviceStatus") { let valid = self.read_device(device); self.publication = Some(Publication::Device { completion: if valid { Completion::Success } else { Completion::Failed }, status: self.status }); result = if valid { 0x10000000 } else { 0x10020000 }; if valid { notification = Some("notifyDeviceStatus"); } }
            else if let Some(query) = element(input, "getFileID") {
                result = 0x10010000;
                if let Some(path) = element(query, "filePath") { let mut bytes = [0u8; 40]; bytes[0] = b'A'; bytes[1] = b':'; let mut length = 2;
                    for byte in path.bytes().take(31).skip(2).take_while(|byte| *byte != 0) { bytes[length] = if byte == b'/' { b'\\' } else { byte }; length += 1; }
                    if let Some(id) = core::str::from_utf8(&bytes[..length]).ok().and_then(|path| transport.resolve(path)) {
                        self.response.begin(true)?; self.response.word_field("result", 0x10000000, 2)?; self.response.append("\t\t<getFileID>\n")?; self.response.word_field("fileID", id.0, 3)?; self.response.append("\t\t</getFileID>\n")?; self.response.end(true)?; self.response_pending = true; return Ok(());
                    }
                }
            }
        }
        self.response.result(result, notification)?; self.response_pending = true; Ok(())
    }
    pub fn receive_response(&mut self, packet: &[u8]) {
        if !self.enabled || !matches!(self.phase, Phase::Configuring | Phase::DateCapability | Phase::TypeCapability | Phase::Start | Phase::Abort | Phase::Continue | Phase::DeviceStatus | Phase::JobStatus) || packet.len() > 65536 { return; }
        let phase = self.phase;
        let output = document(packet).ok().and_then(|text| element(text, "output"));
        let mut success = output.and_then(|text| element(text, "result")).and_then(hex) == Some(0x10000000);
        if success { let output = output.unwrap(); success = match phase {
            Phase::Configuring => if let Some(configure) = element(output, "configurePrintService") {
                if let Some(value) = element(configure, "printServiceAvailable").and_then(hex) { self.available = value != 0x30000000; }
                if let Some(vendor) = element(configure, "vendorName") { copy_text(&mut self.vendor, vendor); } if let Some(product) = element(configure, "productName") { copy_text(&mut self.product, product); } self.available
            } else { false },
            Phase::DateCapability => if let Some(capability) = element(output, "getCapability") { let old = self.date_supported; self.date_supported = element(capability, "datePrints").is_some_and(|text| text.contains("54020000")); if old != self.date_supported && !self.initial { self.publication = Some(Publication::Capabilities { date: self.date_supported, jpeg: self.jpeg_supported }); } true } else { false },
            Phase::TypeCapability => if let Some(capability) = element(output, "getCapability") { let old = self.jpeg_supported; self.jpeg_supported = element(capability, "fileTypes").is_some_and(|text| text.contains("53030000")); self.mpo_supported = capability.contains("53180000"); if old != self.jpeg_supported && !self.initial { self.publication = Some(Publication::Capabilities { date: self.date_supported, jpeg: self.jpeg_supported }); } true } else { false },
            Phase::Start => element(output, "startJob").is_some(), Phase::Abort => element(output, "abortJob").is_some(), Phase::Continue => element(output, "continueJob").is_some(),
            Phase::DeviceStatus => element(output, "getDeviceStatus").is_some_and(|device| self.read_device(device)),
            Phase::JobStatus => if let Some(job) = element(output, "getJobStatus") { self.read_job(job); true } else { false }, _ => false,
        }; }
        let completion = if success { Completion::Success } else { Completion::Failed };
        match phase {
            Phase::Configuring => if success { self.date_pending = true; self.type_pending = true; } else { self.print_state(Completion::Failed); },
            Phase::TypeCapability => if self.initial { self.print_state(Completion::Success); },
            Phase::Start => { if success { self.status[6] = 0x76000000; } self.publication = Some(Publication::Start(completion)); },
            Phase::Abort => self.publication = Some(Publication::Abort(completion)), Phase::Continue => self.publication = Some(Publication::Continue(completion)),
            Phase::DeviceStatus => self.publication = Some(Publication::Device { completion, status: self.status }), Phase::JobStatus => self.publication = Some(Publication::Job { completion, progress: self.progress, images: self.images }), _ => (),
        }
        self.phase = Phase::Idle;
    }
}
fn copy_text<const N: usize>(output: &mut [u8; N], text: &str) { let length = text.len().min(N - 1); output[..length].copy_from_slice(&text.as_bytes()[..length]); output[length] = 0; }
