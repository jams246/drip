use std::slice;

// Pinned Perry ABI 0.5, StringHeader revision 1. The payload follows this header.
#[repr(C)]
pub struct StringHeader {
    utf16_len: u32,
    byte_len: u32,
    capacity: u32,
    refcount: u32,
    flags: u32,
}

#[cfg(not(test))]
unsafe extern "C" {
    fn js_string_from_bytes(data: *const u8, length: u32) -> *mut StringHeader;
}

#[cfg(test)]
pub unsafe fn read_string(value: *const StringHeader) -> String {
    if value.is_null() {
        return String::new();
    }
    let bytes =
        unsafe { slice::from_raw_parts(value.add(1).cast::<u8>(), (*value).byte_len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(not(test))]
pub fn write_string(value: &str) -> *mut StringHeader {
    unsafe { js_string_from_bytes(value.as_ptr(), value.len() as u32) }
}

pub unsafe fn read_path(value: *const StringHeader) -> Result<Vec<u16>, u32> {
    if value.is_null() {
        return Err(123);
    }
    let bytes =
        unsafe { slice::from_raw_parts(value.add(1).cast::<u8>(), (*value).byte_len as usize) };
    let mut path = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let first = bytes[index];
        let (count, mut scalar) = match first {
            1..=0x7f => (1, first as u32),
            0xc2..=0xdf => (2, (first & 0x1f) as u32),
            0xe0..=0xef => (3, (first & 0x0f) as u32),
            0xf0..=0xf4 => (4, (first & 0x07) as u32),
            _ => return Err(123),
        };
        let Some(encoded) = bytes.get(index..index + count) else {
            return Err(123);
        };
        for &byte in &encoded[1..] {
            if byte & 0xc0 != 0x80 {
                return Err(123);
            }
            scalar = scalar << 6 | (byte & 0x3f) as u32;
        }
        let minimum = [0, 0, 0x80, 0x800, 0x10000][count];
        if scalar < minimum || scalar > 0x10ffff {
            return Err(123);
        }
        if scalar <= 0xffff {
            path.push(scalar as u16);
        } else {
            let pair = scalar - 0x10000;
            path.push(0xd800 | (pair >> 10) as u16);
            path.push(0xdc00 | (pair & 0x3ff) as u16);
        }
        index += count;
    }
    path.push(0);
    Ok(path)
}

#[cfg(test)]
pub fn write_string(value: &str) -> *mut StringHeader {
    use std::alloc::{Layout, alloc};
    let layout = Layout::from_size_align(20 + value.len(), 4).unwrap();
    let pointer = unsafe { alloc(layout) }.cast::<StringHeader>();
    unsafe {
        pointer.write(StringHeader {
            utf16_len: value.encode_utf16().count() as u32,
            byte_len: value.len() as u32,
            capacity: value.len() as u32,
            refcount: 0,
            flags: 0,
        });
        std::ptr::copy_nonoverlapping(value.as_ptr(), pointer.add(1).cast(), value.len());
    }
    pointer
}

pub fn quote_utf16(value: &[u16]) -> String {
    use std::fmt::Write;
    let mut output = String::from("\"");
    for &unit in value {
        match unit {
            0x22 => output.push_str("\\\""),
            0x5c => output.push_str("\\\\"),
            0x20..=0x7e => output.push(char::from_u32(unit as u32).unwrap()),
            _ => write!(&mut output, "\\u{unit:04x}").unwrap(),
        }
    }
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_abi_preserves_unicode_and_wtf8_surrogates() {
        let string = write_string("file\u{1f600}");
        assert_eq!(unsafe { read_string(string) }, "file\u{1f600}");
        assert_eq!(
            unsafe { read_path(string) }.unwrap(),
            "file\u{1f600}"
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>()
        );
        let surrogate = write_string("\u{e000}");
        unsafe {
            std::ptr::copy_nonoverlapping([0xedu8, 0xa0, 0x80].as_ptr(), surrogate.add(1).cast(), 3)
        };
        assert_eq!(unsafe { read_path(surrogate) }.unwrap(), vec![0xd800, 0]);
        assert_eq!(
            unsafe { read_path(write_string("invalid\0path")) },
            Err(123)
        );
    }
}
