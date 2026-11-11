//! Timing and command policy: image 4 at 0x006b8d84, 0x006b8db8, 0x006b8e54,
//! 0x006b93a4 and 0x006b94a0, source pin 55436fc85c0560dd252b0410af6f2427fe198cd3.

use xm1_lens_transport::{FailureMode, TimingParameters, TimingProfile};
use xm1_lens_transport::frame::RetryLimit;
use xm1_lens_transport::wire::TimerTicks;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensModelIdentifier(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawTimingGroup(pub [u8; 5]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupportedCommands(pub [u8; 64]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryConfiguration { pub delay: u8, pub recovery_limit: u8, pub busy_limit: u8, pub diagnostic: u8 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensSourceConfiguration {
    pub initial: RawTimingGroup,
    pub baseline: RawTimingGroup,
    pub primary: RawTimingGroup,
    pub secondary: RawTimingGroup,
    pub recovery: RecoveryConfiguration,
    pub supported: SupportedCommands,
    pub extended_supported: SupportedCommands,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinishedConfiguration { pub primary: TimingProfile, pub secondary: TimingProfile, pub supported: SupportedCommands, pub restrict_extended_mode: bool }
fn configured(value: u8, fallback: u32) -> u32 { if value == 0 || value == 255 { fallback } else { u32::from(value) } }
impl RawTimingGroup {
    fn durations(self) -> [u32; 5] { [configured(self.0[0], 50) * 10, configured(self.0[1], 50) * 10, configured(self.0[2], 20) * 100, configured(self.0[3], 20) * 100, configured(self.0[4], 20) * 100] }
}
impl LensSourceConfiguration {
    fn timing(self, durations: [u32; 5]) -> TimingParameters {
        fn ticks(value: u32) -> TimerTicks { match TimerTicks::new(value) { Some(value) => value, None => unreachable!() } }
        TimingParameters { command: ticks(durations[0]), normal: ticks(durations[1]), deferred: ticks(durations[2]), accepted_page: ticks(durations[3]), rejected_page: ticks(durations[4]), recovery: ticks(configured(self.recovery.delay, 30) * 100), recovery_limit: RetryLimit::new(configured(self.recovery.recovery_limit, 3)), busy_limit: RetryLimit::new(configured(self.recovery.busy_limit, 10)), failure_mode: if self.recovery.diagnostic == 1 { FailureMode::DiagnosticBlink } else { FailureMode::Notify } }
    }
    pub fn initial_timing(self) -> TimingParameters { self.timing(self.initial.durations()) }
    pub fn finish(self, model: LensModelIdentifier) -> FinishedConfiguration {
        let baseline = self.baseline.durations();
        let primary = self.primary.durations();
        let secondary = self.secondary.durations();
        let mut first = [0; 5];
        let mut second = [0; 5];
        for index in 0..5 { first[index] = baseline[index].max(primary[index]); second[index] = baseline[index].max(secondary[index]); }
        let mut supported = self.supported;
        for (index, byte) in supported.0.iter_mut().enumerate() {
            let mask = match model.0 {
                100 | 101 | 110 => {
                    let mut mask = 0u8;
                    for bit in 0..8 {
                        let command = index * 8 + bit;
                        let enabled = matches!(command, 0 | 18..=22 | 24 | 31..=34 | 36 | 38..=39 | 48 | 63..=64 | 80 | 84..=86 | 88 | 96..=98 | 160 | 168 | 192 | 200..=201 | 255 | 272..=282 | 288..=289 | 336..=340 | 352)
                            || (model.0 != 100 && command == 169) || (model.0 == 110 && command == 50);
                        if enabled { mask |= 1 << bit; }
                    }
                    mask
                }
                200.. => self.extended_supported.0[index],
                _ => 255,
            };
            *byte &= mask;
        }
        FinishedConfiguration { primary: self.timing(first).profile(), secondary: self.timing(second).profile(), supported, restrict_extended_mode: model.0 == 101 }
    }
    #[cfg(target_arch = "arm")]
    /// The Power configuration base and the addressed configuration extents must be published.
    pub unsafe fn read_power() -> Self {
        extern "C" { fn xm1_power_configuration_byte(offset: u32) -> u32; }
        fn source(offset: u32) -> u8 { unsafe { xm1_power_configuration_byte(offset) as u8 } }
        fn group(addresses: [u32; 5]) -> RawTimingGroup { RawTimingGroup(addresses.map(source)) }
        let mut supported = [0; 64];
        let mut extended_supported = [0; 64];
        for index in 0..64 { supported[index] = source(0x4000 + index as u32); extended_supported[index] = source(0x3e00bf + index as u32); }
        Self {
            initial: group([0x4053, 0x4054, 0x4056, 0x4057, 0x4058]),
            baseline: group([0x405a, 0x405b, 0x405d, 0x405e, 0x405f]),
            primary: group([0x3e0090, 0x3e0091, 0x3e0094, 0x3e0095, 0x3e0096]),
            secondary: group([0x3e0098, 0x3e0099, 0x3e0094, 0x3e0095, 0x3e0096]),
            recovery: RecoveryConfiguration { delay: source(0x4061), recovery_limit: source(0x4062), busy_limit: source(0x4063), diagnostic: source(0x4051) },
            supported: SupportedCommands(supported), extended_supported: SupportedCommands(extended_supported),
        }
    }
}
