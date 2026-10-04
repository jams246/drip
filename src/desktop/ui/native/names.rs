use crate::native_abi::{StringHeader, quote_utf16, read_path, write_string};
use crate::watch_win32::{GetLastError, close, open_path};

type NameQuery = unsafe extern "system" fn(*const u16, *mut u16, u32) -> u32;
const INITIAL_NAME_UNITS: usize = 260;
const MAX_NAME_UNITS: usize = 32768;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLongPathNameW(path: *const u16, output: *mut u16, capacity: u32) -> u32;
    fn GetShortPathNameW(path: *const u16, output: *mut u16, capacity: u32) -> u32;
    fn GetFinalPathNameByHandleW(handle: isize, output: *mut u16, capacity: u32, flags: u32) -> u32;
}

fn read_name(path: &[u16], query: NameQuery) -> Result<Vec<u16>, u32> {
    read_output(|output, capacity| unsafe { query(path.as_ptr(), output, capacity) })
}

fn read_output(mut query: impl FnMut(*mut u16, u32) -> u32) -> Result<Vec<u16>, u32> {
    let mut output = vec![0; INITIAL_NAME_UNITS];
    loop {
        let length = query(output.as_mut_ptr(), output.len() as u32);
        if length == 0 {
            return Err(unsafe { GetLastError() });
        }
        if (length as usize) < output.len() {
            output.truncate(length as usize);
            return Ok(output);
        }
        if length as usize >= MAX_NAME_UNITS {
            return Err(206);
        }
        output.resize(length as usize + 1, 0);
    }
}

fn canonical_name(path: &[u16]) -> Result<Vec<u16>, u32> {
    let handle = open_path(path, 0, 0x0220_0000)?;
    let result = read_output(|output, capacity| unsafe {
        GetFinalPathNameByHandleW(handle, output, capacity, 0)
    });
    close(handle);
    result.map(|mut name| {
        if name.starts_with(&[92, 92, 63, 92]) {
            name.drain(..4);
        }
        name
    })
}

pub fn query_wide(path: &[u16]) -> String {
    let long = match canonical_name(path) {
        Ok(path) => path,
        Err(error) => return result(&[], &[], error),
    };
    match read_name(path, GetShortPathNameW) {
        Ok(short) => result(&long, &short, 0),
        Err(error) => result(&long, &[], error),
    }
}

fn result(long: &[u16], short: &[u16], error: u32) -> String {
    format!(
        "{{\"longPath\":{},\"shortPath\":{},\"error\":{error}}}",
        quote_utf16(long),
        quote_utf16(short)
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn js_drip_query_names(path: *const StringHeader) -> *mut StringHeader {
    let output = match unsafe { read_path(path) } {
        Ok(path) => query_wide(&path),
        Err(error) => result(&[], &[], error),
    };
    write_string(&output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch_win32::wide_path;
    use std::fs;

    #[test]
    fn real_short_names_expand_to_existing_long_paths_and_missing_reports_error() {
        let root = crate::watch_tests::fixture("short-names");
        let path = root.join("Long Name For Native Expansion.txt");
        fs::write(&path, "names").unwrap();
        let input = wide_path(path.to_str().unwrap());
        let short = read_name(&input, GetShortPathNameW).unwrap();
        assert!(String::from_utf16(&short).unwrap().contains('~'));
        let mut short_input = short.clone();
        short_input.push(0);
        let expanded = read_name(&short_input, GetLongPathNameW).unwrap();
        assert_eq!(expanded, input[..input.len() - 1]);
        let result = query_wide(&short_input);
        assert!(result.contains("Long Name For Native Expansion.txt"));
        assert!(result.contains("\"error\":0"));
        let normalized = path.to_str().unwrap().replace('\\', "/").to_lowercase();
        let normalized_names = query_wide(&wide_path(&normalized));
        assert!(
            normalized_names.contains("\"error\":0"),
            "{normalized_names}"
        );
        assert!(
            normalized_names
                .contains("Long Name For Native Expansion.txt")
        );
        fs::remove_file(&path).unwrap();
        assert!(query_wide(&short_input).contains("\"error\":2"));
        crate::watch_tests::remove_fixture(&root);
    }
}
