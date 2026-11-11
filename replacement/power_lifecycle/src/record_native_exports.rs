//! C-ABI exports over the context-first record bodies.
//!
//! Each entry forwards the stock argument words to one body and returns the
//! stock result. The stock body has no failing case, so an adapter error halts.

macro_rules! stock_export {
    (
        $symbol:ident,
        $abi:ident,
        $effects:ty,
        ($($cp:ident: $cty:ty),*) -> $cret:ty,
        ($($ap:ident: $aty:ty),*),
        ($($conv:expr),*),
        $body:path,
        $ret:expr
        $(, bounds $($bounds:tt)+)?
    ) => {
        #[cfg(target_arch = "arm")]
        #[no_mangle]
        pub unsafe extern "C" fn $symbol($($cp: $cty),*) -> $cret {
            crate::record_native_boundary::with_native_context::<$effects, _>(|context| {
                match $abi(context $(, $conv)*) {
                    Ok(value) => value,
                    Err(_) => crate::record_native_boundary::fault(),
                }
            })
        }

        #[allow(dead_code)]
        pub fn $abi<M: crate::record_address_space::GuestAddressSpace, E>(
            context: &mut crate::record_address_space::RecordContext<M, E>,
            $($ap: $aty),*
        ) -> Result<$cret, crate::record_native_boundary::AbiFault>
        $(where E: $($bounds)+)?
        {
            $body(context $(, $ap)*)
                .map_err(crate::record_native_boundary::AbiFault::from)
                .map($ret)
        }
    };
}

pub(crate) use stock_export;


pub mod record_configuration {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_69f8fc,
        helper_69f8fc_abi,
        (),
        (key: u32) -> u32,
        (key: u32),
        (key),
        crate::record_configuration::helper_69f8fc,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_69fec8,
        helper_69fec8_abi,
        (),
        (key: u32) -> u32,
        (key: u32),
        (key),
        crate::record_configuration::helper_69fec8,
        |value| value
    );
}

pub mod record_display_selection_buffers {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_105386c,
        helper_105386c_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_display_selection_buffers::helper_105386c,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f47468,
        helper_f47468_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_display_selection_buffers::helper_f47468,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f48e74,
        helper_f48e74_abi,
        (),
        (destination: u32) -> (),
        (destination: GuestAddress),
        (GuestAddress::new(destination as u32)),
        crate::record_display_selection_buffers::helper_f48e74,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f48f1c,
        helper_f48f1c_abi,
        (),
        (destination: u32) -> (),
        (destination: GuestAddress),
        (GuestAddress::new(destination as u32)),
        crate::record_display_selection_buffers::helper_f48f1c,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f48fd8,
        helper_f48fd8_abi,
        (),
        (destination: u32, enabled: u32) -> (),
        (destination: GuestAddress, enabled: u32),
        (GuestAddress::new(destination as u32), enabled),
        crate::record_display_selection_buffers::helper_f48fd8,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f4929c,
        helper_f4929c_abi,
        (),
        (_residue0: u32) -> (),
        (),
        (),
        crate::record_display_selection_buffers::helper_f4929c,
        |_| ()
    );
}

pub mod record_hardware_deep_banks {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_edae88,
        helper_edae88_abi,
        (),
        (selector: u32, output: u32) -> u32,
        (selector: u32, output: GuestAddress),
        (selector, GuestAddress::new(output as u32)),
        crate::record_hardware_deep_banks::helper_edae88,
        |value| value
    );
}

pub mod record_hardware_deep_calibration {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_ef5310,
        helper_ef5310_abi,
        (),
        (profile: u32) -> i32,
        (profile: u32),
        (profile),
        crate::record_hardware_deep_calibration::helper_ef5310,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_ef56cc,
        helper_ef56cc_abi,
        (),
        (output: u32, area: u32) -> *mut u32,
        (output: GuestAddress, area: u32),
        (GuestAddress::new(output as u32), area),
        crate::record_hardware_deep_calibration::helper_ef56cc,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_ef5768,
        helper_ef5768_abi,
        (),
        (output: u32) -> (),
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_hardware_deep_calibration::helper_ef5768,
        |_| ()
    );
}

pub mod record_image_command_resources {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_103b054,
        helper_103b054_abi,
        (),
        (output: u32) -> (),
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_image_command_resources::helper_103b054,
        |_| ()
    );
}

pub mod record_image_command_results {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_103b044,
        helper_103b044_abi,
        (),
        (output: u32) -> (),
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_image_command_results::helper_103b044,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_10517f8,
        helper_10517f8_abi,
        crate::record_native_boundary::NativeImageCache,
        (output: u32) -> u32,
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_image_command_results::helper_10517f8,
        |value| value, bounds crate::record_image_command_results::ImageCache
    );
}

pub mod record_image_init_state {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_1050f8c,
        helper_1050f8c_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_image_init_state::helper_1050f8c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_1050fa4,
        helper_1050fa4_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_image_init_state::helper_1050fa4,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_1051fb4,
        helper_1051fb4_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_image_init_state::helper_1051fb4,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_1051fcc,
        helper_1051fcc_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_image_init_state::helper_1051fcc,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_10546d0,
        helper_10546d0_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_image_init_state::helper_10546d0,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_10546e0,
        helper_10546e0_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_image_init_state::helper_10546e0,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f4c384,
        helper_f4c384_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_image_init_state::helper_f4c384,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f4c3b8,
        helper_f4c3b8_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_image_init_state::helper_f4c3b8,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_fea418,
        helper_fea418_abi,
        (),
        (window: u32) -> (),
        (window: GuestAddress),
        (GuestAddress::new(window as u32)),
        crate::record_image_init_state::helper_fea418,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_fea428,
        helper_fea428_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_image_init_state::helper_fea428,
        |_| ()
    );
    #[cfg(target_arch = "arm")]
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_1039610(callback: u32) -> u32 {
        crate::record_native_boundary::with_native_context::<(), _>(|context| {
            let registry = crate::record_address_space::CallbackRegistry::<_, 1>::new();
            match helper_1039610_abi(context, callback, &registry) {
                Ok(value) => value,
                Err(_) => crate::record_native_boundary::fault(),
            }
        })
    }

    #[allow(dead_code)]
    pub fn helper_1039610_abi<M: crate::record_address_space::GuestAddressSpace, E, const N: usize>(
        context: &mut crate::record_address_space::RecordContext<M, E>,
        callback: u32,
        registry: &crate::record_address_space::CallbackRegistry<crate::record_address_space::RecordContext<M, E>, N>,
    ) -> Result<u32, crate::record_native_boundary::AbiFault> {
        let callback = match callback { 0 => None, _ => return Err(crate::record_native_boundary::AbiFault) };
        crate::record_image_init_state::helper_1039610(context, callback, registry)
            .map_err(crate::record_native_boundary::AbiFault::from)
    }
}

