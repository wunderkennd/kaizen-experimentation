//! C FFI bindings for the hash library.
//!
//! Used by Go services (M7 Feature Flags) via CGo bridge.
//! cbindgen generates experimentation_ffi.h from these extern functions.

use std::ffi::CStr;
use std::os::raw::c_char;

/// Sentinel value returned when an FFI call receives invalid input (null pointers,
/// non-UTF-8 strings, or zero `total_buckets`).
pub const EXPERIMENTATION_BUCKET_ERROR: u32 = u32::MAX;

/// Compute bucket assignment via FFI.
///
/// Returns `EXPERIMENTATION_BUCKET_ERROR` (`u32::MAX`) if:
/// - `user_id` or `salt` is null
/// - either string is not valid UTF-8
/// - `total_buckets` is 0
///
/// # Safety
/// `user_id` and `salt` must be valid null-terminated C strings (or null).
#[no_mangle]
pub unsafe extern "C" fn experimentation_bucket(
    user_id: *const c_char,
    salt: *const c_char,
    total_buckets: u32,
) -> u32 {
    if user_id.is_null() || salt.is_null() || total_buckets == 0 {
        return EXPERIMENTATION_BUCKET_ERROR;
    }

    let user_id = match unsafe { CStr::from_ptr(user_id) }.to_str() {
        Ok(s) => s,
        Err(_) => return EXPERIMENTATION_BUCKET_ERROR,
    };
    let salt = match unsafe { CStr::from_ptr(salt) }.to_str() {
        Ok(s) => s,
        Err(_) => return EXPERIMENTATION_BUCKET_ERROR,
    };

    experimentation_hash::bucket(user_id, salt, total_buckets)
}

/// Check if a bucket falls within an allocation range (inclusive).
///
/// Returns 1 if `bucket >= start_bucket && bucket <= end_bucket`, 0 otherwise.
/// Uses `u8` instead of `bool` for C ABI portability.
#[no_mangle]
pub extern "C" fn experimentation_is_in_allocation(
    bucket: u32,
    start_bucket: u32,
    end_bucket: u32,
) -> u8 {
    u8::from(experimentation_hash::is_in_allocation(bucket, start_bucket, end_bucket))
}
