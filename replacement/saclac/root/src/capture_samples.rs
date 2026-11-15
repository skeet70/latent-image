#![forbid(unsafe_code)]

use xm1_power_lifecycle::{record_address_space::GuestAddress, record_hardware_requests};
use xm1_saclac_calc::adjust::motion::sampling::PositionSamples;
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::pipeline_work::{PlanBlock, SampleSnapshot};

const STATE: u32 = 0x016f_1028;
const POSITIONS: u32 = STATE + 0xf4;
const SNAPSHOT: u32 = 0x016f_137c;

// f209c4/d0 uses this60-byte request synchronously. No helper retains its address.
// Weights and sample requests share the scratch only across serialized task43 calls.
pub(crate) const REQUEST: GuestAddress = GuestAddress::new(0x016b_5484);

pub(crate) struct Samples<M> { machine: M, count: usize }
impl<M: Machine> Samples<M> {
    pub(crate) fn new(machine: M) -> Self {
        let count = usize::from(machine.read_u16(STATE + 4));
        Self { machine, count }
    }
}
impl<M: Machine> PositionSamples for Samples<M> {
    fn count(&self) -> usize { self.count }
    fn position(&self, index: usize) -> i16 {
        assert!(index < self.count);
        position(&self.machine, index as u32)
    }
}

pub(crate) fn position(machine: &impl Machine, index: u32) -> i16 {
    machine.read_u16(POSITIONS + 2 * index) as i16
}

pub(crate) fn idle(machine: &impl Machine) -> bool {
    // fef920..928 returns1 only for state0, not for every state other than1.
    machine.read_u32(STATE) == 0
}

pub(crate) fn snapshot(machine: &impl Machine) -> SampleSnapshot {
    let mut snapshot = SampleSnapshot::default();
    for (index, bytes) in snapshot.0.chunks_exact_mut(4).enumerate() {
        bytes.copy_from_slice(&machine.read_u32(SNAPSHOT + 4 * index as u32).to_le_bytes());
    }
    snapshot
}

pub(crate) fn take<M: Machine + Clone>(machine: &M, level: u16, targets: &[PlanBlock; 5]) -> Option<i16> {
    let mut writer = machine.clone();
    writer.write_u32(REQUEST.get(), u32::from(level));
    for (index, target) in targets.iter().enumerate() {
        writer.write_u32(REQUEST.get() + 4 + 4 * index as u32, target.0);
    }
    let taken = record_hardware_requests::helper_feaa24(&mut crate::calibration_inputs::runtime(machine), REQUEST)
        .expect("capture sample memory");
    if taken == 0 { None } else { Some(machine.read_u16(REQUEST.get() + 2) as i16) }
}
