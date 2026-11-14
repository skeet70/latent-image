#![forbid(unsafe_code)]

//! Calc ports shared by the acquisition and focus searches.

use super::stock::{self, buffer, call, out, signed, w, Area};
use xm1_saclac_calc::adjust::{
    configuration::FrameInterval,
    focus_profile::LensResources,
    lens::{
        BiasTable, LensConfiguration, LensControl, LensSteps, ReferenceScale, Step, StepRequest,
    },
    motion::drive::{DriveKind, DriveLimits, DriveMode},
    peak::Logarithms,
    timing::{CaptureTiming, LevelScale},
};
use xm1_saclac_calc::exposure::ports::{Lens, ProfileAccess, Properties, Property};
use xm1_saclac_calc::profile::Profile;
use xm1_saclac_config::ports::{Derivations as DerivationPorts, ExportQuery};
use xm1_saclac_config::{
    channel, limits,
    table::{self, Row, Step as TableStep},
};
use xm1_saclac_profile_records::{Memory, ProfileRecords, StockEntries};
use xm1_saclac_shared::{
    settings::{Adjustments, Channel},
    stock::Machine,
};

pub struct Ports<'a, M, R, E> {
    pub machine: &'a M,
    pub records: &'a ProfileRecords<R>,
    pub entries: &'a mut E,
}
impl<M: Machine, R: Memory, E: StockEntries> ProfileAccess for Ports<'_, M, R, E> {
    type Records = R;
    type Entries = E;
    fn profile(&mut self) -> Profile<'_, R, E> {
        Profile::new(self.records, self.entries)
    }
}

