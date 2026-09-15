//! Adapt the official Windows LLVM archive's build-machine library paths for
//! llvm-sys, which expects bare MSVC library names from --system-libs.
use std::env;
use std::io::{self, Write};
use std::path::Path;
use std::process::{self, Command};

fn system_libraries(output: &str, libdir: &Path) -> io::Result<String> {
    let mut libraries = Vec::new();
    for library in output.split_whitespace() {
        let name = library.rsplit(['/', '\\']).next().unwrap();
        if !name.ends_with(".lib") || name.contains(':') {
            return Err(io::Error::other(format!(
                "unexpected LLVM system library: {library}"
            )));
        }
        if name != library && !libdir.join(name).is_file() {
            return Err(io::Error::other(format!(
                "LLVM system library {library} is missing from {}",
                libdir.display()
            )));
        }
        libraries.push(name);
    }
    Ok(format!("{}\n", libraries.join(" ")))
}

fn main() -> io::Result<()> {
    let executable = env::current_exe()?;
    let bin = executable.parent().unwrap();
    let args: Vec<_> = env::args_os().skip(1).collect();
    let output = Command::new(bin.join("llvm-config.exe"))
        .args(&args)
        .output()?;
    io::stderr().write_all(&output.stderr)?;
    if !output.status.success() {
        io::stdout().write_all(&output.stdout)?;
        process::exit(output.status.code().unwrap_or(1));
    }
    if args.iter().any(|arg| arg == "--system-libs") {
        let text = std::str::from_utf8(&output.stdout).map_err(io::Error::other)?;
        let libraries = system_libraries(text, &bin.parent().unwrap().join("lib"))?;
        io::stdout().write_all(libraries.as_bytes())?;
    } else {
        io::stdout().write_all(&output.stdout)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn preserves_windows_sdk_libraries_and_accepts_empty_output() {
        let libdir = Path::new("unused");
        assert_eq!(
            system_libraries("psapi.lib shell32.lib xml2s.lib\r\n", libdir).unwrap(),
            "psapi.lib shell32.lib xml2s.lib\n"
        );
        assert_eq!(system_libraries("\r\n", libdir).unwrap(), "\n");
    }

    #[test]
    fn relocates_build_machine_paths_only_when_the_library_is_installed() {
        let libdir = env::temp_dir().join(format!("dodo-llvm-config-{}", process::id()));
        fs::create_dir_all(&libdir).unwrap();
        fs::write(libdir.join("zstd_static.lib"), b"test archive").unwrap();
        for library in [
            "S:/llvm/utils/release/llvm_package_23.1.1/build_amd64_stage0/zstdbuild/install/lib/zstd_static.lib",
            r"S:\llvm-build\zstd\lib\zstd_static.lib",
        ] {
            let output = format!("psapi.lib zs.lib {library} xml2s.lib\r\n");
            assert_eq!(
                system_libraries(&output, &libdir).unwrap(),
                "psapi.lib zs.lib zstd_static.lib xml2s.lib\n"
            );
        }
        fs::remove_dir_all(&libdir).unwrap();
        assert!(system_libraries("S:/missing/zstd_static.lib", &libdir).is_err());
        assert!(system_libraries("S:xml2s.lib", &libdir).is_err());
        assert!(system_libraries("-lxml2", &libdir).is_err());
    }
}