pub mod record_job_backend_configuration {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_f5ecb0,
        helper_f5ecb0_abi,
        (),
        (kind: u32, mode: u32, flags: u32) -> u32,
        (kind: u32, mode: u32, flags: u32),
        (kind, mode, flags),
        crate::record_job_backend_configuration::helper_f5ecb0,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f5ecc0,
        helper_f5ecc0_abi,
        (),
        (mode: u32, flags: u32, alternate: u32, timing: u32) -> i32,
        (mode: u32, flags: u32, alternate: u32, timing: u32),
        (mode, flags, alternate, timing),
        crate::record_job_backend_configuration::helper_f5ecc0,
        |value| value
    );
}

#[cfg(target_arch = "arm")]
pub mod record_numeric_grid {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_fa8520,
        helper_fa8520_abi,
        (),
        (profile: u32, channel: u32) -> (),
        (profile: u32, channel: u32),
        (profile, channel),
        crate::record_numeric_grid::helper_fa8520,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_fa898c,
        helper_fa898c_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_numeric_grid::helper_fa898c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_fa91b0,
        helper_fa91b0_abi,
        (),
        (input: u32, channel: u32) -> (),
        (input: GuestAddress, channel: u32),
        (GuestAddress::new(input as u32), channel),
        crate::record_numeric_grid::helper_fa91b0,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_fa95a8,
        helper_fa95a8_abi,
        (),
        (key: u32, output: *mut u32) -> u32,
        (key: u32, output: GuestAddress),
        (key, GuestAddress::new(output as u32)),
        crate::record_numeric_grid::helper_fa95a8,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_fa95b8,
        helper_fa95b8_abi,
        (),
        (profile: u32, input: u32, mode: u32, previous_x: *mut u32, previous_y: *mut u32) -> (),
        (profile: u32, input: GuestAddress, mode: u32, previous_x: GuestAddress, previous_y: GuestAddress),
        (profile, GuestAddress::new(input as u32), mode, GuestAddress::new(previous_x as u32), GuestAddress::new(previous_y as u32)),
        crate::record_numeric_grid::helper_fa95b8,
        |_| ()
    );
}

pub mod record_profile_accessors {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_74e020,
        helper_74e020_abi,
        (),
        (handle: u32, _ignored: u32, low: u32, high: u32) -> u32,
        (handle: u32, _ignored: u32, low: u32, high: u32),
        (handle, _ignored, low, high),
        crate::record_profile_accessors::helper_74e020,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_74e1a4,
        helper_74e1a4_abi,
        (),
        (handle: u32, source: u32) -> (),
        (handle: u32, source: GuestAddress),
        (handle, GuestAddress::new(source as u32)),
        crate::record_profile_accessors::helper_74e1a4,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_74e284,
        helper_74e284_abi,
        (),
        (handle: u32, source: u32, index: u32) -> (),
        (handle: u32, source: GuestAddress, index: u32),
        (handle, GuestAddress::new(source as u32), index),
        crate::record_profile_accessors::helper_74e284,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_74e68c,
        helper_74e68c_abi,
        (),
        (_handle: u32) -> u32,
        (_handle: u32),
        (_handle),
        crate::record_profile_accessors::helper_74e68c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_74e6ac,
        helper_74e6ac_abi,
        (),
        (handle: u32, value: u32) -> (),
        (handle: u32, value: u32),
        (handle, value),
        crate::record_profile_accessors::helper_74e6ac,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_74e7a0,
        helper_74e7a0_abi,
        (),
        (_handle: u32) -> u32,
        (_handle: u32),
        (_handle),
        crate::record_profile_accessors::helper_74e7a0,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_74e7c4,
        helper_74e7c4_abi,
        (),
        (handle: u32, value: u32) -> (),
        (handle: u32, value: u32),
        (handle, value),
        crate::record_profile_accessors::helper_74e7c4,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_74fab4,
        helper_74fab4_abi,
        (),
        (destination: *mut u32, handle: u32) -> *mut u32,
        (destination: GuestAddress, handle: u32),
        (GuestAddress::new(destination as u32), handle),
        crate::record_profile_accessors::helper_74fab4,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_74ff0c,
        helper_74ff0c_abi,
        (),
        (destination: *mut u32, handle: u32) -> *mut u32,
        (destination: GuestAddress, handle: u32),
        (GuestAddress::new(destination as u32), handle),
        crate::record_profile_accessors::helper_74ff0c,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_750a4c,
        helper_750a4c_abi,
        (),
        (destination: *mut u32, handle: u32) -> *mut u32,
        (destination: GuestAddress, handle: u32),
        (GuestAddress::new(destination as u32), handle),
        crate::record_profile_accessors::helper_750a4c,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_75a7c4,
        helper_75a7c4_abi,
        (),
        (handle: u32, field: u32, access: u32, tag: u32) -> u32,
        (handle: u32, field: u32, access: u32, tag: u32),
        (handle, field, access, tag),
        crate::record_profile_accessors::helper_75a7c4,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_75a8c0,
        helper_75a8c0_abi,
        (),
        (handle: u32, field: u32) -> u32,
        (handle: u32, field: u32),
        (handle, field),
        crate::record_profile_accessors::helper_75a8c0,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_75a960,
        helper_75a960_abi,
        (),
        (first: u32, second: u32) -> u32,
        (first: u32, second: u32),
        (first, second),
        crate::record_profile_accessors::helper_75a960,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_75ace4,
        helper_75ace4_abi,
        (),
        (resource: u32, _ignored: u32, low: u32, high: u32) -> u32,
        (resource: GuestAddress, _ignored: u32, low: u32, high: u32),
        (GuestAddress::new(resource as u32), _ignored, low, high),
        crate::record_profile_accessors::helper_75ace4,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_75ae3c,
        helper_75ae3c_abi,
        (),
        (destination: u32, source: u32) -> (),
        (destination: GuestAddress, source: GuestAddress),
        (GuestAddress::new(destination as u32), GuestAddress::new(source as u32)),
        crate::record_profile_accessors::helper_75ae3c,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_75ae78,
        helper_75ae78_abi,
        (),
        (destination: u32, source: *const u32) -> (),
        (destination: GuestAddress, source: GuestAddress),
        (GuestAddress::new(destination as u32), GuestAddress::new(source as u32)),
        crate::record_profile_accessors::helper_75ae78,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_75ae8c,
        helper_75ae8c_abi,
        (),
        (destination: u32, source: u32) -> (),
        (destination: GuestAddress, source: GuestAddress),
        (GuestAddress::new(destination as u32), GuestAddress::new(source as u32)),
        crate::record_profile_accessors::helper_75ae8c,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_75af54,
        helper_75af54_abi,
        (),
        (destination: u32, source: u32) -> (),
        (destination: GuestAddress, source: GuestAddress),
        (GuestAddress::new(destination as u32), GuestAddress::new(source as u32)),
        crate::record_profile_accessors::helper_75af54,
        |_| ()
    );
}

