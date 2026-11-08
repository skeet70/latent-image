use crate::{memory_view::{MemoryAddress, MemoryCore, static_span}, modal_permit::CoreId, provenance::Guest};
use fujimod_emulator::board::{factory::{Graph, MemoryMap}, observation::ObservationContext};

struct CompleteMapping { core: CoreId, start: MemoryAddress, end: MemoryAddress, physical: MemoryAddress }
pub(super) struct A32Mappings { spans: Vec<CompleteMapping> }
impl A32Mappings {
    pub(super) fn new(graph: &Graph, access: &ObservationContext) -> Result<Self, String> {
        graph.validate()?;
        if graph.mappings != access.mappings() || graph.processors.len() != access.core_count() {
            return Err("stack diagnostic graph differs from board construction".into());
        }
        for (core, processor) in graph.processors.iter().enumerate() {
            if processor.model != access.cpu(core)?.model() {
                return Err("stack diagnostic graph processor differs".into());
            }
        }
        let capacity = graph.mappings.len() + graph.decoders.iter()
            .flat_map(|decoder| &decoder.choices).map(|choice| choice.processors.len()).sum::<usize>();
        let mut prepared = Self { spans: Vec::with_capacity(capacity) };
        prepared.add_set(&graph.mappings)?;
        for decoder in &graph.decoders {
            for choice in &decoder.choices { prepared.add_set(&choice.processors)?; }
        }
        Ok(prepared)
    }
    fn add_set(&mut self, maps: &[MemoryMap]) -> Result<(), String> {
        for map in maps.iter().filter(|map| map.permissions & 1 != 0) {
            let start = MemoryAddress(map.address);
            let end = MemoryAddress(map.address.checked_add(map.size as u64).ok_or("stack diagnostic map extent overflows")?);
            let span = static_span(maps, MemoryCore(map.core), start..end)?;
            self.spans.push(CompleteMapping { core: CoreId(map.core), start, end, physical: span.physical });
        }
        Ok(())
    }
    pub(super) fn admits(&self, core: CoreId, address: Guest, canonical: Guest) -> bool {
        let start = u64::from(address.0);
        let end = start + 4;
        self.spans.iter().any(|span| span.core == core && span.start.0 <= start && end <= span.end.0
            && span.physical.0 + start - span.start.0 == u64::from(canonical.0))
    }
}
