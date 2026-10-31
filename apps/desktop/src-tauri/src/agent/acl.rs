//! A file only the current Windows user can open: its access list is replaced by one entry
//! for the user, inherited entries left out (the data folder's grant administrators and
//! SYSTEM, and some setups more).
#![allow(unsafe_code)]

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::Security::Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, DACL_SECURITY_INFORMATION,
    GetLengthSid, GetTokenInformation, InitializeAcl, PROTECTED_DACL_SECURITY_INFORMATION,
    TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// The process token, closed when dropped.
struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `OpenProcessToken` and is closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn last_error(what: &str) -> String {
    format!("{what}: {}", std::io::Error::last_os_error())
}

/// The `TOKEN_USER` of the process, in a buffer aligned for it.
fn token_user() -> Result<Vec<u64>, String> {
    let mut handle: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo handle of the current process needs no closing; `handle` is written.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut handle) } == 0 {
        return Err(last_error("cannot open the process token"));
    }
    let token = Token(handle);
    let mut size = 0u32;
    // SAFETY: a null buffer of length 0 asks for the size only.
    unsafe {
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &raw mut size);
    }
    let mut buffer = vec![0u64; (size as usize).div_ceil(8).max(1)];
    let len = u32::try_from(buffer.len() * 8).map_err(|e| e.to_string())?;
    // SAFETY: `buffer` holds `len` bytes, aligned for `TOKEN_USER`.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            len,
            &raw mut size,
        )
    };
    if ok == 0 {
        return Err(last_error("cannot read the process user"));
    }
    Ok(buffer)
}

/// Gives the current user alone full access to `path`.
pub(super) fn restrict_to_user(path: &Path) -> Result<(), String> {
    let user = token_user()?;
    // SAFETY: `token_user` filled the buffer with a `TOKEN_USER`, whose SID points inside it.
    let sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    // SAFETY: `sid` is a valid SID for as long as `user` lives.
    let sid_len = unsafe { GetLengthSid(sid) } as usize;
    let size = std::mem::size_of::<ACL>() + std::mem::size_of::<ACCESS_ALLOWED_ACE>() + sid_len;
    let mut acl = vec![0u64; size.div_ceil(8)];
    let acl_len = u32::try_from(acl.len() * 8).map_err(|e| e.to_string())?;
    let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
    // SAFETY: `acl` holds `acl_len` bytes, aligned for an ACL, room for one ACE with `sid`.
    let built = unsafe {
        InitializeAcl(acl_ptr, acl_len, ACL_REVISION) != 0
            && AddAccessAllowedAce(acl_ptr, ACL_REVISION, FILE_ALL_ACCESS, sid) != 0
    };
    if !built {
        return Err(last_error("cannot build the access list"));
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is a NUL-terminated path; the ACL and SID outlive the call.
    let status = unsafe {
        SetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            acl_ptr,
            std::ptr::null(),
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        #[allow(clippy::cast_possible_wrap)]
        let error = std::io::Error::from_raw_os_error(status as i32);
        Err(format!("cannot restrict {}: {error}", path.display()))
    }
}
