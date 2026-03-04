//! C FFI bindings for the hash library.
//!
//! Used by Go services (M7 Feature Flags) via CGo bridge.
//! cbindgen generates experimentation_ffi.h from these extern functions.

use std::ffi::CStr;
use std::os::raw::c_char;

/// Compute bucket assignment via FFI.
///
/// # Safety
/// `user_id` and `salt` must be valid null-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn experimentation_bucket(
    user_id: *const c_char,
    salt: *const c_char,
    total_buckets: u32,
) -> u32 {
    let user_id = unsafe { CStr::from_ptr(user_id) }.to_str().unwrap_or("");
    let salt = unsafe { CStr::from_ptr(salt) }.to_str().unwrap_or("");

    experimentation_hash::bucket(user_id, salt, total_buckets)
}