pub mod record_profile_aux_native {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_75d490,
        helper_75d490_abi,
        (),
        (record: u32, value: u32) -> u32,
        (record: GuestAddress, value: u32),
        (GuestAddress::new(record as u32), value),
        crate::record_profile_aux_native::helper_75d490,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_75e6f0,
        helper_75e6f0_abi,
        (),
        (command: u32) -> u32,
        (command: u32),
        (command),
        crate::record_profile_aux_native::helper_75e6f0,
        |value| value
    );
}

pub mod record_profile_extensionfields {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_752110,
        helper_752110_abi,
        (),
        (handle: u32, output: u32) -> (),
        (handle: u32, output: GuestAddress),
        (handle, GuestAddress::new(output as u32)),
        crate::record_profile_extensionfields::helper_752110,
        |_| ()
    );
}

pub mod record_profile_globals {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_74a214,
        helper_74a214_abi,
        (),
        (resource: u32) -> u32,
        (resource: u32),
        (resource),
        crate::record_profile_globals::helper_74a214,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_74a740,
        helper_74a740_abi,
        (),
        (high: u32, low: u32) -> u32,
        (high: u32, low: u32),
        (high, low),
        crate::record_profile_globals::helper_74a740,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_76d9c8,
        helper_76d9c8_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_profile_globals::helper_76d9c8,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_76da48,
        helper_76da48_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_profile_globals::helper_76da48,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_76da84,
        helper_76da84_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_profile_globals::helper_76da84,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_76dac4,
        helper_76dac4_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_profile_globals::helper_76dac4,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_76dad4,
        helper_76dad4_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_profile_globals::helper_76dad4,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_76f340,
        helper_76f340_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_profile_globals::helper_76f340,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_76f360,
        helper_76f360_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_profile_globals::helper_76f360,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_772f28,
        helper_772f28_abi,
        (),
        (index: u32) -> u32,
        (index: u32),
        (index),
        crate::record_profile_globals::helper_772f28,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_772ff8,
        helper_772ff8_abi,
        (),
        (value: u32) -> u32,
        (value: u32),
        (value),
        crate::record_profile_globals::helper_772ff8,
        |value| value
    );
}

pub mod record_profile_mapping {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_740888,
        helper_740888_abi,
        (),
        (first: *mut u32, second: u32, third: u32, fourth: u32) -> (),
        (first: GuestAddress, second: GuestAddress, third: GuestAddress, fourth: GuestAddress),
        (GuestAddress::new(first as u32), GuestAddress::new(second as u32), GuestAddress::new(third as u32), GuestAddress::new(fourth as u32)),
        crate::record_profile_mapping::helper_740888,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_74677c,
        helper_74677c_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_profile_mapping::helper_74677c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_746834,
        helper_746834_abi,
        (),
        (value: u32) -> u32,
        (value: u32),
        (value),
        crate::record_profile_mapping::helper_746834,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_746864,
        helper_746864_abi,
        (),
        (value: u32, bank: u32) -> u32,
        (value: u32, bank: u32),
        (value, bank),
        crate::record_profile_mapping::helper_746864,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_746944,
        helper_746944_abi,
        (),
        (value: u32, bank: u32) -> u32,
        (value: u32, bank: u32),
        (value, bank),
        crate::record_profile_mapping::helper_746944,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_7469a0,
        helper_7469a0_abi,
        (),
        (value: u32) -> u32,
        (value: u32),
        (value),
        crate::record_profile_mapping::helper_7469a0,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_746a80,
        helper_746a80_abi,
        (),
        (value: u32, bank: u32) -> u32,
        (value: u32, bank: u32),
        (value, bank),
        crate::record_profile_mapping::helper_746a80,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_746bf8,
        helper_746bf8_abi,
        (),
        (value: u32, bank: u32) -> u32,
        (value: u32, bank: u32),
        (value, bank),
        crate::record_profile_mapping::helper_746bf8,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_746c2c,
        helper_746c2c_abi,
        (),
        (bank: u32) -> u32,
        (bank: u32),
        (bank),
        crate::record_profile_mapping::helper_746c2c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_74714c,
        helper_74714c_abi,
        (),
        (source: u32, value: u32, destination: u32) -> u32,
        (source: u32, value: u32, destination: u32),
        (source, value, destination),
        crate::record_profile_mapping::helper_74714c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_747374,
        helper_747374_abi,
        (),
        (bank: u32, selector: u32) -> u32,
        (bank: u32, selector: u32),
        (bank, selector),
        crate::record_profile_mapping::helper_747374,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_747aac,
        helper_747aac_abi,
        (),
        (value: u32, from: u32, to: u32) -> u32,
        (value: u32, from: u32, to: u32),
        (value, from, to),
        crate::record_profile_mapping::helper_747aac,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_74947c,
        helper_74947c_abi,
        (),
        (value: u32, bank: u32, source: u32, destination: u32, _unused_a: u32, _unused_b: u32, direction: u32) -> u32,
        (value: u32, bank: u32, source: u32, destination: u32, _unused_a: u32, _unused_b: u32, direction: u32),
        (value, bank, source, destination, _unused_a, _unused_b, direction),
        crate::record_profile_mapping::helper_74947c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_75c874,
        helper_75c874_abi,
        (),
        (profile: u32) -> u32,
        (profile: GuestAddress),
        (GuestAddress::new(profile as u32)),
        crate::record_profile_mapping::helper_75c874,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_75c8a4,
        helper_75c8a4_abi,
        (),
        (profile: u32) -> u32,
        (profile: GuestAddress),
        (GuestAddress::new(profile as u32)),
        crate::record_profile_mapping::helper_75c8a4,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_76dbb4,
        helper_76dbb4_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_profile_mapping::helper_76dbb4,
        |value| value
    );
}

pub mod record_profile_merge {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_759250,
        helper_759250_abi,
        (),
        (destination: u32, source: u32) -> (),
        (destination: u32, source: GuestAddress),
        (destination, GuestAddress::new(source as u32)),
        crate::record_profile_merge::helper_759250,
        |_| ()
    );
}

pub mod record_profile_merge_accessors {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_758f20,
        helper_758f20_abi,
        (),
        (handle: u32) -> u32,
        (handle: u32),
        (handle),
        crate::record_profile_merge_accessors::helper_758f20,
        |value| value
    );
}

pub mod record_profile_parameter_writers {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_751d3c,
        helper_751d3c_abi,
        (),
        (output: u32, handle: u32) -> *mut u32,
        (output: GuestAddress, handle: u32),
        (GuestAddress::new(output as u32), handle),
        crate::record_profile_parameter_writers::helper_751d3c,
        |value: GuestAddress| value.get() as *mut u32
    );
}

