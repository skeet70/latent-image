use crate::contract_violation;

#[inline(never)]
pub(crate) fn read_byte(source: &[u8], index: usize) -> u8 {
    *source.get(index).unwrap_or_else(|| contract_violation())
}

#[inline(never)]
pub(crate) fn read_halfword(source: &[u8], index: usize) -> u16 {
    let bytes: &[u8; 2] = source.get(index..index + 2).and_then(|bytes| bytes.try_into().ok()).unwrap_or_else(|| contract_violation());
    if bytes.as_ptr() as usize & 1 == 0 {
        u16::from_le(unsafe { bytes.as_ptr().cast::<u16>().read() })
    } else {
        u16::from_le_bytes([bytes[0], bytes[1]])
    }
}

#[inline(never)]
pub(crate) fn write_byte_at(destination: &mut [u8], index: usize, value: u8) {
    write_byte(destination.get_mut(index).unwrap_or_else(|| contract_violation()), value);
}

#[inline(always)]
pub(crate) fn write_byte(destination: &mut u8, value: u8) {
    #[cfg(settings_storage_firmware)]
    unsafe {
        extern "C" { fn xm1_storage_store8(address: *mut u8, value: u32); }
        xm1_storage_store8(destination, u32::from(value));
    }
    #[cfg(not(settings_storage_firmware))]
    { *destination = value; }
}

pub(crate) fn copy_bytes(destination: &mut [u8], source: &[u8]) {
    if destination.len() != source.len() { contract_violation(); }
    let mut offset = 0;
    if (destination.as_ptr() as usize | source.as_ptr() as usize) & 1 == 0 {
        let paired = source.len() & !1;
        while offset < paired {
            let bytes = source.get(offset..offset+2).unwrap_or_else(|| contract_violation());
            let value = u16::from_le(unsafe { bytes.as_ptr().cast::<u16>().read() });
            write_halfword(destination, offset, value);
            offset += 2;
        }
    }
    for (index, value) in source.iter().enumerate().skip(offset) {
        write_byte(destination.get_mut(index).unwrap_or_else(|| contract_violation()), *value);
    }
}

#[inline(always)]
pub(crate) fn write_output_byte(destination: &mut [u8], index: usize, value: u8) {
    if index >= destination.len() { contract_violation(); }
    #[cfg(settings_storage_firmware)]
    unsafe {
        extern "C" { fn xm1_storage_output8(buffer: *mut u8, length: usize, index: usize, value: u32); }
        xm1_storage_output8(destination.as_mut_ptr(), destination.len(), index, u32::from(value));
    }
    #[cfg(not(settings_storage_firmware))]
    { destination[index] = value; }
}

#[inline(always)]
pub(crate) fn write_output_halfword(destination: &mut [u8], index: usize, value: u16) {
    let end = index.checked_add(2).unwrap_or_else(|| contract_violation());
    if end > destination.len() { contract_violation(); }
    #[cfg(settings_storage_firmware)]
    unsafe {
        extern "C" { fn xm1_storage_output16(buffer: *mut u8, length: usize, index: usize, value: u32); }
        xm1_storage_output16(destination.as_mut_ptr(), destination.len(), index, u32::from(value));
    }
    #[cfg(not(settings_storage_firmware))]
    { destination.get_mut(index..end).unwrap().copy_from_slice(&value.to_le_bytes()); }
}

#[inline(always)]
pub(crate) fn write_halfword(destination: &mut [u8], offset: usize, value: u16) {
    let destination = destination.get_mut(offset..offset+2).unwrap_or_else(|| contract_violation());
    #[cfg(settings_storage_firmware)]
    unsafe {
        extern "C" { fn xm1_storage_store16(address: *mut u8, value: u32); }
        xm1_storage_store16(destination.as_mut_ptr(), u32::from(value));
    }
    #[cfg(not(settings_storage_firmware))]
    { destination.copy_from_slice(&value.to_le_bytes()); }
}
