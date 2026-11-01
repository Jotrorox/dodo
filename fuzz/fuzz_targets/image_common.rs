//! Shared glue of the image_* fuzz targets: each passes its input to one
//! target of fuzz/image/targets (numbered as TARGET_* in common.dodo),
//! compiled from Dodo by build.rs. A Dodo trap aborts the process, which
//! libFuzzer reports as a crash; replay it with the Dodo harness too
//! (`image-harness <target> <artifact>`, see fuzz/README.md).

unsafe extern "C" {
    fn dodo_image_fuzz(target: u32, data: *const u8, length: usize) -> i32;
    fn dodo_image_target_count() -> u32;
}

/// Number of targets this file was written for.
const TARGET_COUNT: u32 = 13;

pub fn run(target: u32, data: &[u8]) -> libfuzzer_sys::Corpus {
    // SAFETY: the Dodo entry points take no pointers but `data`, which is
    // valid for `data.len()` bytes; they keep no reference to it.
    unsafe {
        assert_eq!(
            dodo_image_target_count(),
            TARGET_COUNT,
            "fuzz/image targets renumbered"
        );
        if dodo_image_fuzz(target, data.as_ptr(), data.len()) == 0 {
            libfuzzer_sys::Corpus::Keep
        } else {
            libfuzzer_sys::Corpus::Reject
        }
    }
}