pub mod record_profile_shared_accessors {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_75738c,
        helper_75738c_abi,
        (),
        (_handle: u32) -> u32,
        (_handle: u32),
        (_handle),
        crate::record_profile_shared_accessors::helper_75738c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_75a464,
        helper_75a464_abi,
        (),
        (handle: u32, index: u32, source: *const u32) -> (),
        (handle: u32, index: u32, source: GuestAddress),
        (handle, index, GuestAddress::new(source as u32)),
        crate::record_profile_shared_accessors::helper_75a464,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_75a4ac,
        helper_75a4ac_abi,
        (),
        (_handle: u32, index: u32, destination: *mut u32) -> (),
        (_handle: u32, index: u32, destination: GuestAddress),
        (_handle, index, GuestAddress::new(destination as u32)),
        crate::record_profile_shared_accessors::helper_75a4ac,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_75a4ec,
        helper_75a4ec_abi,
        (),
        (handle: u32, destination: *mut u32) -> (),
        (handle: u32, destination: GuestAddress),
        (handle, GuestAddress::new(destination as u32)),
        crate::record_profile_shared_accessors::helper_75a4ec,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_75a52c,
        helper_75a52c_abi,
        (),
        (_handle: u32, source: *const u32) -> (),
        (_handle: u32, source: GuestAddress),
        (_handle, GuestAddress::new(source as u32)),
        crate::record_profile_shared_accessors::helper_75a52c,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_75a5ac,
        helper_75a5ac_abi,
        (),
        (handle: u32, value: u32) -> (),
        (handle: u32, value: u32),
        (handle, value),
        crate::record_profile_shared_accessors::helper_75a5ac,
        |_| ()
    );
}

pub mod record_profile_shared_cpu {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_75624c,
        helper_75624c_abi,
        (),
        (handle: u32) -> u32,
        (handle: u32),
        (handle),
        crate::record_profile_shared_cpu::helper_75624c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_7562e4,
        helper_7562e4_abi,
        (),
        (handle: u32, index: u32) -> *mut u32,
        (handle: u32, index: u32),
        (handle, index),
        crate::record_profile_shared_cpu::helper_7562e4,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_75641c,
        helper_75641c_abi,
        (),
        (handle: u32, destination: u32) -> *mut u32,
        (handle: u32, destination: GuestAddress),
        (handle, GuestAddress::new(destination as u32)),
        crate::record_profile_shared_cpu::helper_75641c,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_756734,
        helper_756734_abi,
        (),
        (_handle: u32, destination: *mut u32) -> *mut u32,
        (_handle: u32, destination: GuestAddress),
        (_handle, GuestAddress::new(destination as u32)),
        crate::record_profile_shared_cpu::helper_756734,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_756754,
        helper_756754_abi,
        (),
        (handle: u32, source: *const u32) -> u32,
        (handle: u32, source: GuestAddress),
        (handle, GuestAddress::new(source as u32)),
        crate::record_profile_shared_cpu::helper_756754,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_7567b8,
        helper_7567b8_abi,
        (),
        (handle: u32, source: *const u32) -> u32,
        (handle: u32, source: GuestAddress),
        (handle, GuestAddress::new(source as u32)),
        crate::record_profile_shared_cpu::helper_7567b8,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_756e5c,
        helper_756e5c_abi,
        (),
        (handle: u32, source: *const u32) -> u32,
        (handle: u32, source: GuestAddress),
        (handle, GuestAddress::new(source as u32)),
        crate::record_profile_shared_cpu::helper_756e5c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_756f3c,
        helper_756f3c_abi,
        (),
        (destination: *mut u32, _residue1: u32) -> *mut u32,
        (destination: GuestAddress),
        (GuestAddress::new(destination as u32)),
        crate::record_profile_shared_cpu::helper_756f3c,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_756f60,
        helper_756f60_abi,
        (),
        (handle: u32, source: *const u32) -> u32,
        (handle: u32, source: GuestAddress),
        (handle, GuestAddress::new(source as u32)),
        crate::record_profile_shared_cpu::helper_756f60,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_7585dc,
        helper_7585dc_abi,
        (),
        (handle: u32, source: u32) -> (),
        (handle: u32, source: GuestAddress),
        (handle, GuestAddress::new(source as u32)),
        crate::record_profile_shared_cpu::helper_7585dc,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_7588e8,
        helper_7588e8_abi,
        (),
        (destination: *mut u32, _residue1: u32) -> *mut u32,
        (destination: GuestAddress),
        (GuestAddress::new(destination as u32)),
        crate::record_profile_shared_cpu::helper_7588e8,
        |value: GuestAddress| value.get() as *mut u32
    );
}

pub mod record_profile_slot_accessors {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_75502c,
        helper_75502c_abi,
        (),
        (handle: u32) -> u32,
        (handle: u32),
        (handle),
        crate::record_profile_slot_accessors::helper_75502c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_75516c,
        helper_75516c_abi,
        (),
        (handle: u32) -> u32,
        (handle: u32),
        (handle),
        crate::record_profile_slot_accessors::helper_75516c,
        |value| value
    );
}

pub mod record_profile_slotfields {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_752714,
        helper_752714_abi,
        (),
        (handle: u32) -> u32,
        (handle: u32),
        (handle),
        crate::record_profile_slotfields::helper_752714,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_752928,
        helper_752928_abi,
        (),
        (output: *mut u32, handle: u32, index: u32) -> *mut u32,
        (output: GuestAddress, handle: u32, index: u32),
        (GuestAddress::new(output as u32), handle, index),
        crate::record_profile_slotfields::helper_752928,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_754edc,
        helper_754edc_abi,
        (),
        (output: *mut u32, handle: u32) -> *mut u32,
        (output: GuestAddress, handle: u32),
        (GuestAddress::new(output as u32), handle),
        crate::record_profile_slotfields::helper_754edc,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_7557dc,
        helper_7557dc_abi,
        (),
        (handle: u32, index: u32) -> u32,
        (handle: u32, index: u32),
        (handle, index),
        crate::record_profile_slotfields::helper_7557dc,
        |value| value
    );
}

pub mod record_profile_status_accessors {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_756e90,
        helper_756e90_abi,
        (),
        (output: *mut u32, _handle: u32) -> *mut u32,
        (output: GuestAddress, _handle: u32),
        (GuestAddress::new(output as u32), _handle),
        crate::record_profile_status_accessors::helper_756e90,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_756ebc,
        helper_756ebc_abi,
        (),
        (handle: u32, record: *const u32) -> u32,
        (handle: u32, record: GuestAddress),
        (handle, GuestAddress::new(record as u32)),
        crate::record_profile_status_accessors::helper_756ebc,
        |value| value
    );
}

