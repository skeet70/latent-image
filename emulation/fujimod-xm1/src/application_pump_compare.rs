use super::*;

#[derive(Deserialize)]
struct GraphicsSave { engine: GraphicsEngine }
#[derive(Deserialize)]
struct GraphicsEngine {
    regs: Vec<(u32, u32)>, busy: bool, command_pending: Option<serde::de::IgnoredAny>,
    jobs_started: u64, jobs_completed: u64, bytes_completed: u64, acknowledgments: u64,
}
#[derive(Deserialize, Serialize)]
struct Pixels { width: u32, height: u32, rgb_bytes: u64, rgb_sha256: Digest }
#[derive(Deserialize, Serialize)]
struct CapturedArtifact { path: PathBuf, sha256: Digest, bytes: u64 }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "u8", into = "u8")]
enum Layer { Lower, Upper }
impl TryFrom<u8> for Layer {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> { match value { 0 => Ok(Self::Lower), 1 => Ok(Self::Upper), _ => Err("unknown native graphics layer") } }
}
impl From<Layer> for u8 { fn from(value: Layer) -> Self { match value { Layer::Lower => 0, Layer::Upper => 1 } } }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "u8", into = "u8")]
enum Channel { Zero, One, Two, Three }
impl TryFrom<u8> for Channel {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> { match value { 0 => Ok(Self::Zero), 1 => Ok(Self::One), 2 => Ok(Self::Two), 3 => Ok(Self::Three), _ => Err("unknown native graphics channel") } }
}
impl From<Channel> for u8 { fn from(value: Channel) -> Self { match value { Channel::Zero => 0, Channel::One => 1, Channel::Two => 2, Channel::Three => 3 } } }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum PlaneKind { Main, Graphics { layer: Layer, channel: Channel } }
#[derive(Deserialize, Serialize)]
struct Plane { #[serde(flatten)] kind: PlaneKind, native_source_address: u64, offset: u64, bytes: u64 }
#[derive(Deserialize, Serialize)]
struct Scanout {
    schema: u32, instructions: u64,
    #[serde(serialize_with = "node_name")]
    node: fujimod_xm1::run::NodeName,
    instance: usize, pixels: Pixels, png: CapturedArtifact,
    source_bytes: CapturedArtifact, source_planes: Vec<Plane>,
    native_scanout_state: Box<serde_json::value::RawValue>,
    native_frame_state: Box<serde_json::value::RawValue>,
}
fn node_name<S: serde::Serializer>(node: &fujimod_xm1::run::NodeName, serializer: S) -> Result<S::Ok, S::Error> { serializer.serialize_str(&node.0) }
#[derive(Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "String")]
struct NativeWord(u32);
impl TryFrom<String> for NativeWord {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        u32::from_str_radix(value.strip_prefix("0x").ok_or("native word lacks hexadecimal prefix")?, 16).map(Self).map_err(|error| error.to_string())
    }
}
#[derive(Deserialize)]
enum Backdrop {
    #[serde(rename = "declared-black-canvas-unmeasured")] Black,
    #[serde(rename = "declared-white-canvas-unmeasured")] White,
}
#[derive(Deserialize)]
struct NativeController { instances: Vec<NativeInstance>, backdrop_profile: Option<Backdrop> }
#[derive(Deserialize)]
struct NativeInstance { instance: usize, image_base: NativeWord, registers: BTreeMap<NativeWord, NativeWord> }
#[derive(Deserialize)]
struct MainGeometry { width: u32, height: u32, source: NativeWord, stride: u64 }
#[derive(Deserialize)]
struct FramePlane {
    layer: Layer, channel: Channel, width: u32, height: u32, source: NativeWord,
    origin: [u32; 2], visible: Option<bool>, stride_bytes: Option<u64>,
}
#[derive(Deserialize)]
struct NativeFrame {
    instance: usize, image_base: NativeWord, width: u32, height: u32,
    #[serde(default)] graphics: Vec<FramePlane>,
    #[serde(default)] declared_backdrop_used: bool,
    main: Option<MainGeometry>, source: Option<NativeWord>, stride: Option<u64>, canvas: Option<[u32; 2]>,
}
#[derive(Clone, Copy)]
struct Rectangle { x: u32, y: u32, width: u32, height: u32 }
fn full_planes(scanout: &Scanout) -> Result<(), Failure> {
    let controller: NativeController = parse(scanout.native_scanout_state.get().as_bytes(), Refusal::RenderCompletion)?;
    let frame: NativeFrame = parse(scanout.native_frame_state.get().as_bytes(), Refusal::RenderCompletion)?;
    require(scanout.instance < 2 && frame.instance == scanout.instance, Refusal::RenderCompletion, "native scanout instance differs")?;
    let mut instances = controller.instances.iter().filter(|instance| instance.instance == scanout.instance);
    let instance = instances.next().ok_or_else(|| refusal(Refusal::RenderCompletion, "native controller instance is missing"))?;
    require(instances.next().is_none(), Refusal::RenderCompletion, "native controller instance is duplicated")?;
    let base = [0xfff6b000, 0xfff6e000][scanout.instance];
    require(instance.image_base.0 == base && frame.image_base.0 == base, Refusal::RenderCompletion, "native controller base differs")?;
    let reg = |offset: u32| instance.registers.get(&NativeWord(base + offset)).map_or(0, |word| word.0);
    let canvas = [((reg(0x10) >> 16) & 0xfff) + 1, (reg(0x10) & 0xfff) + 1];
    let mut expected = Vec::with_capacity(9);
    let mut endpoint = 0u64;
    let mut add = |kind, address: u32, width: u32, height: u32, padding: u32, depth: u32| -> Result<u64, Failure> {
        let stride = u64::from(width + padding) * u64::from(depth);
        let bytes = u64::from(height - 1).checked_mul(stride).and_then(|value| value.checked_add(u64::from(width) * u64::from(depth))).ok_or_else(|| refusal(Refusal::RenderCompletion, "native plane geometry overflows"))?;
        require(u64::from(address).checked_add(bytes).is_some_and(|end| end <= 0x20000000), Refusal::RenderCompletion, "native plane exceeds physical DRAM")?;
        expected.push(Plane { kind, native_source_address: u64::from(address), offset: endpoint, bytes });
        endpoint = endpoint.checked_add(bytes).ok_or_else(|| refusal(Refusal::RenderCompletion, "native source extent overflows"))?;
        Ok(stride)
    };
    let main_enabled = reg(0) & 1 != 0;
    if main_enabled {
        let dimensions = reg(0x28);
        let width = ((dimensions >> 16) & 0x1fff) + 1;
        let height = (dimensions & 0x1fff) + 1;
        let address = reg(0x2c);
        require(dimensions & 0xe000e000 == 0 && address & 0xc0000000 == 0 && reg(0x30) & !0xfff == 0, Refusal::RenderCompletion, "native main geometry has unmodeled controls")?;
        let stride = add(PlaneKind::Main, address, width, height, reg(0x30), 2)?;
        if let Some(main) = &frame.main {
            require(main.width == width && main.height == height && main.source.0 == address && main.stride == stride, Refusal::RenderCompletion, "native main frame geometry differs from registers")?;
        } else {
            require(frame.width == width && frame.height == height && frame.source == Some(NativeWord(address)) && frame.stride == Some(stride), Refusal::RenderCompletion, "native direct main geometry differs from registers")?;
        }
    } else {
        require(frame.main.is_none() && frame.source.is_none(), Refusal::RenderCompletion, "native frame invents an inactive main plane")?;
    }
    let mut rectangles = Vec::with_capacity(8);
    let mut channels = 0usize;
    for layer in [Layer::Lower, Layer::Upper] {
        let bank = u32::from(u8::from(layer)) * 0x50;
        let mode = reg(0x34 + bank);
        let order = if layer == Layer::Lower { [Channel::Zero, Channel::One, Channel::Two, Channel::Three] } else { [Channel::Three, Channel::Two, Channel::One, Channel::Zero] };
        for channel in order {
            let index = u32::from(u8::from(channel));
            if reg(0x40 + bank) & (1 << (index * 4)) == 0 { continue; }
            channels += 1;
            let mut matches = frame.graphics.iter().filter(|plane| plane.layer == layer && plane.channel == channel);
            let plane = matches.next().ok_or_else(|| refusal(Refusal::RenderCompletion, "enabled native plane identity is missing"))?;
            require(matches.next().is_none(), Refusal::RenderCompletion, "enabled native plane identity is duplicated")?;
            let dimensions = reg(0x54 + bank + index * 4);
            let width = ((dimensions >> 16) & 0x1fff) + 1;
            let height = (dimensions & 0x1fff) + 1;
            let address = reg(0x64 + bank + index * 4);
            let padding = reg(0x74 + bank + index * 4);
            let origin = reg(0x44 + bank + index * 4);
            let orientation = (reg(8) >> if layer == Layer::Upper { 8 } else { 4 }) & 0xf;
            require(mode <= 1 && dimensions & 0xe000e000 == 0 && address & 0xc0000000 == 0 && padding & !0xfff == 0 && origin & 0xc000c000 == 0 && matches!(orientation, 0 | 5), Refusal::RenderCompletion, "native graphics geometry has unmodeled controls")?;
            let scale = if orientation == 5 { 2 } else { 1 };
            let x = ((origin >> 16) & 0x3fff) * scale;
            let y = (origin & 0x3fff) * scale;
            require(plane.width == width && plane.height == height && plane.source.0 == address && plane.origin == [x, y], Refusal::RenderCompletion, "native graphics frame geometry differs from registers")?;
            if x >= canvas[0] || y >= canvas[1] {
                require(plane.visible == Some(false), Refusal::RenderCompletion, "off-canvas native plane claims source bytes")?;
                continue;
            }
            require(plane.visible != Some(false), Refusal::RenderCompletion, "visible native plane was omitted")?;
            let stride = add(PlaneKind::Graphics { layer, channel }, address, width, height, padding, if mode == 0 { 2 } else { 4 })?;
            require(plane.stride_bytes == Some(stride), Refusal::RenderCompletion, "native graphics stride differs from geometry")?;
            rectangles.push(Rectangle { x, y, width: (width * scale).min(canvas[0] - x), height: (height * scale).min(canvas[1] - y) });
        }
    }
    require(frame.graphics.len() == channels, Refusal::RenderCompletion, "native frame includes an inactive graphics plane")?;
    let dimensions = if channels == 0 { frame.canvas.unwrap_or([frame.width, frame.height]) } else { canvas };
    require([scanout.pixels.width, scanout.pixels.height] == dimensions && (channels == 0 || [frame.width, frame.height] == canvas) && frame.declared_backdrop_used == (channels != 0 && !main_enabled && controller.backdrop_profile.is_some()), Refusal::RenderCompletion, "native composition canvas/backdrop differs")?;
    if !main_enabled && !frame.declared_backdrop_used {
        let mut row = Vec::with_capacity(8);
        for y in 0..canvas[1] {
            row.clear();
            row.extend(rectangles.iter().filter(|rectangle| rectangle.y <= y && y < rectangle.y + rectangle.height).map(|rectangle| (rectangle.x, rectangle.x + rectangle.width)));
            row.sort_unstable();
            let mut covered = 0;
            for &(begin, end) in &row { require(begin <= covered, Refusal::RenderCompletion, "native planes leave a canvas coverage gap")?; covered = covered.max(end); }
            require(covered == canvas[0], Refusal::RenderCompletion, "native planes do not cover the full canvas")?;
        }
    }
    require(!expected.is_empty() && expected.len() == scanout.source_planes.len(), Refusal::RenderCompletion, "native selected plane set differs")?;
    for (expected, actual) in expected.iter().zip(&scanout.source_planes) {
        require(expected.kind == actual.kind && expected.native_source_address == actual.native_source_address && expected.offset == actual.offset && expected.bytes == actual.bytes, Refusal::RenderCompletion, "native full-plane identity/extent is truncated or differs")?;
    }
    require(endpoint == scanout.source_bytes.bytes, Refusal::RenderCompletion, "native full-plane source bytes are incomplete")
}
pub(super) struct TruncatedPlane { pub(super) scanout: Vec<u8>, pub(super) planes: Vec<u8> }
pub(super) fn truncated_plane(root: &Path, visible: &Visible) -> Result<TruncatedPlane, Failure> {
    let mut scanout: Scanout = parse(&artifact(root, &visible.scanout)?, Refusal::RenderCompletion)?;
    let mut planes = artifact(root, &visible.planes)?;
    let plane = scanout.source_planes.last_mut().ok_or_else(|| refusal(Refusal::RenderCompletion, "no selected plane to truncate"))?;
    require(plane.bytes > 1 && plane.offset.checked_add(plane.bytes) == Some(planes.len() as u64), Refusal::RenderCompletion, "truncation control lacks a complete final plane")?;
    planes.pop();
    plane.bytes -= 1;
    scanout.source_bytes.bytes -= 1;
    scanout.source_bytes.sha256 = Digest::of(&planes);
    let scanout = serde_json::to_vec(&scanout).map_err(|error| refusal(Refusal::RenderCompletion, error.to_string()))?;
    Ok(TruncatedPlane { scanout, planes })
}
fn word(views: &[Words], address: u64) -> Result<u32, Failure> {
    let address = u32::try_from(address).map_err(|_| refusal(Refusal::Mapping, "render address exceeds32bits"))?;
    projected(views, address)
}
fn byte(views: &[Words], address: u64) -> Result<u8, Failure> {
    Ok((word(views, address & !3)? >> (8 * (address & 3))) as u8)
}
fn half(views: &[Words], address: u64) -> Result<u16, Failure> {
    require(address & 3 <= 2, Refusal::Mapping, "render halfword crosses projection")?;
    Ok((word(views, address & !3)? >> (8 * (address & 3))) as u16)
}
fn ready(report: &Visible, document: &RunDocument, graphics: &GraphicsEngine) -> Result<(), Failure> {
    let firmware = &document.firmware;
    let facts = &firmware.render;
    require(word(&report.state, firmware.scanner_registered.0)? == 1 && word(&report.state, firmware.calibration_pointer.0)? != 0, Refusal::RenderCompletion, "input scanner/calibration pointer not initialized")?;
    for binding in &firmware.keys {
        let address = firmware.key_table.base.0.checked_add(binding.row * firmware.key_table.row_bytes)
            .and_then(|address| address.checked_add(firmware.key_table.released_offset))
            .ok_or_else(|| refusal(Refusal::Mapping, "key release address overflows"))?;
        require(byte(&report.state, address)? == 0, Refusal::RenderCompletion, "a device input remains pressed")?;
    }
    for address in [facts.ui_pending, facts.ui_busy, facts.display_suppression, facts.batch_busy, facts.batch_callback, facts.commands_dirty, facts.display_pending] {
        let value = if address.0 == facts.ui_busy.0 { u32::from(byte(&report.state, address.0)?) } else { word(&report.state, address.0)? };
        require(value == 0, Refusal::RenderCompletion, "application/render work remains pending")?;
    }
    require(byte(&report.state, facts.initialized.0)? == 1 && !graphics.busy && graphics.command_pending.is_none() && graphics.regs.iter().find(|(offset, _)| *offset == 0x34).map_or(0, |(_, value)| *value) == 0 && graphics.jobs_started == graphics.jobs_completed && graphics.acknowledgments >= graphics.jobs_completed, Refusal::RenderCompletion, "graphics/UILIB completion is unacknowledged")?;
    require(facts.task_entries.len() == 4, Refusal::RenderCompletion, "selected render-actor set differs")?;
    let mut statuses = [None; 4];
    for layout in capture::TASK_LAYOUTS {
        let creation = layout.creation.0;
        let tasks = layout.task.0;
        for index in 0..120 {
            let c = creation + 40 * index;
            if projected(&report.task_tables, c + 36)? == 0 { continue; }
            let entry = projected(&report.task_tables, c + 8)?;
            if let Some(slot) = facts.task_entries.iter().position(|candidate| candidate.0 == u64::from(entry)) {
                require(statuses[slot].is_none(), Refusal::RenderCompletion, "render actor has duplicate committed records")?;
                require(physical(Guest(projected(&report.task_tables, tasks + 56 * index + 8)?)) == physical(Guest(c)), Refusal::TaskContext, "render task/creation linkage differs")?;
                statuses[slot] = Some((projected(&report.task_tables, tasks + 56 * index + 12)? & 0xff) as u8);
            }
        }
    }
    require(statuses.iter().all(|status| *status == Some(0x12)), Refusal::RenderCompletion, "selected application/render actor is not waiting")?;
    let probe = Probe {
        copied_command: half(&report.state, facts.copied_command.0)?,
        packet_token: byte(&report.state, facts.packet_token.0)?,
        sender_token: byte(&report.state, facts.sender_token.0)?,
        consumer_token: byte(&report.state, facts.consumer_token.0)?,
        sender_command: half(&report.state, facts.sender_command.0)?,
        staged_render_command: half(&report.state, facts.staged_render_command.0)?,
        staged_render_token: byte(&report.state, facts.staged_render_token.0)?,
        drained: true,
        uilib_waiting: true,
        graphics: fujimod_xm1::render::Graphics {
            idle: true,
            started: graphics.jobs_started,
            completed: graphics.jobs_completed,
            bytes_completed: graphics.bytes_completed,
        },
    };
    require(probe == report.probe && probe.consumer_finished(), Refusal::RenderCompletion, "reported render probe differs from raw state or token consumption")?;
    let node = word(&report.state, firmware.current_menu.0)?;
    let menu = if node == 0 { None } else { Some([word(&report.state, u64::from(node))?, word(&report.state, u64::from(node) + 8)?, word(&report.state, firmware.current_menu.0 + 8)?]) };
    require(menu == report.menu, Refusal::RenderCompletion, "menu result differs from committed descriptor/cursor")
}
fn capture_file(root: &Path, binding: &FileBinding, captured: &CapturedArtifact) -> Result<(), Failure> {
    let expected_name = binding.path.file_name().ok_or_else(|| refusal(Refusal::ArtifactIdentity, "capture filename absent"))?;
    require(captured.path.file_name() == Some(expected_name) && captured.sha256.as_str() == binding.sha256.as_str(), Refusal::RenderCompletion, "native capture artifact binding differs")?;
    require(artifact(root, binding)?.len() as u64 == captured.bytes, Refusal::RenderCompletion, "native capture artifact extent differs")
}
pub(super) fn visible(root: &Path, report: &PumpReport, document: &RunDocument, sequence: &Sequence) -> Result<(), Failure> {
    require(report.inputs.len() == 20 && report.visible.len() == 6, Refusal::RouteUnavailable, "natural first-start and interaction program is incomplete")?;
    let mut previous = Count(80_000_000);
    for (index, input) in report.inputs.iter().enumerate() {
        let expected = match index { 0..=11 => (RouteKey::Up, InputPurpose::FirstStart), 12..=14 => (RouteKey::Ok, InputPurpose::FirstStart), 15 => (RouteKey::Down, InputPurpose::Ordinary), 16 => (RouteKey::Up, InputPurpose::Ordinary), 17 => (RouteKey::Back, InputPurpose::BackHandback), 18 => (RouteKey::Ok, InputPurpose::AfterBack), _ => (RouteKey::Left, InputPurpose::UnhandledProbe) };
        require(input.id.0 == index && input.key == expected.0 && input.purpose == expected.1 && input.pressed.0 >= previous.0 && input.released == input.pressed.0.checked_add(8_000_000).map(Count), Refusal::RouteUnavailable, "device input order/pulse differs")?;
        let released = input.released.ok_or_else(|| refusal(Refusal::RouteUnavailable, "input release absent"))?;
        let debounced = input.debounced.ok_or_else(|| refusal(Refusal::RouteUnavailable, "input debounce absent"))?;
        require(released.0 <= debounced.0 && debounced.0 - released.0 <= 8_000_000, Refusal::RouteUnavailable, "input release deadline differs")?;
        previous = input.settled.unwrap_or(debounced);
        if index >= 15 {
            require(input.before.is_some_and(|probe| probe.drained && probe.consumer_finished()) && input.after.is_some_and(|probe| probe.drained && probe.consumer_finished()) && input.settled.is_some_and(|settled| settled.0 >= debounced.0 && settled.0 <= report.instructions.0), Refusal::RenderCompletion, "ordinary input lacks drained handback")?;
            require(sequence.invocations.iter().any(|pump| {
                let entry = &report.samples[pump.entry.0];
                let handback = &report.samples[pump.handback.0];
                entry.input == Some(input.id) && handback.input == Some(input.id)
                    && input.pressed.0 <= entry.stop.0 && entry.stop.0 < handback.stop.0
                    && input.settled.is_some_and(|settled| handback.stop.0 <= settled.0)
            }), Refusal::Handback, "device input lacks a matching complete pump within its pressed-to-settled interval")?;
        }
    }
    for (index, visible) in report.visible.iter().enumerate() {
        require(visible.input == if index == 0 { None } else { Some(InputId(index + 14)) } && visible.instructions.0 <= report.instructions.0 && visible.host_reads.stopped == visible.instructions, Refusal::RenderCompletion, "visible boundary attribution differs")?;
        raw_views(root, &visible.raw_views, visible.state.iter().chain(&visible.task_tables))?;
        let graphics: GraphicsSave = parse(&artifact(root, &visible.graphics)?, Refusal::RenderCompletion)?;
        ready(visible, document, &graphics.engine)?;
        let scanout: Scanout = parse(&artifact(root, &visible.scanout)?, Refusal::RenderCompletion)?;
        require(scanout.schema == 1 && scanout.node.0 == document.scanout.0 && scanout.instructions == visible.instructions.0 && scanout.pixels.width > 0 && scanout.pixels.height > 0 && u64::from(scanout.pixels.width).checked_mul(u64::from(scanout.pixels.height)).and_then(|pixels| pixels.checked_mul(3)) == Some(scanout.pixels.rgb_bytes) && !scanout.pixels.rgb_sha256.as_str().is_empty(), Refusal::RenderCompletion, "native pixel capture identity/dimensions differ")?;
        capture_file(root, &visible.png, &scanout.png)?;
        capture_file(root, &visible.planes, &scanout.source_bytes)?;
        full_planes(&scanout)?;
        if let Some(id) = visible.input {
            let input = &report.inputs[id.0];
            require(input.settled == Some(visible.instructions) && input.after == Some(visible.probe), Refusal::RenderCompletion, "settled input/visible state differs")?;
            if input.purpose != InputPurpose::UnhandledProbe {
                let before = &report.visible[index - 1];
                require(before.png.sha256.as_str() != visible.png.sha256.as_str() && visible.probe.graphics.started > before.probe.graphics.started && visible.probe.graphics.completed > before.probe.graphics.completed && visible.probe.graphics.bytes_completed > before.probe.graphics.bytes_completed, Refusal::RenderCompletion, "ordinary interaction has no newly acknowledged visible result")?;
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
pub(super) struct Comparison { schema: u32, retained: Digest, owned: Digest, frames: usize, complete_pumps: [usize; 2], pending_work_pumps: [usize; 2], limitations: Vec<&'static str> }
pub(super) fn paired(retained: &Path, owned: &Path, canonical: &Catalog) -> Result<Comparison, Failure> {
    let left_bytes = bounded(&retained.join("application-pump.json"), METADATA_BYTES)?;
    let right_bytes = bounded(&owned.join("application-pump.json"), METADATA_BYTES)?;
    let left: PumpReport = parse(&left_bytes, Refusal::Guest)?;
    let right: PumpReport = parse(&right_bytes, Refusal::Guest)?;
    require(left.mode == Mode::Retained && right.mode == Mode::Owned, Refusal::ArtifactIdentity, "paired roles differ")?;
    let admitted_left = inspect(retained, &left, canonical)?;
    let admitted_right = inspect(owned, &right, canonical)?;
    require(left.identity.observer.as_str() == right.identity.observer.as_str() && left.identity.code.sha256.as_str() == right.identity.code.sha256.as_str() && left.identity.symbols.sha256.as_str() == right.identity.symbols.sha256.as_str() && left.identity.admission.sha256.as_str() == right.identity.admission.sha256.as_str(), Refusal::ArtifactIdentity, "cohort producer/candidate identities differ")?;
    for (before, after) in left.visible.iter().zip(&right.visible) {
        require(before.input == after.input && before.menu == after.menu && before.png.sha256.as_str() == after.png.sha256.as_str() && before.planes.sha256.as_str() == after.planes.sha256.as_str(), Refusal::RenderCompletion, "paired visible menu/full-plane result differs")?;
        for address in (0x1467588..0x1467840).step_by(4).chain((0x944580..0x9445d0).step_by(4)) {
            require(projected(&before.state, address)? == projected(&after.state, address)?, Refusal::HookSequence, "paired caller-visible controller/mailbox state differs")?;
        }
    }
    Ok(Comparison { schema: 1, retained: Digest::of(&left_bytes), owned: Digest::of(&right_bytes), frames: left.visible.len(), complete_pumps: [admitted_left.complete_pumps, admitted_right.complete_pumps], pending_work_pumps: [admitted_left.pending_work_pumps, admitted_right.pending_work_pumps], limitations: vec!["Entered-instruction counts are not silicon cycles and need not match.", "Initialization,outer actor loop,normalizer/actions,kernel/input/settings/calibration/render services remain retained.", "Unhandled/no-op and receive/service failures are not qualified merely by naming an input probe;editor-state cancellation is separate."] })
}
