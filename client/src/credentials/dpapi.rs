#![expect(
    unsafe_code,
    reason = "Windows DPAPI calls require audited pointer boundaries"
)]

use std::{ffi::c_void, slice};

use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    },
    core::PCWSTR,
};

use super::CredentialError;

pub(super) fn protect(plaintext: &[u8]) -> Result<Vec<u8>, CredentialError> {
    let length = u32::try_from(plaintext.len()).map_err(|_| CredentialError::DpapiTooLarge)?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: length,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `input` points to the live plaintext slice and `output` is writable.
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|error| CredentialError::Dpapi {
        operation: "protect",
        code: error.code().0 as u32,
    })?;
    copy_and_free(output, "protect")
}

pub(super) fn unprotect(ciphertext: &[u8]) -> Result<Vec<u8>, CredentialError> {
    let length = u32::try_from(ciphertext.len()).map_err(|_| CredentialError::DpapiTooLarge)?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: length,
        pbData: ciphertext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `input` points to the live ciphertext slice and `output` is writable.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|_| CredentialError::Corrupt)?;
    copy_and_free(output, "unprotect")
}

fn copy_and_free(
    output: CRYPT_INTEGER_BLOB,
    operation: &'static str,
) -> Result<Vec<u8>, CredentialError> {
    if output.pbData.is_null() || output.cbData == 0 {
        if !output.pbData.is_null() {
            // SAFETY: DPAPI allocated this buffer and `copy_and_free` owns it.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(output.pbData as *mut c_void)));
            }
        }
        return Err(CredentialError::DpapiOutput(operation));
    }
    // SAFETY: DPAPI returned `cbData` bytes at `pbData`; the buffer remains live until freed below.
    let bytes = unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    // SAFETY: DPAPI allocated this buffer and `copy_and_free` releases it exactly once.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut c_void)));
    }
    Ok(bytes)
}
