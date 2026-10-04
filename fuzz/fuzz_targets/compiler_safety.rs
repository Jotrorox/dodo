#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../tests/support/compiler_safety.rs"]
mod safety;

fuzz_target!(|data: &[u8]| {
    // Every byte input becomes a small, well-typed program in the flow subset.
    // Crashes and oracle mismatches both leave a replayable libFuzzer artifact.
    for pointer_bits in [32, 64] {
        safety::verify(data, pointer_bits);
    }
});
