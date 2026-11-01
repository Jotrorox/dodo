#![no_main]

#[path = "image_common.rs"]
mod image_common;

libfuzzer_sys::fuzz_target!(|data: &[u8]| -> libfuzzer_sys::Corpus { image_common::run(9, data) });
