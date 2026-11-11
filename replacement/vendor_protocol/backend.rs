pub const CONFIG_BYTES: u32 = 0x420000;
pub const DIAGNOSTIC_BASE: u32 = 0x426000;
pub const DIAGNOSTIC_BYTES: u32 = 0x8000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendError { Range, Capacity, Disabled, InvalidCommand, Unavailable, Medium(i32) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigReadMode { Cached, Encoded }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigRegion { Configuration, Diagnostic }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigSpan { region: ConfigRegion, offset: u32, length: u32 }
impl ConfigSpan {
    pub fn new(offset: u32, length: u32) -> Result<Self, BackendError> {
        let last = match length { 0 => offset, _ => offset.checked_add(length - 1).ok_or(BackendError::Range)? };
        let (region, relative) = if offset < CONFIG_BYTES && last < CONFIG_BYTES {
            (ConfigRegion::Configuration, offset)
        } else if offset >= DIAGNOSTIC_BASE && last < DIAGNOSTIC_BASE + DIAGNOSTIC_BYTES {
            (ConfigRegion::Diagnostic, offset - DIAGNOSTIC_BASE)
        } else { return Err(BackendError::Range); };
        Ok(Self { region, offset: relative, length })
    }
    pub const fn region(self) -> ConfigRegion { self.region }
    pub const fn offset(self) -> u32 { self.offset }
    pub const fn length(self) -> u32 { self.length }
}
pub struct ConfigImages<'a> {
    first: &'a [u8],
    second: &'a [u8],
    diagnostic: &'a [u8],
    encoded: &'a mut [u8],
    encoded_diagnostic: &'a [u8],
}
impl<'a> ConfigImages<'a> {
    pub fn byte(&self, offset: u32) -> Option<u8> { self.encoded.get(offset as usize).copied() }
    pub fn encoded_bytes(&self) -> &[u8] { self.encoded }
    pub fn encoded_region_mut(&mut self, span: ConfigSpan) -> Result<&mut [u8], BackendError> {
        if span.region != ConfigRegion::Configuration { return Err(BackendError::Range); }
        let start = span.offset as usize;
        let end = start.checked_add(span.length as usize).ok_or(BackendError::Range)?;
        self.encoded.get_mut(start..end).ok_or(BackendError::Capacity)
    }
    pub fn write(&mut self, span: ConfigSpan, input: &[u8]) -> Result<(), BackendError> {
        if span.region != ConfigRegion::Configuration || input.len() != span.length as usize { return Err(BackendError::Range); }
        let start = span.offset as usize;
        let end = start.checked_add(input.len()).ok_or(BackendError::Range)?;
        self.encoded.get_mut(start..end).ok_or(BackendError::Capacity)?.copy_from_slice(input);
        Ok(())
    }
    pub fn adopt(first: &'a [u8], second: &'a [u8], diagnostic: &'a [u8], encoded: &'a mut [u8], encoded_diagnostic: &'a [u8]) -> Self {
        Self { first, second, diagnostic, encoded, encoded_diagnostic }
    }
    pub fn debug_enabled(&self) -> Result<bool, BackendError> { self.byte(0xf7).map(|value| value != 0).ok_or(BackendError::Capacity) }
    pub fn read(&self, span: ConfigSpan, mode: ConfigReadMode, output: &mut [u8]) -> Result<(), BackendError> {
        let length = span.length as usize;
        let output = output.get_mut(..length).ok_or(BackendError::Capacity)?;
        if length == 0 && mode == ConfigReadMode::Encoded { return Ok(()); }
        let offset = span.offset as usize;
        let end = offset.checked_add(length).ok_or(BackendError::Range)?;
        match (span.region, mode) {
            (ConfigRegion::Configuration, ConfigReadMode::Cached) => {
                let split = 0x10000;
                if end <= split { output.copy_from_slice(self.first.get(offset..end).ok_or(BackendError::Capacity)?); }
                else if offset >= split { output.copy_from_slice(self.second.get(offset-split..end-split).ok_or(BackendError::Capacity)?); }
                else {
                    let first = self.first.get(offset..split).ok_or(BackendError::Capacity)?;
                    let second = self.second.get(..end-split).ok_or(BackendError::Capacity)?;
                    output[..first.len()].copy_from_slice(first);
                    output[first.len()..].copy_from_slice(second);
                }
            }
            (region, mode) => {
                let source = match (region, mode) {
                    (ConfigRegion::Configuration, _) => self.encoded,
                    (ConfigRegion::Diagnostic, ConfigReadMode::Cached) => self.diagnostic,
                    (ConfigRegion::Diagnostic, ConfigReadMode::Encoded) => self.encoded_diagnostic,
                };
                output.copy_from_slice(source.get(offset..end).ok_or(BackendError::Capacity)?);
            }
        }
        Ok(())
    }
}

pub struct DebugRequest<'a> { pub command: &'a [u8], pub arguments: &'a [u8] }
impl<'a> DebugRequest<'a> {
    pub fn parse(input: &'a [u8]) -> Result<Self, BackendError> {
        let space = input.iter().position(|byte| *byte == b' ').ok_or(BackendError::InvalidCommand)?;
        if space == 0 || input.len() < space + 3 || space >= 256 { return Err(BackendError::InvalidCommand); }
        let arguments = &input[space+1..input.len()-1];
        if arguments.len() >= 256 { return Err(BackendError::Capacity); }
        Ok(Self { command: &input[..space], arguments })
    }
}

pub struct DebugMemory<'a> { base: u32, bytes: &'a mut [u8] }
impl<'a> DebugMemory<'a> {
    pub fn adopt(base: u32, bytes: &'a mut [u8]) -> Result<Self, BackendError> {
        let length = u32::try_from(bytes.len()).map_err(|_| BackendError::Range)?;
        base.checked_add(length).ok_or(BackendError::Range)?;
        Ok(Self { base, bytes })
    }
    fn range(&self, address: u32, length: usize) -> Result<core::ops::Range<usize>, BackendError> {
        let offset = address.checked_sub(self.base).ok_or(BackendError::Range)? as usize;
        let end = offset.checked_add(length).ok_or(BackendError::Range)?;
        if end > self.bytes.len() { return Err(BackendError::Range); }
        Ok(offset..end)
    }
    pub fn read(&self, enabled: bool, address: u32, output: &mut [u8]) -> Result<(), BackendError> {
        if !enabled { return Err(BackendError::Disabled); }
        output.copy_from_slice(&self.bytes[self.range(address, output.len())?]);
        Ok(())
    }
    pub fn write(&mut self, enabled: bool, address: u32, input: &[u8]) -> Result<(), BackendError> {
        if !enabled { return Err(BackendError::Disabled); }
        let range = self.range(address, input.len())?;
        self.bytes[range].copy_from_slice(input);
        Ok(())
    }
}
