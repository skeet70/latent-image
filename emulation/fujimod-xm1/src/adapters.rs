//! The driver's adapter devices for the run document's `adapter` nodes.

use crate::{sd_host, ui_conversion};
use fujimod_emulator_devices::Device;
use fujimod_emulator_devices::graph::Mapping;
use fujimod_emulator_exr2_plan::{Adapter, AdapterFactory, Resources};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Implementation {
    UiConversion,
    SdHost,
}

impl Implementation {
    fn parse(name: &str) -> Option<Self> {
        match name {
            ui_conversion::IMPLEMENTATION => Some(Self::UiConversion),
            sd_host::IMPLEMENTATION => Some(Self::SdHost),
            _ => None,
        }
    }
}

/// Builds the declared UI conversion and the retained SD host; refuses every other implementation.
pub struct Xm1Adapters;

impl AdapterFactory for Xm1Adapters {
    fn construct(
        &mut self,
        name: &str,
        spec: &Adapter,
        windows: &[Mapping],
        resources: &Resources,
        _: &BTreeMap<String, usize>,
    ) -> Result<Box<dyn Device>, String> {
        match Implementation::parse(&spec.implementation) {
            Some(Implementation::UiConversion) => ui_conversion::construct(name, spec, windows),
            Some(Implementation::SdHost) => sd_host::construct(name, spec, windows, resources),
            None => Err(format!("{name} requires unavailable adapter {}", spec.implementation)),
        }
    }
}
