#![forbid(unsafe_code)]

//! Lens control queries, the lens notices, the relative move and the lens synchronization.

use xm1_saclac_interface::shared::RelativeMove;
use xm1_saclac_interface::stock::{self, StockCallbackWord};
use xm1_saclac_interface::{Completion, LensDc, LensDcOperation, LensDcRequest};

/// The three lens control probes, in the order the autofocus reply queries them (0x76d9a8,
/// 0x76da38, 0x76dba4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensProbe { First, Second, Third }

/// A probe is overridden when the lens control's runtime holds its override word at 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeState { Clear, Reported, Overridden }

/// The lens control state other tasks own.
pub trait LensControl {
    fn probe(&mut self, probe: LensProbe) -> ProbeState;
    /// 0x80 when the second probe is overridden or the lens control has no value (0x76da48).
    fn value(&mut self) -> u32;
    /// The runtime's step word (0x76fc4c).
    fn step(&mut self) -> u32;
    /// 0x76dac4.
    fn prefers_relative(&mut self) -> bool;
    /// The upper limit of `zone - 1`, as a signed halfword (0x772f28).
    fn zone_limit(&mut self, zone: u8) -> i16;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Notice { First, Second, Third }

/// Lens bounds as the bounds notifier receives them, after the caller's detail word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensBounds(pub [u32; 3]);

/// The notices and the bounds notifier in the lens control block other tasks share.
pub trait LensNotices {
    fn notice(&self, notice: Notice) -> u32;
    fn clear_notices(&mut self);
    fn notifier_enabled(&self) -> bool;
    fn notifier(&self) -> Option<StockCallbackWord>;
    fn enter_notifier(&mut self, notifier: StockCallbackWord, detail: u32, bounds: LensBounds);
}

/// Enters the bounds notifier when one is set and enabled (0xefb9f0).
pub fn notify_bounds(detail: u32, bounds: LensBounds, notices: &mut impl LensNotices) {
    if notices.notifier_enabled() {
        if let Some(notifier) = notices.notifier() {
            notices.enter_notifier(notifier, detail, bounds);
        }
    }
}

/// The zone a lens value falls in (0x772ff8).
pub fn zone(value: i32, lens: &mut impl LensControl) -> u8 {
    if value == 0 {
        return 0;
    }
    (2..=10).find(|zone| value < i32::from(lens.zone_limit(*zone))).map_or(10, |zone| zone - 1)
}

/// Posts a relative move to lensdc (0x76e1ac). `completions` are the queue-43 completions the
/// move's callback posts.
pub fn move_relative<L: LensDc>(request: &RelativeMove, completions: &[Completion], lens: &mut L) -> Result<(), L::Error> {
    let bytes = stock::shared::relative_move(request);
    lens.request(LensDcRequest { operation: LensDcOperation::Request0b, parameters: stock::parameters(&bytes), completions })
}

/// The task blocked in a lens synchronization and the callback that releases it.
pub trait Rendezvous {
    /// Records the calling task as the one to release and returns the callback lensdc enters to
    /// release it.
    fn arm(&mut self) -> StockCallbackWord;
    fn wait(&mut self);
}

/// Asks lensdc to synchronize and blocks the calling task until lensdc enters the rendezvous
/// callback (0xf472bc).
pub fn synchronize<L: LensDc>(lens: &mut L, rendezvous: &mut impl Rendezvous) -> Result<(), L::Error> {
    let bytes = stock::shared::lens_synchronize(rendezvous.arm());
    lens.request(LensDcRequest { operation: LensDcOperation::Request17, parameters: stock::parameters(&bytes), completions: &[] })?;
    rendezvous.wait();
    Ok(())
}
