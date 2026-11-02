use super::*;

#[derive(Deserialize)]
struct GraphicsSave { engine: GraphicsEngine }
#[derive(Deserialize)]
struct GraphicsEngine {
    regs: Vec<(u32, u32)>, busy: bool, command_pending: Option<serde::de::IgnoredAny>,
    jobs_started: u64, jobs_completed: u64, bytes_completed: u64, acknowledgments: u64,
}
#[derive(Deserialize)]
struct Pixels { width: u32, height: u32, rgb_bytes: u64, rgb_sha256: Digest }
#[derive(Deserialize)]
struct CapturedArtifact { path: PathBuf, sha256: Digest, bytes: u64 }
#[derive(Deserialize)]
struct Plane { native_source_address: u64, offset: u64, bytes: u64 }
#[derive(Deserialize)]
struct Scanout {
    schema: u32, instructions: u64, pixels: Pixels, png: CapturedArtifact,
    source_bytes: CapturedArtifact, source_planes: Vec<Plane>,
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
pub(super) fn visible(root: &Path, report: &PumpReport, document: &RunDocument) -> Result<(), Failure> {
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
            require(report.samples.iter().any(|sample| sample.input == Some(input.id) && physical(sample.pc) == CARRIER.0) && report.samples.iter().any(|sample| sample.input == Some(input.id) && physical(sample.pc) == 0xc23278), Refusal::Handback, "device input did not traverse an owned/retained pump and handback")?;
        }
    }
    for (index, visible) in report.visible.iter().enumerate() {
        require(visible.input == if index == 0 { None } else { Some(InputId(index + 14)) } && visible.instructions.0 <= report.instructions.0 && visible.host_reads.stopped == visible.instructions, Refusal::RenderCompletion, "visible boundary attribution differs")?;
        raw_views(root, &visible.raw_views, visible.state.iter().chain(&visible.task_tables))?;
        let graphics: GraphicsSave = parse(&artifact(root, &visible.graphics)?, Refusal::RenderCompletion)?;
        ready(visible, document, &graphics.engine)?;
        let scanout: Scanout = parse(&artifact(root, &visible.scanout)?, Refusal::RenderCompletion)?;
        require(scanout.schema == 1 && scanout.instructions == visible.instructions.0 && scanout.pixels.width > 0 && scanout.pixels.height > 0 && u64::from(scanout.pixels.width).checked_mul(u64::from(scanout.pixels.height)).and_then(|pixels| pixels.checked_mul(3)) == Some(scanout.pixels.rgb_bytes) && !scanout.pixels.rgb_sha256.as_str().is_empty(), Refusal::RenderCompletion, "native pixel capture dimensions differ")?;
        capture_file(root, &visible.png, &scanout.png)?;
        capture_file(root, &visible.planes, &scanout.source_bytes)?;
        let mut endpoint = 0u64;
        for plane in &scanout.source_planes {
            require(plane.offset == endpoint && plane.bytes > 0 && plane.native_source_address < 1u64 << 32, Refusal::RenderCompletion, "selected plane coverage is incomplete")?;
            endpoint = endpoint.checked_add(plane.bytes).ok_or_else(|| refusal(Refusal::RenderCompletion, "selected plane extent overflows"))?;
        }
        require(endpoint != 0 && endpoint == scanout.source_bytes.bytes, Refusal::RenderCompletion, "selected full-plane bytes are missing")?;
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
