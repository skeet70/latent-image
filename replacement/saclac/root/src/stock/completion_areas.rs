use core::cell::RefCell;
use xm1_saclac_imaging::capture::{detection::{self, Crop, CropRequest, Detection, DetectionPorts, Extent, Point, Rect}, ports::Adjustments, AdjustmentKey};
use xm1_saclac_interface::{calc::SharedAddress, stock::record_handle_word, RecordHandle};
use xm1_saclac_profile_geometry::{controller::{queries::centred_rectangle, windows::CropInput, ProfileGeometry, Tables}, geometry::AreaSource, memory::Memory, ports::Ports, profile::{ProfileHandle, Rectangle}};
use xm1_saclac_shared::{settings::{self, AdjustmentKey as SharedKey}, stock::{Machine, Stock}};
use xm1_shot_sequence::boundary::ClientRef;

const LISTS: u32 = 0x0169_e580;
const PRIMARY_LIST: u32 = LISTS + 0xc3c;
const REPORT: u32 = LISTS + 0x1f04;
const PRIMARY_DETECTION: u32 = 0x0169_dd40;
const SECONDARY_DETECTION: u32 = 0x0169_d920;
const LIST_WORDS: u32 = 0x414 / 4;

/// f51b50's exact query/refresh order at x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
pub fn source_offset(machine: &impl Machine, record: RecordHandle) -> u32 {
    let value = machine.enter(0x128_7354, &[]);
    let record = record_handle_word(record) as usize;
    if machine.enter(0x9e_b104, &[record]) == 1 && machine.enter(0x9e_b6ec, &[record]) == 6 {
        machine.enter(0x9e_d8e4, &[record]);
    }
    value
}

struct DetectionView<'a, M, G, P, T> {
    stock: &'a Stock<M>,
    geometry: &'a RefCell<ProfileGeometry<G, P, T>>,
    detection: Detection,
}

impl<M: Machine, G, P, T> Adjustments for DetectionView<'_, M, G, P, T> {
    fn byte(&self, key: AdjustmentKey) -> u8 { settings::Adjustments::byte(self.stock, SharedKey(key.0)) }
}

impl<M: Machine, G: Memory, P: Ports, T: Tables> DetectionPorts for DetectionView<'_, M, G, P, T> {
    fn detection(&mut self) -> Detection { self.detection }
    fn crop(&mut self, request: &CropRequest) -> Crop {
        let crop = self.geometry.borrow_mut().crop_geometry(&CropInput {
            profile: ProfileHandle(request.profile), area: request.area, divisors: request.ratio, relative: request.inset,
        });
        // Decode the same eight ABI words as Imaging's stock detection adapter.
        Crop {
            origin: Point { x: crop.window.top, y: crop.window.left },
            frame: [crop.window.width, crop.window.height],
            window: Rect { origin: Point { x: crop.crop.top, y: crop.crop.left }, extent: Extent { height: crop.crop.width, width: crop.crop.height } },
        }
    }
    fn centered(&mut self, crop: &Crop, detection: &Detection) -> Rect {
        let rectangle = centred_rectangle(Rectangle {
            top: crop.origin.x, left: crop.origin.y, width: crop.frame[0], height: crop.frame[1],
        }, detection.frame);
        Rect { origin: Point { x: rectangle.top, y: rectangle.left }, extent: Extent { height: rectangle.width, width: rectangle.height } }
    }
}

