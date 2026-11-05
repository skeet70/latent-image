//! The checkpointed first-start and ff80 attach procedure.
use crate::bridge::menu_value;
use crate::ff80::{Ff80, RendezvousInputs};
use crate::guest::RouteKey;
use crate::machine::{KeyPulse, Machine};
use crate::reset_observation::{AttachPhase, ResetObserver};
use serde_json::{Value, json};

const FIRST_START_SCREEN: u64 = 100_000_000;
const FIRST_START_PULSE: KeyPulse = KeyPulse::instructions(8_000_000, 8_000_000);
const LIVE_VIEW_SETTLE: u64 = 40_000_000;
const ATTACH_HOLD_PULSE: u64 = 8_000_000;

pub struct Attachment {
    pub pre_attach: Value,
    pub device: Vec<u8>,
    pub configuration: Vec<u8>,
}

pub fn attach(machine: Machine, rendezvous: RendezvousInputs, hold: Option<RouteKey>, mut observer: Option<&mut ResetObserver>)
    -> Result<(Ff80, Attachment), String> {
    let mut bridge = Ff80::new(machine, rendezvous);
    let mut boundary = |bridge: &Ff80, phase| observer.as_mut().map_or(Ok(()), |observer| observer.boundary(&bridge.machine, phase));
    bridge.advance(FIRST_START_SCREEN.saturating_sub(bridge.machine.instructions()))?;
    boundary(&bridge, AttachPhase::FirstStart)?;
    if rendezvous == RendezvousInputs::DialPushAndWriteProtect && !bridge.machine.card_present()? {
        return Err("the SD host does not read the inserted card as present".into());
    }
    for key in [RouteKey::Up; 12].into_iter().chain([RouteKey::Ok, RouteKey::Ok]) {
        bridge.key(key, FIRST_START_PULSE)?;
    }
    bridge.advance(LIVE_VIEW_SETTLE)?;
    boundary(&bridge, AttachPhase::LiveView)?;
    let pressed = bridge.machine.instructions();
    let mut pre_attach = json!({"instructions": pressed});
    if let Some(key) = hold {
        bridge.machine.press(key)?;
        bridge.advance(ATTACH_HOLD_PULSE)?;
    }
    let attached = bridge.machine.instructions();
    let (device, configuration) = bridge.enumerate()?;
    if let Some(key) = hold {
        bridge.machine.release_key(key)?;
        let released = bridge.machine.instructions();
        bridge.advance(ATTACH_HOLD_PULSE)?;
        pre_attach["attach_hold"] = json!({"key": key, "pressed_instructions": pressed,
            "attach_instructions": attached, "released_instructions": released,
            "menu_after_release": menu_value(bridge.machine.menu()?)});
    }
    boundary(&bridge, AttachPhase::Attached)?;
    Ok((bridge, Attachment { pre_attach, device, configuration }))
}