/// The non-record services can be lent separately when a calc API takes several ports.
pub struct Tables<'a, M>(pub &'a M);
impl<M: Machine> Properties for Tables<'_, M> {
    fn property(&mut self, property: Property) -> u32 {
        u32::from(Area(self.0).byte(xm1_saclac_shared::settings::AdjustmentKey(property.key())))
    }
}
impl<M: Machine> Lens for Tables<'_, M> {
    fn level(&mut self, lens: u32, reference: u32) -> i16 {
        table::combined_word(&Area(self.0), TableStep(lens), Row(reference))
    }
    fn base_level(&mut self, lens: u32) -> i16 {
        table::primary_rounded_word(&Area(self.0), TableStep(lens))
    }
    fn reference_offset(&mut self, reference: u32) -> i16 {
        table::relative_word(&Area(self.0), Row(reference))
    }
    fn calibration_index(&mut self, lens: u32) -> u32 {
        table::column(&Area(self.0), TableStep(lens))
    }
}
impl<M: Machine> LensConfiguration for Tables<'_, M> {
    fn channel_value(&mut self, value: i32) -> u32 {
        channel::packed(&Area(self.0), Channel(value))
    }
    fn channel_curve(&mut self, value: i32) -> u32 {
        call(self.0, stock::CHANNEL_CURVE, &[value as u32])
    }
    fn bias(&mut self, resource: i32, kind: BiasTable) -> i16 {
        channel::bias(&Area(self.0), Channel(resource), bias_table(kind))
    }
    fn biased_position(&mut self, resource: i32, kind: BiasTable) -> i16 {
        channel::biased_step(&Area(self.0), Channel(resource), bias_table(kind))
    }
    fn resource_count(&mut self) -> u8 {
        channel::count(&Area(self.0)) as u8
    }
    fn selection_word(&mut self, channel: i32, flags: u32, offset: i16) -> u32 {
        call(
            self.0,
            stock::SELECTION_WORD,
            &[channel as u32, 0, flags, signed(offset)],
        )
    }
    fn remap(&mut self, word: u32, channel: i32, mode: u32) -> i32 {
        call(
            self.0,
            stock::REMAP,
            &[word, channel as u32, 0, mode, 0, 0, 1],
        ) as i32
    }
    fn position_limits(&mut self, key: u32) -> [i16; 2] {
        [
            limits::channel_limit(
                &Area(self.0),
                limits::ChannelLimit::Primary,
                Channel(key as i32),
            )
            .0,
            limits::channel_limit(
                &Area(self.0),
                limits::ChannelLimit::Secondary,
                Channel(key as i32),
            )
            .0,
        ]
    }
    fn mapped_value(&mut self, key: u32) -> u32 {
        u32::from(table::value(&Area(self.0), TableStep(key), Row(0)))
    }
}
const fn bias_table(kind: BiasTable) -> u32 {
    match kind {
        BiasTable::Area => 0,
        BiasTable::Descending => 1,
        BiasTable::Ascending => 2,
    }
}
impl<M: Machine> LensControl for Tables<'_, M> {
    fn position(&mut self) -> u16 {
        call(self.0, stock::LENS_POSITION, &[]) as u16
    }
    fn travel_margin(&mut self) -> u16 {
        Area(self.0).lens_adjustment()
    }
}
impl<M: Machine> ReferenceScale for Tables<'_, M> {
    fn reference_scale(&mut self, group: u16, entry: u8) -> u32 {
        call(
            self.0,
            stock::REFERENCE_SCALE,
            &[u32::from(group), u32::from(entry)],
        )
    }
}
impl<M: Machine> LensSteps for Tables<'_, M> {
    fn step(&mut self, request: StepRequest) -> Step {
        let mut derivations = Derivations::new(self.0);
        let (size, rate) = limits::selected_limit(
            &Area(self.0),
            &mut derivations,
            limits::LimitQuery {
                channel: Channel(request.resource as i32),
                first: request.index,
                value: u32::from(request.base),
                divisor: request.period,
                mode: limits::LimitMode::Bounded {
                    upper: i16::MAX,
                    lower: 0,
                },
                kind: kind(request.kind),
                side: side(request.mode),
                limit_mode: request.mode.step,
            },
        );
        Step { size, rate }
    }
    fn travel_distance(&mut self, resource: u32, index: u32, travel: i16) -> u16 {
        limits::scaled_value(
            &mut Derivations::new(self.0),
            Channel(resource as i32),
            index,
            signed(travel),
        )
    }
}
fn kind(kind: DriveKind) -> limits::Kind {
    if kind.0 == 0 {
        limits::Kind::First
    } else {
        limits::Kind::Second
    }
}
fn side(mode: DriveMode) -> limits::Side {
    if mode.direction == 1 {
        limits::Side::One
    } else {
        limits::Side::Other
    }
}
impl<M: Machine> DriveLimits for Tables<'_, M> {
    fn first_ramp_speed(&mut self, k: DriveKind, m: DriveMode) -> u32 {
        limits::limit(
            &Area(self.0),
            limits::Limit::ShiftedHalf,
            kind(k),
            side(m),
            m.step,
        )
    }
    fn first_ramp_acceleration(&mut self, k: DriveKind, m: DriveMode) -> u32 {
        limits::limit(&Area(self.0), limits::Limit::Word, kind(k), side(m), m.step)
    }
    fn second_ramp_acceleration(&mut self, k: DriveKind, m: DriveMode) -> u32 {
        limits::limit(
            &Area(self.0),
            limits::Limit::OptionalWord,
            kind(k),
            side(m),
            m.step,
        )
    }
    fn slowest_ramped_speed(&mut self) -> u32 {
        limits::shifted_second_floor(&Area(self.0))
    }
}
impl<M: Machine> FrameInterval for Tables<'_, M> {
    fn frame_interval(&mut self, timing: u32) -> Option<u32> {
        let mut bytes = [0; 4];
        (buffer(
            self.0,
            stock::FRAME_INTERVAL,
            &[w(timing), out(0)],
            &mut bytes,
        ) == 0)
            .then(|| stock::word(&bytes, 0))
    }
}
impl<M: Machine> CaptureTiming for Tables<'_, M> {
    fn readout_start(&mut self, source: u32) -> u32 {
        let mut bytes = [0; 12];
        buffer(
            self.0,
            stock::READOUT_START,
            &[w(source), out(0), w(1)],
            &mut bytes,
        );
        stock::word(&bytes, 4)
    }
    fn reference_line(&mut self, source: u32) -> u16 {
        call(self.0, stock::REFERENCE_LINE, &[source]) as u16
    }
    fn line_period(&mut self, source: u32) -> Option<u32> {
        let mut bytes = [0; 8];
        (buffer(self.0, stock::LINE_PERIOD, &[w(source), out(0)], &mut bytes) == 0)
            .then(|| stock::word(&bytes, 0))
    }
}
impl<M: Machine> LevelScale for Tables<'_, M> {
    fn linear(&mut self, level: i16) -> u32 {
        call(self.0, stock::LINEAR, &[signed(level)])
    }
    fn log2_difference(&mut self, value: u32, reference: u32) -> i16 {
        call(self.0, stock::LOG_DIFFERENCE, &[value, reference]) as i16
    }
}
impl<M: Machine> LensResources for Tables<'_, M> {
    fn resource(&mut self) -> Option<u32> {
        let resource = call(self.0, stock::LENS_IDENTITY, &[]);
        ((resource as i32) >= 0).then_some(resource)
    }
    fn reference(&mut self) -> u32 {
        call(self.0, stock::LENS_REFERENCE, &[])
    }
    fn flags(&mut self) -> u32 {
        call(self.0, stock::LENS_FLAGS, &[])
    }
    fn channel_bias(&mut self, resource: u32) -> i32 {
        i32::from(channel::bias(&Area(self.0), Channel(resource as i32), 0))
    }
}
impl<M: Machine> Logarithms for Tables<'_, M> {
    fn centi_log2(&mut self, value: u64) -> Option<u32> {
        (value != 0).then(|| {
            call(
                self.0,
                stock::LOGARITHM,
                &[value as u32, (value >> 32) as u32],
            )
        })
    }
}

