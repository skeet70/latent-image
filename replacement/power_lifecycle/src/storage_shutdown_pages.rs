use crate::storage_shutdown_actor::MediumStatus;
use crate::storage_shutdown_lifecycle::{LensCompletion, LensPageOperation, LensTransferPlan};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageBoundary { Primary, Auxiliary }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageNumber(pub u16);

pub trait LensPageSoftware {
    fn clear_notifications(&mut self);
    fn request_write(&mut self, page: PageNumber, address: ConfigurationAddress, operation: LensPageOperation);
    fn request_read(&mut self, page: PageNumber, operation: LensPageOperation);
    async fn wait(&mut self, completion: LensCompletion, context: &mut [u8; 0xc0]) -> MediumStatus;
    async fn page_boundary(&mut self, boundary: PageBoundary) -> MediumStatus;
    fn copy_received(&mut self, source: u32, destination: &mut [u8]);
    fn copy_to_configuration(&mut self, address: ConfigurationAddress, source: &[u8]);
}

fn half(context: &[u8; 0xc0], offset: usize) -> u16 { u16::from_le_bytes([context[offset], context[offset + 1]]) }
fn put_half(context: &mut [u8; 0xc0], offset: usize, value: u16) { context[offset..offset + 2].copy_from_slice(&value.to_le_bytes()); }
fn word(context: &[u8; 0xc0], offset: usize) -> u32 {
    u32::from_le_bytes([context[offset], context[offset + 1], context[offset + 2], context[offset + 3]])
}

pub async fn write_pages(
    software: &mut impl LensPageSoftware,
    context: &mut [u8; 0xc0],
    base: ConfigurationAddress,
    page_bytes: u32,
    operation: LensPageOperation,
    boundary: Option<PageBoundary>,
) -> MediumStatus {
    put_half(context, 0x1a, 0);
    loop {
        let page = half(context, 0x1a);
        let address = if page != 0xffff {
            if page >= half(context, 0x18) { return MediumStatus(0); }
            ConfigurationAddress(base.0.wrapping_add(page_bytes.wrapping_mul(u32::from(page))))
        } else { base };
        software.clear_notifications();
        software.request_write(PageNumber(page), address, operation);
        if software.wait(LensCompletion::Write, context).await.0 != 0 { return MediumStatus(-255); }
        if half(context, 0x1a) != 0xff00 {
            if let Some(boundary) = boundary {
                if software.page_boundary(boundary).await.0 != 0 { return MediumStatus(-255); }
            }
        }
        if word(context, 0x14) != 0 { return MediumStatus(-255); }
    }
}

pub async fn read_auxiliary_pages(
    software: &mut impl LensPageSoftware,
    context: &mut [u8; 0xc0],
    transfer: &mut [u8; 0x800],
    plan: LensTransferPlan,
) -> MediumStatus {
    loop {
        software.request_read(PageNumber(half(context, 0x1a)), plan.operation);
        if software.wait(LensCompletion::Read, context).await.0 != 0 { return MediumStatus(-255); }
        let current = half(context, 0x1a);
        if current == 0xff00 { return MediumStatus(0); }
        if word(context, 0x14) != 0 { return MediumStatus(-255); }
        context[0x34..0x38].copy_from_slice(&0u32.to_le_bytes());
        let next = if i32::from(current) < i32::from(half(context, 0x18)) - 1 {
            current.wrapping_add(1)
        } else if current == 0xffff { 0xff00 } else { 0xffff };
        put_half(context, 0x1a, next);
        let received = half(context, 0x1c);
        if received < half(context, 0x18) {
            let bytes = usize::from(plan.page_bytes);
            software.copy_received(word(context, 0x20), &mut transfer[..bytes]);
            let received = half(context, 0x1c);
            let offset = u32::from(plan.page_bytes) * u32::from(received);
            if received >= half(context, 0x18) || bytes == 0 || offset > 0x400 { return MediumStatus(-255); }
            software.copy_to_configuration(ConfigurationAddress(0x3e5000 + offset), &transfer[..bytes]);
        }
    }
}

pub fn read_completion(context: &mut [u8; 0xc0], status: MediumStatus, page: PageNumber, buffer: u32) -> u32 {
    if status.0 == 0 {
        context[0x20..0x24].copy_from_slice(&buffer.to_le_bytes());
        put_half(context, 0x1c, page.0);
        context[0x14..0x18].copy_from_slice(&0u32.to_le_bytes());
    } else {
        put_half(context, 0x1a, page.0);
        context[0x14..0x18].copy_from_slice(&(-255i32).to_le_bytes());
    }
    if status.0 != 0 { 2 } else { u32::from(page.0 == 0xffff) }
}

pub fn write_completion(context: &mut [u8; 0xc0], status: MediumStatus, page: PageNumber) -> bool {
    context[0x14..0x18].copy_from_slice(&(if status.0 == 0 { 0i32 } else { -255 }).to_le_bytes());
    put_half(context, 0x1a, page.0);
    status.0 == 0 && page.0 == 0xff00
}
