#![forbid(unsafe_code)]

use xm1_saclac_imaging::capture::configuration::CaptureConfiguration;
use xm1_saclac_interface::{area_measurement::{AreaEntry, MeasuredAreas}, calc::SharedAddress, calculation::ReferenceRequest};
use crate::{exposure_memory::AddressWidth, stock::acquisition::Aligned};

/// Factory-owned storage, placed before requests and retained until their peer completions.
/// Reference calculations and area measurements have separate in-place input lifetimes.
pub struct Workspace {
    pub(crate) configuration: CaptureConfiguration,
    reference: Aligned<{ ReferenceRequest::BYTES }>,
    areas: Aligned<{ 3 * AreaEntry::BYTES }>,
}
impl Default for Workspace {
    fn default() -> Self {
        Self { configuration: CaptureConfiguration::default(), reference: Aligned([0; ReferenceRequest::BYTES]), areas: Aligned([0; 3 * AreaEntry::BYTES]) }
    }
}
impl Workspace {
    pub(crate) fn stage_reference(&mut self, request: &ReferenceRequest) -> Result<SharedAddress, AddressWidth> {
        let at = address(&self.reference.0)?;
        self.reference.0.copy_from_slice(request.bytes());
        Ok(at)
    }
    pub(crate) fn stage_areas(&mut self, areas: &MeasuredAreas) -> Result<SharedAddress, AddressWidth> {
        let at = address(&self.areas.0)?;
        for (out, area) in self.areas.0.chunks_exact_mut(AreaEntry::BYTES).zip(areas) { out.copy_from_slice(&area.0); }
        Ok(at)
    }
    pub fn statistics_ready(&self) -> bool { self.configuration.first_published() }
}
fn address(bytes: &[u8]) -> Result<SharedAddress, AddressWidth> {
    u32::try_from(bytes.as_ptr() as usize).map(SharedAddress).map_err(|_| AddressWidth)
}