pub mod record_scan_bounds_modes {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_f4a520,
        helper_f4a520_abi,
        (),
        (profile: u32, kind: u32) -> u32,
        (profile: u32, kind: u32),
        (profile, kind),
        crate::record_scan_bounds_modes::helper_f4a520,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f4a57c,
        helper_f4a57c_abi,
        (),
        (mode: u32) -> u32,
        (mode: u32),
        (mode),
        crate::record_scan_bounds_modes::helper_f4a57c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f4a848,
        helper_f4a848_abi,
        (),
        (previous: u32, proposed: u32, kind: u32) -> u32,
        (previous: u32, proposed: u32, kind: u32),
        (previous, proposed, kind),
        crate::record_scan_bounds_modes::helper_f4a848,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f4acfc,
        helper_f4acfc_abi,
        (),
        (mode: u32) -> u32,
        (mode: u32),
        (mode),
        crate::record_scan_bounds_modes::helper_f4acfc,
        |value| value
    );
    #[cfg(target_arch = "arm")]
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_f49fd8(mode: u32) -> u32 {
        match helper_f49fd8_abi(mode) { Ok(value) => value, Err(_) => crate::record_native_boundary::fault() }
    }

    #[allow(dead_code)]
    pub fn helper_f49fd8_abi(mode: u32) -> Result<u32, crate::record_native_boundary::AbiFault> {
        Ok(crate::record_scan_bounds_modes::helper_f49fd8(mode))
    }

    #[cfg(target_arch = "arm")]
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_f4a0e4(value: u32) -> u32 {
        match helper_f4a0e4_abi(value) { Ok(value) => value, Err(_) => crate::record_native_boundary::fault() }
    }

    #[allow(dead_code)]
    pub fn helper_f4a0e4_abi(value: u32) -> Result<u32, crate::record_native_boundary::AbiFault> {
        Ok(crate::record_scan_bounds_modes::helper_f4a0e4(value))
    }

    #[cfg(target_arch = "arm")]
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_f4a154(previous: u32, proposed: u32) -> u32 {
        match helper_f4a154_abi(previous, proposed) { Ok(value) => value, Err(_) => crate::record_native_boundary::fault() }
    }

    #[allow(dead_code)]
    pub fn helper_f4a154_abi(previous: u32, proposed: u32) -> Result<u32, crate::record_native_boundary::AbiFault> {
        Ok(crate::record_scan_bounds_modes::helper_f4a154(previous, proposed))
    }
}

pub mod record_scan_bounds_votes {
    use super::stock_export;
    use crate::record_address_space::{GuestAddress, GuestAddressSpace};
    stock_export!(
        xm1_power_record_helper_f4bc58,
        helper_f4bc58_abi,
        (),
        (profile: u32, _kind: u32, selection: u32) -> u32,
        (profile: u32, _kind: u32, selection: u32),
        (profile, _kind, selection),
        crate::record_scan_bounds_votes::helper_f4bc58,
        |value| value
    );
    #[cfg(target_arch = "arm")]
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_f4b49c(profile: u32, kind: u32, selection: u32, flags: *mut u16) {
        crate::record_native_boundary::with_native_context::<(), _>(|context| {
            match helper_f4b49c_abi(context, profile, kind, selection, GuestAddress::new(flags as u32)) {
                Ok(()) => (),
                Err(_) => crate::record_native_boundary::fault(),
            }
        })
    }

    #[allow(dead_code)]
    pub fn helper_f4b49c_abi<M: crate::record_address_space::GuestAddressSpace, E>(
        context: &mut crate::record_address_space::RecordContext<M, E>,
        profile: u32,
        kind: u32,
        selection: u32,
        flags: GuestAddress,
    ) -> Result<(), crate::record_native_boundary::AbiFault> {
        let value = context.read_u16(flags)?;
        let value = crate::record_scan_bounds_votes::helper_f4b49c(context, profile, kind, selection, value)?;
        context.write_u16(flags, value)?;
        Ok(())
    }
}

pub mod record_scan_core_image {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_10538a4,
        helper_10538a4_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_scan_core_image::helper_10538a4,
        |value| value
    );
}

pub mod record_scan_messages {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_f4f16c,
        helper_f4f16c_abi,
        (),
        (resource: u32, _selector: u32) -> u32,
        (resource: u32, _selector: u32),
        (resource, _selector),
        crate::record_scan_messages::helper_f4f16c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f51b94,
        helper_f51b94_abi,
        (),
        (resource: u32, selector: u32) -> u32,
        (resource: u32, selector: u32),
        (resource, selector),
        crate::record_scan_messages::helper_f51b94,
        |value| value
    );
}

pub mod record_scan_publication_history {
    use super::stock_export;
    use crate::record_address_space::{GuestAddress, GuestAddressSpace};
    stock_export!(
        xm1_power_record_helper_f4d884,
        helper_f4d884_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_scan_publication_history::helper_f4d884,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f4d8bc,
        helper_f4d8bc_abi,
        (),
        (mode: u32, flags: u32) -> (),
        (mode: u32, flags: u32),
        (mode, flags),
        crate::record_scan_publication_history::helper_f4d8bc,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f4d968,
        helper_f4d968_abi,
        (),
        (mode: u32, flags: u32) -> (),
        (mode: u32, flags: u32),
        (mode, flags),
        crate::record_scan_publication_history::helper_f4d968,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f4dbac,
        helper_f4dbac_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_scan_publication_history::helper_f4dbac,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f4dc18,
        helper_f4dc18_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_scan_publication_history::helper_f4dc18,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f4dc50,
        helper_f4dc50_abi,
        (),
        (input: u32) -> (),
        (input: GuestAddress),
        (GuestAddress::new(input as u32)),
        crate::record_scan_publication_history::helper_f4dc50,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f4dd50,
        helper_f4dd50_abi,
        (),
        (source: u32) -> (),
        (source: GuestAddress),
        (GuestAddress::new(source as u32)),
        crate::record_scan_publication_history::helper_f4dd50,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f4ddbc,
        helper_f4ddbc_abi,
        (),
        (mode: u32, flags: u32) -> u32,
        (mode: u32, flags: u32),
        (mode, flags),
        crate::record_scan_publication_history::helper_f4ddbc,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f4dea4,
        helper_f4dea4_abi,
        (),
        (input: u32) -> (),
        (input: GuestAddress),
        (GuestAddress::new(input as u32)),
        crate::record_scan_publication_history::helper_f4dea4,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f4df4c,
        helper_f4df4c_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_scan_publication_history::helper_f4df4c,
        |_| ()
    );
    #[cfg(target_arch = "arm")]
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_f4a014(code: u32, selection: *mut u32, flags: *mut u16) {
        crate::record_native_boundary::with_native_context::<(), _>(|context| {
            match helper_f4a014_abi(context, code, GuestAddress::new(selection as u32), GuestAddress::new(flags as u32)) {
                Ok(()) => (),
                Err(_) => crate::record_native_boundary::fault(),
            }
        })
    }

    #[allow(dead_code)]
    pub fn helper_f4a014_abi<M: crate::record_address_space::GuestAddressSpace, E>(
        context: &mut crate::record_address_space::RecordContext<M, E>,
        code: u32,
        selection: GuestAddress,
        flags: GuestAddress,
    ) -> Result<(), crate::record_native_boundary::AbiFault> {
        let (value, flag) = crate::record_scan_publication_history::helper_f4a014(context, code)?;
        context.write_u32(selection, value)?;
        context.write_u16(flags, flag)?;
        Ok(())
    }

