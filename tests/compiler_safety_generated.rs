#[path = "support/compiler_safety.rs"]
mod safety;

#[test]
fn generated_small_programs_have_independent_safety_outcomes() {
    let mut accepted = 0;
    let mut rejected = 0;
    for seed in 0..1024 {
        let bytes = safety::seeded_bytes(seed);
        for pointer_bits in [32, 64] {
            // Print the seed before unwinding so failures are directly replayable.
            let result = std::panic::catch_unwind(|| safety::verify(&bytes, pointer_bits))
                .unwrap_or_else(|error| {
                    eprintln!("compiler safety seed={seed}, pointer_bits={pointer_bits}");
                    std::panic::resume_unwind(error)
                });
            if result {
                accepted += 1;
            } else {
                rejected += 1;
            }
        }
    }
    assert!(accepted > 100, "safe cases missing: {accepted}");
    assert!(rejected > 100, "unsafe cases missing: {rejected}");
}

#[test]
fn byte_mutations_exercise_small_and_extreme_fuzz_inputs() {
    for pointer_bits in [32, 64] {
        safety::verify(&[], pointer_bits);
        for byte in 0..=255 {
            // Short/truncated inputs and repeated/extreme decoder choices.
            safety::verify(&[byte], pointer_bits);
            safety::verify(&[byte; 128], pointer_bits);
        }
        let mut input = safety::seeded_bytes(0xd0d0);
        for offset in 0..32 {
            let original = input[offset];
            for bit in 0..8 {
                input[offset] = original ^ (1 << bit);
                safety::verify(&input, pointer_bits);
            }
            input[offset] = original;
        }
    }
}

#[test]
fn libfuzzer_seed_corpus_has_specified_outcomes() {
    let corpus: &[(&[u8], bool)] = &[
        (
            include_bytes!("../fuzz/corpus/compiler_safety/initialized"),
            true,
        ),
        (
            include_bytes!("../fuzz/corpus/compiler_safety/uninitialized"),
            false,
        ),
        (include_bytes!("../fuzz/corpus/compiler_safety/move"), false),
        (
            include_bytes!("../fuzz/corpus/compiler_safety/reinitialize"),
            true,
        ),
        (
            include_bytes!("../fuzz/corpus/compiler_safety/branch-initialization"),
            true,
        ),
        (
            include_bytes!("../fuzz/corpus/compiler_safety/shadow"),
            false,
        ),
        (
            include_bytes!("../fuzz/corpus/compiler_safety/loop-back-edge"),
            false,
        ),
        (
            include_bytes!("../fuzz/corpus/compiler_safety/loop-reinitialize"),
            true,
        ),
        (
            include_bytes!("../fuzz/corpus/compiler_safety/nested-loop-break"),
            false,
        ),
        (
            include_bytes!("../fuzz/corpus/compiler_safety/return-edge"),
            true,
        ),
        (
            include_bytes!("../fuzz/corpus/compiler_safety/range-reinitialize"),
            true,
        ),
    ];
    for (bytes, accepted) in corpus {
        for pointer_bits in [32, 64] {
            assert_eq!(safety::verify(bytes, pointer_bits), *accepted, "{bytes:?}");
        }
    }
}

#[test]
#[ignore = "extended fuzz campaign / seed or libFuzzer artifact replay"]
fn replay_or_fuzz_generated_safety() {
    if let Some(path) = std::env::var_os("DODO_SAFETY_INPUT") {
        let input = std::fs::read(path).expect("read fuzz artifact");
        for pointer_bits in [32, 64] {
            safety::verify(&input, pointer_bits);
        }
        return;
    }
    let number = |name: &str, default| {
        std::env::var(name)
            .map(|value| value.parse::<u64>().expect("decimal u64"))
            .unwrap_or(default)
    };
    let start = number("DODO_SAFETY_SEED", 0);
    let count = number("DODO_SAFETY_CASES", 10_000);
    for seed in start..start.checked_add(count).expect("seed range overflow") {
        let input = safety::seeded_bytes(seed);
        for pointer_bits in [32, 64] {
            let result = std::panic::catch_unwind(|| safety::verify(&input, pointer_bits));
            if let Err(error) = result {
                eprintln!("compiler safety seed={seed}, pointer_bits={pointer_bits}");
                std::panic::resume_unwind(error);
            }
        }
    }
}