macro_rules! forward {
    ($trait:ident { $(fn $name:ident(&mut self $(, $arg:ident: $ty:ty)*) -> $ret:ty;)* }) => {
        impl<M: Machine, R: Memory, E: StockEntries> $trait for Ports<'_, M, R, E> {
            $(fn $name(&mut self $(, $arg: $ty)*) -> $ret { $trait::$name(&mut Tables(self.machine) $(, $arg)*) })*
        }
    };
}
forward!(Properties { fn property(&mut self, property: Property) -> u32; });
forward!(Lens { fn level(&mut self, lens: u32, reference: u32) -> i16; fn base_level(&mut self, lens: u32) -> i16; fn reference_offset(&mut self, reference: u32) -> i16; fn calibration_index(&mut self, lens: u32) -> u32; });
forward!(LensConfiguration { fn channel_value(&mut self, channel: i32) -> u32; fn channel_curve(&mut self, channel: i32) -> u32; fn bias(&mut self, resource: i32, kind: BiasTable) -> i16; fn biased_position(&mut self, resource: i32, kind: BiasTable) -> i16; fn resource_count(&mut self) -> u8; fn selection_word(&mut self, channel: i32, flags: u32, offset: i16) -> u32; fn remap(&mut self, word: u32, channel: i32, mode: u32) -> i32; fn position_limits(&mut self, key: u32) -> [i16; 2]; fn mapped_value(&mut self, key: u32) -> u32; });
forward!(LensControl { fn position(&mut self) -> u16; fn travel_margin(&mut self) -> u16; });
forward!(ReferenceScale { fn reference_scale(&mut self, group: u16, entry: u8) -> u32; });
forward!(LensSteps { fn step(&mut self, request: StepRequest) -> Step; fn travel_distance(&mut self, resource: u32, index: u32, travel: i16) -> u16; });
forward!(DriveLimits { fn first_ramp_speed(&mut self, kind: DriveKind, mode: DriveMode) -> u32; fn first_ramp_acceleration(&mut self, kind: DriveKind, mode: DriveMode) -> u32; fn second_ramp_acceleration(&mut self, kind: DriveKind, mode: DriveMode) -> u32; fn slowest_ramped_speed(&mut self) -> u32; });
forward!(FrameInterval { fn frame_interval(&mut self, timing: u32) -> Option<u32>; });
forward!(CaptureTiming { fn readout_start(&mut self, source: u32) -> u32; fn reference_line(&mut self, source: u32) -> u16; fn line_period(&mut self, source: u32) -> Option<u32>; });
forward!(LevelScale { fn linear(&mut self, level: i16) -> u32; fn log2_difference(&mut self, value: u32, reference: u32) -> i16; });
forward!(LensResources { fn resource(&mut self) -> Option<u32>; fn reference(&mut self) -> u32; fn flags(&mut self) -> u32; fn channel_bias(&mut self, resource: u32) -> i32; });
forward!(Logarithms { fn centi_log2(&mut self, value: u64) -> Option<u32>; });

