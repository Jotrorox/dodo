//! Hardware abstraction layer: host execution with fakes, interrupt-control
//! lowering on bare-metal targets, and rejection where no mask exists.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn dodo(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dodo"))
        .args(args)
        .arg(path)
        .output()
        .unwrap()
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("dodo-hal-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn registers_protocols_and_fakes_execute_on_the_host() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("checks");
    let source = root.join("tests/stdlib/hal_checks.dodo");
    for optimization in ["0", "3"] {
        let executable = scratch.0.join(format!(
            "hal-O{optimization}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let mut args = vec!["build", source.to_str().unwrap(), "-O", optimization];
        args.push("-o");
        success(&dodo(&args, &executable), "compile HAL fixture");
        success(
            &Command::new(&executable).output().unwrap(),
            "execute HAL fixture",
        );
    }
}

#[test]
fn interrupt_control_lowers_to_each_architecture_mask() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scratch = Scratch::new("lowering");
    let source = root.join("tests/stdlib/hal_checks.dodo");
    for (target, expected) in [
        ("thumbv6m-none-eabi", &["mrs", "cpsid", "msr"][..]),
        ("thumbv7em-none-eabihf", &["mrs", "cpsid", "msr"][..]),
        ("riscv32-unknown-none-elf", &["csrrci", "csrs"][..]),
        ("riscv64-unknown-none-elf", &["csrrci", "csrs"][..]),
    ] {
        let assembly = scratch.0.join(format!("{target}.s"));
        success(
            &dodo(
                &[
                    "build",
                    source.to_str().unwrap(),
                    "--emit",
                    "asm",
                    "--target",
                    target,
                    "-O",
                    "3",
                    "-o",
                ],
                &assembly,
            ),
            &format!("emit HAL assembly for {target}"),
        );
        let text = fs::read_to_string(&assembly).unwrap().to_lowercase();
        for instruction in expected {
            assert!(
                text.contains(instruction),
                "{target} lacks {instruction}:\n{text}"
            );
        }
    }

    let cpu = scratch.write(
        "cpu.dodo",
        "package cpu_ops\nimport \"core/cpu\"\npub fn idle() {\n cpu.fence()\n cpu.wait_for_interrupt()\n}\n",
    );
    let assembly = scratch.0.join("idle.s");
    success(
        &dodo(
            &[
                "build",
                cpu.to_str().unwrap(),
                "--emit",
                "asm",
                "--target",
                "thumbv6m-none-eabi",
                "-o",
            ],
            &assembly,
        ),
        "emit wait-for-interrupt",
    );
    let text = fs::read_to_string(&assembly).unwrap();
    assert!(text.contains("wfi") && text.contains("dmb"), "{text}");
}

#[test]
fn freestanding_targets_without_an_interrupt_model_are_rejected() {
    let scratch = Scratch::new("rejected");
    let critical = scratch.write(
        "critical.dodo",
        "package critical\nimport \"std/embedded/hal\"\npub fn section() {\n guard := hal.Critical.enter()\n core.drop(guard)\n}\n",
    );
    let output = dodo(
        &[
            "build",
            critical.to_str().unwrap(),
            "--emit",
            "obj",
            "--target",
            "wasm32-unknown-unknown",
            "-o",
        ],
        &scratch.0.join("critical.o"),
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not supported for target"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The rest of the HAL stays portable to the same target.
    let registers = scratch.write(
        "registers.dodo",
        "package registers\nimport \"std/embedded/hal\"\npub fn enable(address: usize) {\n r: hal.Reg<u32> = unsafe { hal.Reg.at(address) }\n r.set(0x70, 3)\n}\n",
    );
    success(
        &dodo(
            &[
                "build",
                registers.to_str().unwrap(),
                "--emit",
                "obj",
                "--target",
                "wasm32-unknown-unknown",
                "-o",
            ],
            &scratch.0.join("registers.o"),
        ),
        "portable registers on wasm",
    );
}

#[test]
fn field_values_that_do_not_fit_trap() {
    let scratch = Scratch::new("trap");
    let source = scratch.write(
        "main.dodo",
        "package main\nimport \"std/embedded/hal\"\nfn main() {\n value := hal.encode(0x70u32, 8)\n core.assert_eq(value, 0u32)\n}\n",
    );
    let executable = scratch
        .0
        .join(format!("trap{}", std::env::consts::EXE_SUFFIX));
    success(
        &dodo(&["build", source.to_str().unwrap(), "-o"], &executable),
        "compile trapping fixture",
    );
    let output = Command::new(&executable).output().unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("assert"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
