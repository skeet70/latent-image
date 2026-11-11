use crate::backend::BackendError;
use record_store::Bank;

pub const RECORD_BYTES: usize = 0x800;
pub const BLOCK_BYTES: usize = 0x20000;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageOffset(u32);
impl PageOffset {
    pub fn new(offset: u32) -> Result<Self, BackendError> {
        if offset % RECORD_BYTES as u32 != 0 || offset > (BLOCK_BYTES - RECORD_BYTES) as u32 { return Err(BackendError::Range); }
        Ok(Self(offset))
    }
    pub const fn get(self) -> u32 { self.0 }
}
pub trait ConfigurationMedium {
    fn read(&mut self, bank: Bank, page: PageOffset, output: &mut [u8; RECORD_BYTES]) -> Result<(), BackendError>;
    fn program(&mut self, bank: Bank, page: PageOffset, input: &[u8; RECORD_BYTES]) -> Result<(), BackendError>;
    fn erase(&mut self, bank: Bank) -> Result<(), BackendError>;
}
pub struct HostConfigurationMedium<'a> { banks: &'a mut [[u8; BLOCK_BYTES]; 2] }
impl<'a> HostConfigurationMedium<'a> { pub fn adopt(banks: &'a mut [[u8; BLOCK_BYTES]; 2]) -> Self { Self { banks } } }
impl ConfigurationMedium for HostConfigurationMedium<'_> {
    fn read(&mut self, bank: Bank, page: PageOffset, output: &mut [u8; RECORD_BYTES]) -> Result<(), BackendError> {
        let start = page.0 as usize;
        output.copy_from_slice(&self.banks[bank as usize][start..start+RECORD_BYTES]);
        Ok(())
    }
    fn program(&mut self, bank: Bank, page: PageOffset, input: &[u8; RECORD_BYTES]) -> Result<(), BackendError> {
        let start = page.0 as usize;
        let destination = &mut self.banks[bank as usize][start..start+RECORD_BYTES];
        if destination.iter().zip(input).any(|(old, new)| old & new != *new) { return Err(BackendError::Medium(-1)); }
        for (old, new) in destination.iter_mut().zip(input) { *old &= *new; }
        Ok(())
    }
    fn erase(&mut self, bank: Bank) -> Result<(), BackendError> { self.banks[bank as usize].fill(0xff); Ok(()) }
}
#[derive(Clone, Copy)]
struct Scan { sequence: u32, latest: Option<PageOffset>, empty: Option<PageOffset> }
impl Scan { const fn new() -> Self { Self { sequence: 0, latest: None, empty: None } } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitOutcome { Clean, Written { bank: Bank, page: PageOffset }, Failed(BackendError) }
pub struct ExtraConfiguration<'a> { record: &'a mut [u8; RECORD_BYTES], scans: [Scan; 2], current: Bank, dirty: bool }
impl<'a> ExtraConfiguration<'a> {
    pub fn load(record: &'a mut [u8; RECORD_BYTES], scratch: &mut [u8; RECORD_BYTES], medium: &mut impl ConfigurationMedium) -> Result<Self, BackendError> {
        record.fill(0);
        let mut scans = [Scan::new(); 2];
        for bank in [Bank::Zero, Bank::One] {
            let scan = &mut scans[bank as usize];
            for offset in (0..BLOCK_BYTES).step_by(RECORD_BYTES) {
                let page = PageOffset::new(offset as u32)?;
                medium.read(bank, page, scratch)?;
                let sequence = u32::from_le_bytes(scratch[..4].try_into().unwrap());
                if sequence == u32::MAX { if scan.empty.is_none() { scan.empty = Some(page); } }
                else if sequence > scan.sequence { scan.sequence = sequence; scan.latest = Some(page); }
            }
        }
        let current = if scans[1].latest.is_some() && scans[1].sequence > scans[0].sequence { Bank::One } else { Bank::Zero };
        if let Some(page) = scans[current as usize].latest { medium.read(current, page, record)?; }
        Ok(Self { record, scans, current, dirty: false })
    }
    pub fn record(&self) -> &[u8; RECORD_BYTES] { self.record }
    pub fn edit(&mut self) -> &mut [u8; RECORD_BYTES] { self.dirty = true; self.record }
    pub fn reset_all(&mut self) { self.record[4..0x544].fill(0); self.dirty = true; }
    pub fn commit(&mut self, medium: &mut impl ConfigurationMedium) -> CommitOutcome {
        if !self.dirty { return CommitOutcome::Clean; }
        let sequence = u32::from_le_bytes(self.record[..4].try_into().unwrap()).wrapping_add(1) & 0x7fff;
        self.record[..4].copy_from_slice(&sequence.to_le_bytes());
        let (bank, page) = if let Some(page) = self.scans[self.current as usize].empty { (self.current, page) }
        else {
            let bank = self.current.next();
            if let Err(error) = medium.erase(bank) { return CommitOutcome::Failed(error); }
            (bank, PageOffset(0))
        };
        match medium.program(bank, page, self.record) {
            Ok(()) => CommitOutcome::Written { bank, page },
            Err(error) => CommitOutcome::Failed(error),
        }
    }
}