/// The config range API carries stock output words. The boundary substitutes the caller's buffer.
pub struct Derivations<'a, M> {
    pub machine: &'a M,
    pub range: Option<&'a mut [u8; 4]>,
}
impl<'a, M> Derivations<'a, M> {
    pub fn new(machine: &'a M) -> Self {
        Self {
            machine,
            range: None,
        }
    }
}
impl<M: Machine> DerivationPorts for Derivations<'_, M> {
    fn export_word(&mut self, channel: Channel, q: ExportQuery) -> i16 {
        let mut bytes = [0; 2];
        buffer(
            self.machine,
            stock::EXPORT_WORD,
            &[
                w(channel.0 as u32),
                w(q.selector),
                w(q.first),
                w(q.second),
                out(0),
                w(0),
            ],
            &mut bytes,
        );
        stock::half(&bytes, 0) as i16
    }
    fn adjust_direction(&mut self, word: i16, direction: u32, mode: u32) -> i16 {
        call(
            self.machine,
            stock::ADJUST_DIRECTION,
            &[signed(word), direction, mode],
        ) as i16
    }
    fn scaled_limit(&mut self, channel: Channel, first: u32, scale: u32) -> u16 {
        call(
            self.machine,
            stock::SCALED_LIMIT,
            &[channel.0 as u32, first, scale],
        ) as u16
    }
    fn channel_scale(&mut self, channel: Channel) -> u16 {
        call(self.machine, stock::CHANNEL_CURVE, &[channel.0 as u32]) as u16
    }
    fn table_scale(&mut self, step: u32, channel: Channel) -> u16 {
        call(self.machine, stock::TABLE_SCALE, &[step, channel.0 as u32]) as u16
    }
    fn curve_position(&mut self, level: i16, channel: Channel) -> u32 {
        call(
            self.machine,
            stock::CURVE_POSITION,
            &[signed(level), channel.0 as u32],
        )
    }
    fn curve_value(&mut self, position: u32, channel: Channel) -> i16 {
        call(
            self.machine,
            stock::CURVE_VALUE,
            &[position, channel.0 as u32],
        ) as i16
    }
    fn channel_word(&mut self, channel: Channel, selector: u32, table: u32) -> i16 {
        call(
            self.machine,
            stock::CHANNEL_WORD,
            &[channel.0 as u32, selector, table],
        ) as i16
    }
    fn reference_word(&mut self, mode: u32, channel: Channel) -> i16 {
        let mut bytes = [0; 2];
        buffer(
            self.machine,
            stock::REFERENCE_WORD,
            &[w(0), w(mode), w(channel.0 as u32), w(0), w(0), w(0), out(0)],
            &mut bytes,
        );
        stock::half(&bytes, 0) as i16
    }
    fn apply_reference(&mut self, word: i16, channel: Channel) {
        call(
            self.machine,
            stock::APPLY_REFERENCE,
            &[signed(word), channel.0 as u32, 0, u32::MAX, 1],
        );
    }
    fn lens_table(&mut self, selector: u32, first: u32, second: u32) -> u32 {
        call(self.machine, stock::LENS_TABLE, &[selector, first, second])
    }
    fn lens_limits(&mut self, table: u32, words: [u32; 11]) -> u32 {
        let mut args = [w(0); 12];
        args[0] = w(table);
        for (out, &word) in args[1..].iter_mut().zip(&words) {
            *out = w(word);
        }
        if let Some(bytes) = self.range.as_deref_mut() {
            args[5] = out(0);
            args[6] = out(2);
            buffer(self.machine, stock::LENS_LIMITS, &args, bytes)
        } else {
            buffer(self.machine, stock::LENS_LIMITS, &args, &mut [])
        }
    }
}
