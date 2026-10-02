//! `ebk epub` as a function of a shared library. On Android an application may load a library from its own
//! directory but not start a program from there, so the KOReader plug-in uses this instead of the program.

use std::ffi::{c_char, c_int, CStr};
use std::path::PathBuf;

/// Writes the members of the EBK file `ebk` as the EPUB file `epub`, which must not exist; with `store` not
/// zero, without compression. Gives 0, or 1 with the reason in `message` (UTF-8, ends with a zero byte, cut to
/// `message_len` bytes).
///
/// # Safety
/// `ebk` and `epub` are strings that end with a zero byte; `message` has room for `message_len` bytes, or
/// `message_len` is 0.
#[no_mangle]
pub unsafe extern "C" fn ebk_to_epub(ebk: *const c_char, epub: *const c_char, store: c_int, message: *mut c_char, message_len: usize) -> c_int {
    let (ebk, epub) = (CStr::from_ptr(ebk).to_bytes().to_vec(), CStr::from_ptr(epub).to_bytes().to_vec());
    // a panic must not cross into the program that called (the JPEG codec's are caught further in already)
    let result = std::panic::catch_unwind(|| {
        let (Some(ebk), Some(epub)) = (path(&ebk), path(&epub)) else { return Err("the file names are not UTF-8".to_owned()) };
        ebk_cli::to_epub(&ebk, &epub, store != 0, ebk_cli::DEFAULT_MAX_OUTPUT, ebk_core::MAX_MEMBER_LEN).map_err(|e| format!("{e:#}"))
    });
    let error = match result {
        Ok(Ok(_)) => return 0,
        Ok(Err(e)) => e,
        Err(_) => "the EBK reader failed (a bug)".to_owned(),
    };
    if message_len > 0 {
        let mut len = error.len().min(message_len - 1);
        while !error.is_char_boundary(len) {
            len -= 1;
        }
        std::ptr::copy_nonoverlapping(error.as_ptr(), message.cast::<u8>(), len);
        *message.add(len) = 0;
    }
    1
}

/// A file name as the system has it: any bytes on Unix, UTF-8 elsewhere.
fn path(bytes: &[u8]) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(bytes).ok().map(PathBuf::from)
    }
}
