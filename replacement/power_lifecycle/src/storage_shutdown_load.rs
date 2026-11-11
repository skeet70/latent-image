use crate::storage_shutdown_actor::MediumStatus;
use crate::storage_shutdown_lifecycle::{LensCompletion, LensLoadMode, LensPageOperation};
use crate::storage_shutdown_pages::{ConfigurationAddress, LensPageSoftware, PageNumber};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensChannel { Header, Registered, Full }
impl LensChannel {
    pub const fn abi(self) -> u8 { match self { Self::Header => 0, Self::Registered => 1, Self::Full => 6 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadDiagnostic { Unchanged, Registered, Unregistered, Defaults }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescriptorStatus(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefaultBank { Primary, Extended }

pub trait LensLoadSoftware: LensPageSoftware {
    fn software_status(&self) -> MediumStatus;
    fn request_channel(&mut self, channel: LensChannel);
    fn lens_available(&mut self) -> bool;
    fn snapshot_descriptor(&mut self, destination: &mut [u8; 0x80]) -> DescriptorStatus;
    async fn delay_ticks(&mut self, ticks: u32);
    fn configuration_variant(&mut self) -> [u8; 2];
    async fn synchronize_lens(&mut self);
    fn read_cached_profile(&mut self, slot: u8) -> MediumStatus;
    fn release_lens(&mut self);
    fn reset_lens(&mut self);
    fn read_defaults(&mut self, bank: DefaultBank);
    fn diagnostic(&mut self, diagnostic: LoadDiagnostic);
    fn prepare_output(&mut self, unregistered: bool);
    fn publish_configuration(&mut self);
    async fn finish_load(&mut self);
}

struct SynchronizedLens<'a, S: LensLoadSoftware>(Option<&'a mut S>);
impl<'a, S: LensLoadSoftware> SynchronizedLens<'a, S> {
    fn software(&mut self) -> &mut S { self.0.as_deref_mut().unwrap() }
    fn release(mut self) -> &'a mut S {
        let software = self.0.take().unwrap();
        software.release_lens();
        software
    }
}
impl<S: LensLoadSoftware> Drop for SynchronizedLens<'_, S> {
    fn drop(&mut self) {
        if let Some(software) = self.0.as_deref_mut() { software.release_lens(); }
    }
}

fn half(context: &[u8; 0xc0], offset: usize) -> u16 { u16::from_le_bytes([context[offset], context[offset + 1]]) }
fn put_half(context: &mut [u8; 0xc0], offset: usize, value: u16) { context[offset..offset + 2].copy_from_slice(&value.to_le_bytes()); }
fn word(context: &[u8; 0xc0], offset: usize) -> u32 {
    u32::from_le_bytes([context[offset], context[offset + 1], context[offset + 2], context[offset + 3]])
}
fn put_word(context: &mut [u8; 0xc0], offset: usize, value: u32) { context[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }

pub async fn receive_profile(
    software: &mut impl LensLoadSoftware,
    context: &mut [u8; 0xc0],
    transfer: &mut [u8; 0x800],
    mode: LensLoadMode,
    channel: LensChannel,
) -> MediumStatus {
    put_word(context, 0x34, 0);
    put_half(context, 0x38, 0xff00);
    software.request_channel(channel);
    if software.wait(LensCompletion::Information, context).await.0 != 0 || word(context, 0x14) != 0 { return MediumStatus(-255); }
    match channel {
        LensChannel::Header => { put_half(context, 0x18, 1); put_half(context, 0x1a, 0xffff); }
        LensChannel::Full => {
            let pages = half(context, 0x30);
            put_half(context, 0x18, pages);
            put_word(context, 0x24, 4);
            put_word(context, 0x28, 0);
            put_word(context, 0x2c, u32::from(pages).wrapping_sub(4));
            put_half(context, 0x1a, 1);
        }
        LensChannel::Registered => {
            put_half(context, 0x18, 0x44);
            put_word(context, 0x24, 4);
            put_word(context, 0x28, 4);
            put_word(context, 0x2c, 0x3c);
            put_half(context, 0x1a, 1);
        }
    }
    loop {
        software.request_read(PageNumber(half(context, 0x1a)), LensPageOperation::Bytes2048);
        if software.wait(LensCompletion::Read, context).await.0 != 0 { return MediumStatus(-255); }
        let current = half(context, 0x1a);
        if current == 0xff00 { return software.software_status(); }
        if word(context, 0x14) != 0 { return MediumStatus(-255); }
        put_word(context, 0x34, 0);
        let next = if i32::from(current) < i32::from(half(context, 0x18)) - 1 { current.wrapping_add(1) }
            else if current == 0xffff { 0xff00 } else { 0xffff };
        put_half(context, 0x1a, next);
        if half(context, 0x1c) >= half(context, 0x18) { continue; }
        software.copy_received(word(context, 0x20), transfer);
        let page = u32::from(half(context, 0x1c));
        let prefix = word(context, 0x2c);
        let middle_end = prefix.wrapping_add(word(context, 0x28));
        let (base, index) = if page < prefix { (0x3e2000u32, page) }
            else { (0x3e0000u32, page.wrapping_sub(if page >= middle_end { middle_end } else { prefix }) & 0xffff) };
        let selected = match mode.0 {
            1 => page < prefix,
            2 => page >= middle_end,
            3 => page >= prefix && page < middle_end,
            4 => page < middle_end,
            _ => page < prefix || page >= middle_end,
        };
        if !selected || page >= u32::from(half(context, 0x18)) { continue; }
        if page == middle_end && u16::from_le_bytes([transfer[0x47], transfer[0x48]]) != 0xaa55 { return MediumStatus(-255); }
        if channel == LensChannel::Header {
            software.copy_to_configuration(ConfigurationAddress(0x3e001f), &transfer[0x1f..0x27]);
        } else {
            software.copy_to_configuration(ConfigurationAddress(base.wrapping_add(index << 11)), transfer);
        }
    }
}

pub fn descriptor_matches(descriptor: &[u8; 0x80], entry: &[u8], variant: [u8; 2]) -> bool {
    descriptor[8..16] == entry[0x18..0x20]
        && descriptor[0x50..0x5a] == entry[0x20..0x2a]
        && descriptor[0x5a..0x5c] == entry[0x32..0x34]
        && entry[0x50..0x52] == variant
}

pub async fn load_profile(
    software: &mut impl LensLoadSoftware,
    cache: &[u8; 0xa08],
    context: &mut [u8; 0xc0],
    transfer: &mut [u8; 0x800],
    mode: LensLoadMode,
) -> MediumStatus {
    put_word(context, 0x3c, 0);
    if mode.0 < 8 && ((1u32 << mode.0) & 0x9e) != 0 {
        if receive_profile(software, context, transfer, mode, LensChannel::Registered).await.0 == 0 {
            software.prepare_output(true);
            software.publish_configuration();
            software.finish_load().await;
            return software.software_status();
        }
        software.reset_lens();
    } else if software.lens_available() {
        let mut descriptor = [0; 0x80];
        let status = loop {
            let status = software.snapshot_descriptor(&mut descriptor);
            if status.0 == -255 || status.0 == 0 { break status; }
            software.delay_ticks(10).await;
        };
        if status.0 == 0 {
            if descriptor == context[0x40..0xc0] {
                software.diagnostic(LoadDiagnostic::Unchanged);
                software.finish_load().await;
                return software.software_status();
            }
            let variant = software.configuration_variant();
            let found = (0..usize::from(cache[2])).find(|&slot| descriptor_matches(&descriptor, &cache[slot * 0x80..], variant));
            let mut synchronized = SynchronizedLens(Some(&mut *software));
            synchronized.software().synchronize_lens().await;
            let status = synchronized.software().software_status();
            if status.0 != 0 { return status; }
            let (status, unregistered) = if let Some(slot) = found {
                (synchronized.software().read_cached_profile(slot as u8), false)
            } else {
                (receive_profile(synchronized.software(), context, transfer, mode, LensChannel::Full).await, true)
            };
            if status.0 == 0 {
                synchronized.software().diagnostic(if unregistered { LoadDiagnostic::Unregistered } else { LoadDiagnostic::Registered });
                let software = synchronized.release();
                context[0x40..0xc0].copy_from_slice(&descriptor);
                software.prepare_output(unregistered);
                software.publish_configuration();
                software.finish_load().await;
                return software.software_status();
            }
            if unregistered { synchronized.software().reset_lens(); }
            drop(synchronized);
        }
    }
    put_word(context, 0x3c, 1);
    software.read_defaults(DefaultBank::Primary);
    software.read_defaults(DefaultBank::Extended);
    context[0x40..0xc0].fill(0);
    software.diagnostic(LoadDiagnostic::Defaults);
    software.prepare_output(false);
    software.publish_configuration();
    software.finish_load().await;
    software.software_status()
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::DescriptorStatus;
    extern "C" {
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(saved: u32);
        fn xm1_power_configuration_byte(key: u32) -> u32;
    }
    pub unsafe fn lens_available() -> bool { unsafe { core::ptr::read_volatile(0x95e1d0 as *const u32) == 1 } }
    pub unsafe fn snapshot_descriptor(destination: &mut [u8; 0x80]) -> DescriptorStatus {
        let saved = unsafe { xm1_power_interrupt_lock() };
        for (index, byte) in destination.iter_mut().enumerate() {
            *byte = unsafe { core::ptr::read_volatile((0x93c544 + index) as *const u8) };
        }
        unsafe { xm1_power_interrupt_unlock(saved) };
        DescriptorStatus(unsafe { core::ptr::read_volatile(0x95e1d8 as *const i32) })
    }
    pub unsafe fn configuration_variant() -> [u8; 2] {
        [unsafe { xm1_power_configuration_byte(0x204) } as u8, unsafe { xm1_power_configuration_byte(0x205) } as u8]
    }
    pub unsafe fn reset_lens() { unsafe { core::ptr::write_volatile(0x96b3fa as *mut u8, 1) }; }
    pub unsafe fn auxiliary_size() -> u16 {
        let high = unsafe { core::ptr::read_volatile(0x95caab as *const u8) };
        let low = unsafe { core::ptr::read_volatile(0x95caaa as *const u8) };
        u16::from(low) + (u16::from(high) << 8)
    }
}