/// The full update publishes its report before querying/copying the list. Detection only writes
/// its sparse forty-byte result; the copied list's remaining bytes retain their shared-buffer values.
pub fn render_areas<M: Machine, G: Memory, P: Ports, T: Tables>(
    machine: &mut M, stock: &Stock<M>, geometry: &RefCell<ProfileGeometry<G, P, T>>,
    source: &AreaSource, scale: [u32; 2], paired: bool, destination: Option<SharedAddress>,
    detector: impl FnOnce() -> Detection,
) {
    let full = destination.is_none();
    let destination = destination.map_or(PRIMARY_LIST, |address| address.0);
    if !paired {
        if full { machine.write_u32(REPORT, 2); }
        if let Ok(list) = geometry.borrow_mut().paired_areas(source, scale) {
            crate::shot_capture_geometry::publish_areas(machine, ClientRef(destination), &list);
        }
        return;
    }
    let mut view = DetectionView { stock, geometry, detection: detector() };
    let placement = detection::rescale_detection(&mut view, source.area, source.ratio, Extent { height: scale[0], width: scale[1] });
    let output = if full { PRIMARY_DETECTION } else { SECONDARY_DETECTION };
    xm1_saclac_imaging::stock::detection::write_placement(&placement, |index, value| machine.write_u32(output + index as u32 * 4, value));
    if full {
        let report = machine.read_u32(output + 4);
        machine.write_u32(REPORT, report);
    }
    for index in 0..LIST_WORDS {
        let value = machine.read_u32(output + 12 + index * 4);
        machine.write_u32(destination + index * 4, value);
    }
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use core::arch::naked_asm;
    use xm1_power_lifecycle::{self as core_record, record_address_space::{GuestAddress, MemoryError, NativeAddressSpace, RecordContext}};
    use xm1_saclac_controllers::layers::MirrorSources;
    use xm1_saclac_shared::stock::Guest;

    unsafe fn access<R>(body: impl FnOnce(&mut RecordContext<NativeAddressSpace<'_>, ()>) -> Result<R, MemoryError>) -> R {
        unsafe { core_record::record_native_boundary::with_native_context::<(), _>(|context| {
            body(context).unwrap_or_else(|_| core_record::record_native_boundary::fault())
        }) }
    }

    unsafe extern "C" fn backed(profile: u32, mode: u32, destination: u32, scale_frame: u32) {
        let (services, geometry) = super::super::composition::area_services();
        let mut machine = Guest;
        let record = xm1_saclac_interface::stock::record_handle_from_a2(profile);
        let offset = source_offset(&machine, record);
        let (second, third, ratio) = unsafe { access(|context| Ok((
            core_record::record_profile_slotfields::helper_7527f8(context, profile)?,
            core_record::record_profile_slotfields::helper_752890(context, profile)?,
            core_record::record_profile_accessors::helper_74ec30(context, profile)?,
        ))) };
        let source = AreaSource { area: [offset, second, third], ratio: [ratio as u16, (ratio >> 16) as u16] };
        let scaled = MirrorSources::capture_window(&mut *services.borrow_mut(), mode, ratio);
        let scale = if let Some(scale) = scaled {
            machine.write_u32(scale_frame, scale[0]);
            machine.write_u32(scale_frame + 4, scale[1]);
            scale
        } else {
            // The disabled conversion leaves these two native frame words unchanged.
            [machine.read_u32(scale_frame), machine.read_u32(scale_frame + 4)]
        };
        let paired = unsafe { access(|context| core_record::record_profile_accessors::helper_74fc64(context, profile)) } != 0;
        let full = destination == 0;
        {
            let services = services.borrow();
            render_areas(&mut machine, &services.stock, geometry, &source, scale, paired,
                if full { None } else { Some(SharedAddress(destination)) }, || {
                    let mut detector = xm1_saclac_imaging::stock::detection::StockDetection;
                    detector.detection()
                });
        }
        if full {
            unsafe { access(|context| {
                core_record::record_profile_shared_cpu::helper_75638c(context, profile, 0)?;
                core_record::record_profile_shared_cpu::helper_756478(context, profile, GuestAddress::new(PRIMARY_LIST + 4))?;
                let report = machine.read_u32(REPORT);
                core_record::record_profile_shared_cpu::helper_756508(context, profile, u32::from(report == 0))
            }) };
        }
    }

    #[unsafe(naked)]
    unsafe extern "C" fn primary(_profile: u32, _mode: u32, _destination: u32) {
        naked_asm!(
            "push {{r4,r5,r11,lr}}", "add r11,sp,#12", "sub sp,sp,#24",
            "sub r3,r11,#20", "bl {body}", "sub sp,r11,#12", "pop {{r4,r5,r11,pc}}",
            body = sym backed,
        )
    }

    #[unsafe(naked)]
    unsafe extern "C" fn secondary(_profile: u32, _mode: u32, _destination: u32) {
        naked_asm!(
            "push {{r4,r5,r6,r11,lr}}", "add r11,sp,#16", "sub sp,sp,#28",
            "sub r3,r11,#28", "bl {body}", "sub sp,r11,#16", "pop {{r4,r5,r6,r11,pc}}",
            body = sym backed,
        )
    }

    pub(in crate::stock) fn prepare(record: RecordHandle, mode: u32, destination: Option<SharedAddress>) {
        let profile = record_handle_word(record);
        unsafe { match destination {
            None => primary(profile, mode, 0),
            Some(destination) => secondary(profile, mode, destination.0),
        } }
    }
}

#[cfg(target_arch = "arm")]
pub(super) use native::prepare;