    #[cfg(target_arch = "arm")]
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_f4d790(mode: u32, selection: *mut u32, flags: *mut u16) {
        crate::record_native_boundary::with_native_context::<(), _>(|context| {
            match helper_f4d790_abi(context, mode, GuestAddress::new(selection as u32), GuestAddress::new(flags as u32)) {
                Ok(()) => (),
                Err(_) => crate::record_native_boundary::fault(),
            }
        })
    }

    #[allow(dead_code)]
    pub fn helper_f4d790_abi<M: crate::record_address_space::GuestAddressSpace, E>(
        context: &mut crate::record_address_space::RecordContext<M, E>,
        mode: u32,
        selection: GuestAddress,
        flags: GuestAddress,
    ) -> Result<(), crate::record_native_boundary::AbiFault> {
        let (value, flag) = crate::record_scan_publication_history::helper_f4d790(context, mode)?;
        context.write_u32(selection, value)?;
        context.write_u16(flags, flag)?;
        Ok(())
    }
}

pub mod record_scan_publication_workspace {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_fb2438,
        helper_fb2438_abi,
        crate::record_native_boundary::NativeCachePublication,
        (output: u32) -> (),
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_scan_publication_workspace::helper_fb2438,
        |_| (), bounds crate::record_scan_publication_workspace::CachePublication
    );
    stock_export!(
        xm1_power_record_helper_fb259c,
        helper_fb259c_abi,
        crate::record_native_boundary::NativeCachePublication,
        (output: u32) -> (),
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_scan_publication_workspace::helper_fb259c,
        |_| (), bounds crate::record_scan_publication_workspace::CachePublication
    );
    stock_export!(
        xm1_power_record_helper_fb26a4,
        helper_fb26a4_abi,
        crate::record_native_boundary::NativeCachePublication,
        (output: u32) -> (),
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_scan_publication_workspace::helper_fb26a4,
        |_| (), bounds crate::record_scan_publication_workspace::CachePublication
    );
    stock_export!(
        xm1_power_record_helper_fb27dc,
        helper_fb27dc_abi,
        crate::record_native_boundary::NativeCachePublication,
        (output: u32) -> (),
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_scan_publication_workspace::helper_fb27dc,
        |_| (), bounds crate::record_scan_publication_workspace::CachePublication
    );
    stock_export!(
        xm1_power_record_helper_fb35d8,
        helper_fb35d8_abi,
        crate::record_native_boundary::NativeCachePublication,
        (output: u32) -> (),
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_scan_publication_workspace::helper_fb35d8,
        |_| (), bounds crate::record_scan_publication_workspace::CachePublication
    );
}

pub mod record_statistics_getters {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_dca798,
        helper_dca798_abi,
        (),
        (output: *mut u32, mode: u32, dimensions: *const u32) -> u32,
        (output: GuestAddress, mode: u32, dimensions: GuestAddress),
        (GuestAddress::new(output as u32), mode, GuestAddress::new(dimensions as u32)),
        crate::record_statistics_getters::helper_dca798,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_ee8bb0,
        helper_ee8bb0_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_statistics_getters::helper_ee8bb0,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_ee8e70,
        helper_ee8e70_abi,
        (),
        (output: u32) -> u32,
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_statistics_getters::helper_ee8e70,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_ee96e0,
        helper_ee96e0_abi,
        (),
        (output: u32) -> *mut u32,
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_statistics_getters::helper_ee96e0,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_ee96f4,
        helper_ee96f4_abi,
        (),
        (output: u32) -> *mut u32,
        (output: GuestAddress),
        (GuestAddress::new(output as u32)),
        crate::record_statistics_getters::helper_ee96f4,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_eea030,
        helper_eea030_abi,
        (),
        (channel: *mut u32, first: *mut u32, second: *mut u32) -> (),
        (channel: Option<GuestAddress>, first: Option<GuestAddress>, second: Option<GuestAddress>),
        (if channel.is_null() { None } else { Some(GuestAddress::new(channel as u32)) }, if first.is_null() { None } else { Some(GuestAddress::new(first as u32)) }, if second.is_null() { None } else { Some(GuestAddress::new(second as u32)) }),
        crate::record_statistics_getters::helper_eea030,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_eea094,
        helper_eea094_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_statistics_getters::helper_eea094,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_ef53d0,
        helper_ef53d0_abi,
        (),
        (index: u32) -> u32,
        (index: u32),
        (index),
        crate::record_statistics_getters::helper_ef53d0,
        |value| value
    );
}

pub mod record_statistics_numeric {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_ee9928,
        helper_ee9928_abi,
        (),
        (value: u32, output: *mut u32) -> u32,
        (value: u32, output: GuestAddress),
        (value, GuestAddress::new(output as u32)),
        crate::record_statistics_numeric::helper_ee9928,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_ee9ab0,
        helper_ee9ab0_abi,
        (),
        (value: u32) -> u32,
        (value: i32),
        (value as i32),
        crate::record_statistics_numeric::helper_ee9ab0,
        |value| value
    );
}

#[cfg(target_arch = "arm")]
pub mod record_task43_active_selections {
    use super::stock_export;
    use crate::record_address_space::{GuestAddress, GuestAddressSpace};
    stock_export!(
        xm1_power_record_helper_f32530,
        helper_f32530_abi,
        (),
        (mask: u32) -> (),
        (mask: u32),
        (mask),
        crate::record_task43_active_selections::helper_f32530,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f32558,
        helper_f32558_abi,
        (),
        (mask: u32) -> (),
        (mask: u32),
        (mask),
        crate::record_task43_active_selections::helper_f32558,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f32580,
        helper_f32580_abi,
        (),
        (mask: u32) -> (),
        (mask: u32),
        (mask),
        crate::record_task43_active_selections::helper_f32580,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f325a8,
        helper_f325a8_abi,
        (),
        (mask: u32) -> u32,
        (mask: u32),
        (mask),
        crate::record_task43_active_selections::helper_f325a8,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f32c74,
        helper_f32c74_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_active_selections::helper_f32c74,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f32dd8,
        helper_f32dd8_abi,
        (),
        (status: u32, stage: u32, output: u32) -> (),
        (status: u32, stage: u32, output: GuestAddress),
        (status, stage, GuestAddress::new(output as u32)),
        crate::record_task43_active_selections::helper_f32dd8,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f32e60,
        helper_f32e60_abi,
        (),
        (category: u32) -> *mut u32,
        (category: u32),
        (category),
        crate::record_task43_active_selections::helper_f32e60,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_f32f80,
        helper_f32f80_abi,
        (),
        (first: u32, second: u32) -> u32,
        (first: u32, second: u32),
        (first, second),
        crate::record_task43_active_selections::helper_f32f80,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f330e4,
        helper_f330e4_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_task43_active_selections::helper_f330e4,
        |value| value
    );
    #[cfg(target_arch = "arm")]
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_f32cc0(flags: u32, first: *mut u32, second: *mut u32) {
        crate::record_native_boundary::with_native_context::<(), _>(|context| {
            match helper_f32cc0_abi(context, flags, GuestAddress::new(first as u32), GuestAddress::new(second as u32)) {
                Ok(()) => (),
                Err(_) => crate::record_native_boundary::fault(),
            }
        })
    }

