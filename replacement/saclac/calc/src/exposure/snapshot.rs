//! The record snapshot an exposure takes at preparation (0xf69a4c, 0xf69b08).

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::shared::{group_14, group_27};

use crate::exposure::context::Snapshot;
use crate::exposure::ports::Ports;
use crate::stock::{record, shared};

/// Takes the record's focus result and adjustment tables. A focus result taken at another lens
/// reference leaves the snapshot invalid; a valid one lowers the shared focus curve by its terms.
pub fn take(ports: &mut impl Ports, record_handle: RecordHandle, lens_reference: u32) -> Snapshot {
    let profile = ports.profile();
    let focus = record::focus_result(&profile.read_shared(record_handle, &group_14::FOCUS_RESULT));
    let mut snapshot = Snapshot { focus, ..Snapshot::default() };
    if focus.reference == lens_reference {
        snapshot.valid = focus.valid;
        snapshot.extent = focus.extent;
    }
    snapshot.bounds = [
        profile.read_shared(record_handle, &group_27::AT_0262),
        profile.read_shared(record_handle, &group_27::AT_0264),
        profile.read_shared(record_handle, &group_27::AT_0268),
        profile.read_shared(record_handle, &group_27::AT_026A),
    ];
    snapshot.subjects = record::subject_list(&profile.read_shared(record_handle, &group_27::AT_026C));
    snapshot.table_words = record::words(&profile.read_shared(record_handle, &group_27::AT_0680));
    snapshot.limits = [
        profile.read_shared(record_handle, &group_27::AT_06F8),
        profile.read_shared(record_handle, &group_27::AT_06FA),
        profile.read_shared(record_handle, &group_27::AT_06FC),
        profile.read_shared(record_handle, &group_27::AT_06FE),
        profile.read_shared(record_handle, &group_27::AT_0700),
        profile.read_shared(record_handle, &group_27::AT_0702) as i16,
    ];
    snapshot.limit_word = profile.read_shared(record_handle, &group_27::AT_0704);
    snapshot.table_block = record::words(&profile.read_shared(record_handle, &group_27::AT_06A8));
    if snapshot.valid != 0 {
        let [first, second, third, fourth, fifth, sixth] = focus.terms;
        let lens = ports.level(focus.lens, focus.reference) as u16;
        snapshot.offset = fifth.wrapping_add(first).wrapping_sub(third).wrapping_sub(second).wrapping_sub(sixth).wrapping_sub(fourth).wrapping_add(lens) as i16;
        let lowered = second.wrapping_add(third).wrapping_add(fourth);
        for point in 0..shared::CURVE_POINTS {
            let at = shared::focus_curve(point);
            let value = ports.read_u16(at);
            ports.write_u16(at, value.wrapping_sub(lowered));
        }
    }
    snapshot
}
