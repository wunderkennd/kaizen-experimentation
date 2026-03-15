/**
 * Production hash provider that delegates to the generated UniFFI bindings.
 * Requires the UniFFI-generated native library to be loaded.
 *
 * This file lives in src/uniffi/kotlin/ which is only added to the main source
 * set when the UniFFI-generated bindings are present. In CI (where the native
 * library is not built), this directory does not exist and the file is excluded.
 */
package com.experimentation.sdk

import uniffi.experimentation_hash.uniffiBucket
import uniffi.experimentation_hash.uniffiIsInAllocation

class UniFFIHashProvider : HashProvider {
    override fun bucket(userId: String, salt: String, totalBuckets: Int): Int =
        uniffiBucket(userId, salt, totalBuckets.toUInt()).toInt()

    override fun isInAllocation(bucket: Int, startBucket: Int, endBucket: Int): Boolean =
        uniffiIsInAllocation(bucket.toUInt(), startBucket.toUInt(), endBucket.toUInt())
}