    #[allow(dead_code)]
    pub fn helper_f32cc0_abi<M: crate::record_address_space::GuestAddressSpace, E>(
        context: &mut crate::record_address_space::RecordContext<M, E>,
        flags: u32,
        first: GuestAddress,
        second: GuestAddress,
    ) -> Result<(), crate::record_native_boundary::AbiFault> {
        let pair = crate::record_task43_active_selections::helper_f32cc0(context, flags)?;
        context.write_u32(first, pair.first.get())?;
        context.write_u32(second, pair.second.get())?;
        Ok(())
    }

    #[cfg(target_arch = "arm")]
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_f3300c(first: *mut u8, second: *mut u8) -> u32 {
        crate::record_native_boundary::with_native_context::<(), _>(|context| {
            match helper_f3300c_abi(context, GuestAddress::new(first as u32), GuestAddress::new(second as u32)) {
                Ok(value) => value,
                Err(_) => crate::record_native_boundary::fault(),
            }
        })
    }

    #[allow(dead_code)]
    pub fn helper_f3300c_abi<M: crate::record_address_space::GuestAddressSpace, E>(
        context: &mut crate::record_address_space::RecordContext<M, E>,
        first: GuestAddress,
        second: GuestAddress,
    ) -> Result<u32, crate::record_native_boundary::AbiFault> {
        let masks = crate::record_task43_active_selections::helper_f3300c(context)?;
        context.write_u8(first, masks.first)?;
        context.write_u8(second, masks.second)?;
        Ok(u32::from(masks.changed))
    }
}

pub mod record_task43_helpers18 {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_f2be34,
        helper_f2be34_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_helpers18::helper_f2be34,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f2c3c0,
        helper_f2c3c0_abi,
        (),
        (destination: u32) -> *mut u32,
        (destination: GuestAddress),
        (GuestAddress::new(destination as u32)),
        crate::record_task43_helpers18::helper_f2c3c0,
        |value: GuestAddress| value.get() as *mut u32
    );
    stock_export!(
        xm1_power_record_helper_f2c898,
        helper_f2c898_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_helpers18::helper_f2c898,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f2c8ac,
        helper_f2c8ac_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_helpers18::helper_f2c8ac,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f2cb5c,
        helper_f2cb5c_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_helpers18::helper_f2cb5c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f2cb90,
        helper_f2cb90_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_helpers18::helper_f2cb90,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f2d094,
        helper_f2d094_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_helpers18::helper_f2d094,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f2d0b8,
        helper_f2d0b8_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_helpers18::helper_f2d0b8,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f2d0e4,
        helper_f2d0e4_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_helpers18::helper_f2d0e4,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f2d130,
        helper_f2d130_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_helpers18::helper_f2d130,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f2d154,
        helper_f2d154_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_helpers18::helper_f2d154,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f2d180,
        helper_f2d180_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_helpers18::helper_f2d180,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f2d68c,
        helper_f2d68c_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_helpers18::helper_f2d68c,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f2d79c,
        helper_f2d79c_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_task43_helpers18::helper_f2d79c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f2dcf4,
        helper_f2dcf4_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_helpers18::helper_f2dcf4,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f2ddd4,
        helper_f2ddd4_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_helpers18::helper_f2ddd4,
        |value| value
    );
}

pub mod record_task43_machine23_state {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_f3aa94,
        helper_f3aa94_abi,
        (),
        (event: u32) -> (),
        (event: u32),
        (event),
        crate::record_task43_machine23_state::helper_f3aa94,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f3aab4,
        helper_f3aab4_abi,
        (),
        (value: u32) -> i32,
        (value: u32),
        (value),
        crate::record_task43_machine23_state::helper_f3aab4,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f3adbc,
        helper_f3adbc_abi,
        (),
        (query: u32) -> i32,
        (query: u32),
        (query),
        crate::record_task43_machine23_state::helper_f3adbc,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f3ae2c,
        helper_f3ae2c_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_machine23_state::helper_f3ae2c,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f3ae54,
        helper_f3ae54_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_machine23_state::helper_f3ae54,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f3aec0,
        helper_f3aec0_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_machine23_state::helper_f3aec0,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f3aedc,
        helper_f3aedc_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_machine23_state::helper_f3aedc,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f3c0bc,
        helper_f3c0bc_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_machine23_state::helper_f3c0bc,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f3c0d0,
        helper_f3c0d0_abi,
        (),
        (request: u32) -> (),
        (request: u32),
        (request),
        crate::record_task43_machine23_state::helper_f3c0d0,
        |_| ()
    );
}

pub mod record_task43_machine26_analysis_history {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_f43d94,
        helper_f43d94_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_machine26_analysis_history::helper_f43d94,
        |_| ()
    );
}

pub mod record_task43_machine26_state {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_f3d3e0,
        helper_f3d3e0_abi,
        (),
        (value: u32) -> (),
        (value: u32),
        (value),
        crate::record_task43_machine26_state::helper_f3d3e0,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f3d400,
        helper_f3d400_abi,
        (),
        (value: u32) -> (),
        (value: u32),
        (value),
        crate::record_task43_machine26_state::helper_f3d400,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f3d420,
        helper_f3d420_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_machine26_state::helper_f3d420,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f3d45c,
        helper_f3d45c_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_machine26_state::helper_f3d45c,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f3e32c,
        helper_f3e32c_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_machine26_state::helper_f3e32c,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f3e3c8,
        helper_f3e3c8_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_machine26_state::helper_f3e3c8,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f3eaf0,
        helper_f3eaf0_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_machine26_state::helper_f3eaf0,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f3eb3c,
        helper_f3eb3c_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_machine26_state::helper_f3eb3c,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f3ebd4,
        helper_f3ebd4_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_machine26_state::helper_f3ebd4,
        |value| value
    );
}

pub mod record_task43_resource_forward {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_f47300,
        helper_f47300_abi,
        (),
        (position: u32, zoom: u32, aperture: u32, target: u32, flags: u32, format: u32, direction: u32) -> u32,
        (position: u32, zoom: u32, aperture: u32, target: u32, flags: u32, format: u32, direction: u32),
        (position, zoom, aperture, target, flags, format, direction),
        crate::record_task43_resource_forward::helper_f47300,
        |value| value
    );
}

