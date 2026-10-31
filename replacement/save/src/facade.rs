//! The save request facade: image-5 `FUN_00e518c0`.

use super::contract::*;
use super::ports::*;

/// Admission modes `FUN_00e51858` can answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Override {
    None,
    ProfileA5,
    ProfileA7,
    ProfileA9,
}

fn override_mode<S: SelectorService>(services: &mut S) -> Override {
    if services.present() && services.profile().is_a5() {
        return Override::ProfileA5;
    }
    if services.present() && services.profile().is_a7() {
        return Override::ProfileA7;
    }
    if services.present() && services.profile().is_a9() {
        return Override::ProfileA9;
    }
    Override::None
}

fn dispatch_profile_shortcut<S: SelectorService>(services: &mut S) -> bool {
    if services.save_dispatch() != SaveDispatch::ENABLED {
        return false;
    }
    if services.profile().is_a2() {
        return true;
    }
    if services.profile().above_0x82() {
        if services.profile().below_0x88() {
            return true;
        }
    }
    services.profile().is_a4()
}

/// The admission shortcut returns before metadata and storage.
pub fn admission_shortcut<S: SaveServices>(services: &mut S) -> bool {
    if !services.present() {
        return false;
    }
    if dispatch_profile_shortcut(services) {
        return true;
    }
    if override_mode(services) != Override::None {
        return true;
    }
    services.profile().is_a8()
}

fn phase_two<S: SaveServices>(services: &mut S, status_out: Option<StatusOut>) {
    let task = services.current_task();
    services.set_save_task(SaveTaskSlot::PhaseTwo, task);
    let completion = services.release_completion();
    services.register_release(completion);
    let _flags = services.wait(WaitMask::PHASE_TWO, WaitTimeout::Infinite);
    services.set_save_task(SaveTaskSlot::PhaseTwo, None);
    if let Some(out) = status_out {
        out.write(RawStatus::SAVED);
    }
}

/// Runs `FUN_00e518c0`.
pub fn run<S: SaveServices>(
    services: &mut S,
    kind: SaveKind,
    status_out: Option<StatusOut>,
) -> SaveOutcome {
    if admission_shortcut(services) {
        let status = services.save_status();
        if let Some(out) = status_out {
            out.write(status);
        }
        return SaveOutcome {
            returned: SaveReturn::Success,
            status,
            shortcut: true,
            phase_one: PhaseOne::NotWaited,
            phase_two: PhaseTwo::NotRun,
        };
    }

    let bank = kind.bank();
    let metadata = if kind == SaveKind::Kind0 && services.frame_state() == FrameState::ONE {
        MetadataPass::Selected
    } else {
        MetadataPass::Alternate
    };
    services.prepare_metadata(metadata);
    services.write_save_mode(bank, kind.save_mode());
    services.clear_descriptor_word(bank);

    if services.subframe_command() == SubframeCommand::ZERO {
        let preset = services.extent_preset();
        services.set_extent_preset(preset);
    }

    let names_extent = match kind {
        SaveKind::Kind0 => services.extent_layout() == ExtentLayout::TWO,
        SaveKind::Kind1 => {
            services.frame_state() == FrameState::ZERO
                && services.extent_layout() == ExtentLayout::TWO
        }
        _ => false,
    };
    if names_extent {
        services.name_extent(bank);
    }

    services.sample_cached_state();
    services.stage(StagePoint::Start);
    let task = services.current_task();
    services.set_save_task(SaveTaskSlot::PhaseOne, task);
    let completion = services.save_completion();
    services.start(kind.storage_variant(), bank, completion);
    let flags = services.wait(WaitMask::PHASE_ONE, WaitTimeout::Infinite);
    services.set_save_task(SaveTaskSlot::PhaseOne, None);
    services.stage(StagePoint::End);

    if kind != SaveKind::Kind3
        && bank == DescriptorBank::BANK2
        && services.extent_layout() == ExtentLayout::TWO
    {
        services.copy_extent_name(bank);
    }

    let interrupted = flags.interrupted();
    let returned = if interrupted {
        SaveReturn::Interrupted
    } else {
        SaveReturn::Success
    };
    let phase_one = PhaseOne::Observed(flags);

    let mut status = services.save_status();
    if let Some(out) = status_out {
        out.write(status);
    }

    let phase_two = if interrupted && status.is_saved() {
        phase_two(services, status_out);
        status = RawStatus::SAVED;
        PhaseTwo::Completed
    } else {
        PhaseTwo::NotRun
    };

    SaveOutcome {
        returned,
        status,
        shortcut: false,
        phase_one,
        phase_two,
    }
}
