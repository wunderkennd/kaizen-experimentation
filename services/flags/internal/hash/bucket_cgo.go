//go:build cgo && has_ffi

package hash

/*
#cgo CFLAGS: -I${SRCDIR}/../../../../crates/experimentation-ffi/target
#cgo LDFLAGS: -L${SRCDIR}/../../../../target/release -L${SRCDIR}/../../../../target/debug -lexperimentation_ffi
#include "experimentation_ffi.h"
#include <stdlib.h>
*/
import "C"
import (
	"fmt"
	"unsafe"
)

// Bucket computes a deterministic bucket assignment via the Rust FFI library.
// This implementation calls experimentation_bucket() from the Rust FFI crate,
// ensuring identical hash output to the Rust native implementation.
func Bucket(userID, salt string, totalBuckets uint32) uint32 {
	if totalBuckets == 0 {
		panic("totalBuckets must be > 0")
	}

	cUserID := C.CString(userID)
	cSalt := C.CString(salt)
	defer C.free(unsafe.Pointer(cUserID))
	defer C.free(unsafe.Pointer(cSalt))

	result := C.experimentation_bucket(cUserID, cSalt, C.uint32_t(totalBuckets))
	if uint32(result) == C.EXPERIMENTATION_BUCKET_ERROR {
		panic(fmt.Sprintf("experimentation_bucket returned error for user_id=%q salt=%q total_buckets=%d", userID, salt, totalBuckets))
	}

	return uint32(result)
}