pub mod record_task43_selection_early_tail {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_f23314,
        helper_f23314_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_selection_early_tail::helper_f23314,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f234d8,
        helper_f234d8_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_task43_selection_early_tail::helper_f234d8,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f234f8,
        helper_f234f8_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_selection_early_tail::helper_f234f8,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f23564,
        helper_f23564_abi,
        (),
        (value: u32) -> (),
        (value: u32),
        (value),
        crate::record_task43_selection_early_tail::helper_f23564,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f23624,
        helper_f23624_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_selection_early_tail::helper_f23624,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f23648,
        helper_f23648_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_task43_selection_early_tail::helper_f23648,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f23668,
        helper_f23668_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_task43_selection_early_tail::helper_f23668,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f2368c,
        helper_f2368c_abi,
        (),
        (value: u32) -> (),
        (value: u32),
        (value),
        crate::record_task43_selection_early_tail::helper_f2368c,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f236ac,
        helper_f236ac_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_task43_selection_early_tail::helper_f236ac,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f236cc,
        helper_f236cc_abi,
        (),
        (handle: u32) -> (),
        (handle: u32),
        (handle),
        crate::record_task43_selection_early_tail::helper_f236cc,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f23880,
        helper_f23880_abi,
        (),
        (value: u32) -> (),
        (value: u32),
        (value),
        crate::record_task43_selection_early_tail::helper_f23880,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f238a0,
        helper_f238a0_abi,
        (),
        () -> u32,
        (),
        (),
        crate::record_task43_selection_early_tail::helper_f238a0,
        |value| value
    );
}

pub mod record_task43_selection_lists {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_f156d4,
        helper_f156d4_abi,
        (),
        () -> (),
        (),
        (),
        crate::record_task43_selection_lists::helper_f156d4,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f156e8,
        helper_f156e8_abi,
        (),
        (item: u32) -> (),
        (item: u32),
        (item),
        crate::record_task43_selection_lists::helper_f156e8,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f15708,
        helper_f15708_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_selection_lists::helper_f15708,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f15cbc,
        helper_f15cbc_abi,
        (),
        (use_three: u32) -> (),
        (use_three: u32),
        (use_three),
        crate::record_task43_selection_lists::helper_f15cbc,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_f165fc,
        helper_f165fc_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_selection_lists::helper_f165fc,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f16664,
        helper_f16664_abi,
        (),
        () -> i32,
        (),
        (),
        crate::record_task43_selection_lists::helper_f16664,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_f17abc,
        helper_f17abc_abi,
        (),
        (destination: *mut u32) -> (),
        (destination: GuestAddress),
        (GuestAddress::new(destination as u32)),
        crate::record_task43_selection_lists::helper_f17abc,
        |_| ()
    );
}

#[cfg(target_arch = "arm")]
pub mod record_image_init_result {
    use crate::record_address_space::GuestAddress;

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_1039580(window: u32, callback: u32) -> u32 {
        crate::record_native_boundary::with_native_context::<crate::record_native_boundary::NativeCriticalKernel, _>(|context| {
            let registry = crate::record_address_space::CallbackRegistry::<_, 1>::new();
            match helper_1039580_abi(context, GuestAddress::new(window), callback, &registry) {
                Ok(value) => value,
                Err(_) => crate::record_native_boundary::fault(),
            }
        })
    }

    fn helper_1039580_abi<M: crate::record_address_space::GuestAddressSpace, E: crate::osal::CriticalKernel, const N: usize>(
        context: &mut crate::record_address_space::RecordContext<M, E>,
        window: GuestAddress,
        callback: u32,
        registry: &crate::record_address_space::CallbackRegistry<crate::record_address_space::RecordContext<M, E>, N>,
    ) -> Result<u32, crate::record_native_boundary::AbiFault> {
        let callback = match callback { 0 => None, _ => return Err(crate::record_native_boundary::AbiFault) };
        crate::record_image_init_result::helper_1039580(context, window, callback, registry)
            .map_err(crate::record_native_boundary::AbiFault::from)
    }
}

pub mod record_image_geometry_query {
    use super::stock_export;
    stock_export!(
        xm1_power_record_helper_ef5ab8,
        helper_ef5ab8_abi,
        (),
        (mode: u32, index: u32) -> u32,
        (profile: u32, kind: u32),
        (mode, index),
        crate::record_image_geometry_query::profiles::helper_ef5ab8,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_ef5cc4,
        helper_ef5cc4_abi,
        (),
        (mode: u32, index: u32) -> u32,
        (profile: u32, kind: u32),
        (mode, index),
        crate::record_image_geometry_query::profiles::helper_ef5cc4,
        |value| value
    );
}

pub mod record_deep_geometry {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    stock_export!(
        xm1_power_record_helper_eda668,
        helper_eda668_abi,
        (),
        (kind: u32, profile: u32, options: u32, first: u32, second: u32, third: u32, output: u32) -> u32,
        (flags: u32, parameters: GuestAddress, compact: GuestAddress, third: GuestAddress, fifth: GuestAddress, sixth: GuestAddress, report: GuestAddress),
        (kind, GuestAddress::new(profile), GuestAddress::new(options), GuestAddress::new(first), GuestAddress::new(second), GuestAddress::new(third), GuestAddress::new(output)),
        crate::record_deep_geometry::helper_eda668,
        |value| value
    );
    stock_export!(
        xm1_power_record_helper_ef6030,
        helper_ef6030_abi,
        (),
        (input: u32, output: u32) -> (),
        (input: GuestAddress, output: GuestAddress),
        (GuestAddress::new(input), GuestAddress::new(output)),
        crate::record_deep_geometry::helper_ef6030,
        |_| ()
    );
}

#[cfg(target_arch = "arm")]
pub mod record_frontend_transfer {
    use super::stock_export;
    use crate::record_address_space::GuestAddress;
    use crate::record_native_boundary::optional_address;
    stock_export!(
        xm1_power_record_helper_ffc3fc,
        helper_ffc3fc_abi,
        (),
        (first: u32, second: u32, third: u32, fourth: u32) -> (),
        (grid: Option<GuestAddress>, grid_dimensions: Option<GuestAddress>, regions: Option<GuestAddress>, region_dimensions: Option<GuestAddress>),
        (optional_address(first), optional_address(second), optional_address(third), optional_address(fourth)),
        crate::record_frontend_transfer::helper_ffc3fc,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_ffc668,
        helper_ffc668_abi,
        (),
        (first: u32, second: u32, report: u32) -> (),
        (grid: Option<GuestAddress>, regions: Option<GuestAddress>, parameters: GuestAddress),
        (optional_address(first), optional_address(second), GuestAddress::new(report)),
        crate::record_frontend_transfer::helper_ffc668,
        |_| ()
    );

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_record_helper_ffc97c(first: u32, second: u32, third: u32, fourth: u32, source: u32, dimensions: u32, gain: i32) {
        crate::record_native_boundary::with_native_context::<(), _>(|context| {
            let Ok(coordinate) = i16::try_from(gain) else { crate::record_native_boundary::fault() };
            match crate::record_frontend_transfer::helper_ffc97c(context, optional_address(first), optional_address(second), optional_address(third), optional_address(fourth), optional_address(source), optional_address(dimensions), coordinate) {
                Ok(()) => (),
                Err(_) => crate::record_native_boundary::fault(),
            }
        })
    }
}
