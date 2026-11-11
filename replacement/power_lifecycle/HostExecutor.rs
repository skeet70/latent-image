use std::io::{self, Read, Write};
use xm1_power_lifecycle::{area2::*, area2_codec::Area2Codec, mirror::*, record_store::*};

struct Medium<'a> { banks: &'a mut [u8], fail: Option<u32>, requests: u32 }
impl RecordMedium for Medium<'_> {
    fn read(&mut self, _: RecordKind, bank: Bank, offset: BytePosition, output: &mut [u8]) -> Result<(), MediumError> {
        let start = bank as usize * BLOCK_BYTES + offset.0 as usize;
        output.copy_from_slice(&self.banks[start..start + output.len()]); Ok(())
    }
    fn program(&mut self, _: RecordKind, bank: Bank, offset: BytePosition, bytes: &[u8]) -> Result<(), MediumError> {
        let request = self.requests; self.requests += 1;
        if self.fail == Some(request) { return Err(MediumError(-63)); }
        let start = bank as usize * BLOCK_BYTES + offset.0 as usize;
        for (destination, value) in self.banks[start..start + bytes.len()].iter_mut().zip(bytes) { *destination &= value; }
        Ok(())
    }
    fn erase(&mut self, _: RecordKind, bank: Bank) -> Result<(), MediumError> {
        let request = self.requests; self.requests += 1;
        if self.fail == Some(request) { return Err(MediumError(-63)); }
        self.banks[bank as usize * BLOCK_BYTES..(bank as usize + 1) * BLOCK_BYTES].fill(0xff); Ok(())
    }
}
struct Stage([u8; RECORD_BYTES]);
impl Staging for Stage { fn bytes(&mut self, _: RecordKind) -> &mut [u8] { &mut self.0 } }
struct Driver { retained: [u8; HEADER_BYTES], failed: bool }
impl HeaderBackupDriver for Driver {
    fn context(&mut self) -> ExecutionContext { ExecutionContext::Task }
    fn acquire(&mut self, _: BackupSemaphore) {}
    fn release(&mut self, _: BackupSemaphore) {}
    fn diagnostic(&mut self, _: BackupDiagnostic) { panic!("task context diagnostic"); }
    fn admit(&mut self, _: RequestDirection) -> Result<(), DeviceStatus> { Ok(()) }
    fn write_backup(&mut self, key: BackupKey, value: BackupValue, _: BackupWriteMode) -> DeviceStatus {
        if self.failed { return DeviceStatus(-63); }
        self.retained[usize::from(key.0.checked_sub(0x0f).expect("backup key"))] = value.0; DeviceStatus(0)
    }
    fn store_request_status(&mut self, _: RequestStatus) {}
    fn report_failure(&mut self, _: RequestStatus) { panic!("unexpected model request failure"); }
}
fn word(bytes: &[u8]) -> u32 { u32::from_le_bytes(bytes[..4].try_into().unwrap()) }
fn main() -> io::Result<()> {
    let mut bytes = Vec::new(); io::stdin().take(1024 * 1024).read_to_end(&mut bytes)?;
    assert!(bytes.len() >= 11 && &bytes[..4] == b"PL2\x01");
    let version = FirmwareVersion { major: bytes[4], minor: bytes[5] };
    let fail = match word(&bytes[6..10]) { u32::MAX => None, step => Some(step) };
    let backup_failed = match bytes[10] { 0 => false, 1 => true, _ => panic!("backup failure selector") };
    let (working, rest) = bytes[11..].split_at_mut(RECORD_BYTES);
    let (snapshot, rest) = rest.split_at_mut(RECORD_BYTES);
    let (live, rest) = rest.split_at_mut(BACKING_BYTES);
    let (banks, operations) = rest.split_at_mut(BLOCK_BYTES * 2);
    assert_eq!(operations.len() % 9, 0);
    let working = unsafe { &mut *(working.as_mut_ptr() as *mut Area2Record) };
    let snapshot = unsafe { &mut *(snapshot.as_mut_ptr() as *mut Area2Record) };
    let live: &mut [u8; BACKING_BYTES] = live.try_into().unwrap();
    let mut medium = Medium { banks, fail, requests: 0 };
    let mut stage = Stage([0xff; RECORD_BYTES]);
    let mut cache = CacheState::new();
    let mut block: Box<[[u8; BLOCK_BYTES]; 2]> = vec![[0; BLOCK_BYTES]; 2].into_boxed_slice().try_into().ok().unwrap();
    let mut service = Service::new(); service.open(&mut stage).unwrap();
    let area2 = service.register(RecordKind::Kind2, RecordBytes(512), RecordCount(62), SequenceOffset(0), Area2Codec).unwrap();
    let area3 = service.register(RecordKind::Kind3, RecordBytes(512), RecordCount(62), SequenceOffset(0), Area2Codec).unwrap();
    let keys = core::array::from_fn(|index| BackupKey(0x0f + index as u16));
    let mut shadow = [0x35; HEADER_BYTES];
    let mut backing = SharedBacking::without_export_residue(live, &mut shadow, &keys, Driver { retained: [0x35; HEADER_BYTES], failed: backup_failed });
    let mut output = io::BufWriter::new(io::stdout().lock());
    let saves = operations.chunks_exact(9).filter(|operation| operation[0] == 2).count();
    output.write_all(&(saves as u32).to_le_bytes())?;
    for operation in operations.chunks_exact(9) {
        let first = word(&operation[1..5]); let second = word(&operation[5..9]);
        match operation[0] {
            0 => backing.live_mut()[first as usize] = u8::try_from(second).expect("live byte"),
            1 => working.0[..2].copy_from_slice(&u16::try_from(first).expect("sequence").to_le_bytes()),
            2 => {
                let selector = match first { 0 => SaveSelector::Snapshot, 1 => SaveSelector::Close, _ => panic!("selector") };
                let result = Area2Lifecycle::adopt(working, snapshot, area2, area3).save(selector, &mut backing, version, &mut service, &mut Entry::new(&mut medium, &mut stage, &mut block, &mut cache));
                output.write_all(&result.completion.abi().to_le_bytes())?;
            }
            _ => panic!("operation"),
        }
    }
    output.write_all(&[u8::from(service.get_mut(area2).is_some()), u8::from(service.get_mut(area3).is_some())])?;
    output.write_all(&working.0)?; output.write_all(&snapshot.0)?; output.write_all(backing.live())?;
    output.write_all(medium.banks)?;
    output.write_all(&backing.driver().retained)?;
    output.flush()
}
