// Version: 1.0.0 · updated 26-09-06-20-15
//
// Put the icon inside the Windows executable.
//
// WHY THERE IS A BUILD SCRIPT AT ALL. Without one, Windows draws the generic
// blank-document placeholder for hub.exe: on the desktop, in the taskbar, in
// the Start menu. Somebody told "double-click the Gorilla icon" is then looking
// for a blank page among a dozen other blank pages, and the program looks like
// something that arrived from nowhere rather than something to trust with a
// child's work.
//
// NO CRATES. The obvious answer is the `winres` crate and it is not available
// here: this project has no dependencies, deliberately, and a build-time
// dependency is still a dependency to audit and to fetch on a machine with a
// bad connection. windres ships with the GNU toolchain that already compiles
// this on Windows, so the resource is compiled with that and linked directly.
//
// A missing windres, or a missing icon, is a warning and not an error. The
// program works perfectly without an icon, and a build that fails on a machine
// where the icon cannot be made is worse than one that produces a plain
// executable.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../packaging/icon/hub.ico");

    // Only for Windows OUTPUT, not on a Windows host. Cross-compiling to Linux
    // from here must not try to attach a Windows resource.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ico = manifest.join("../../packaging/icon/hub.ico");
    if !ico.is_file() {
        println!("cargo:warning=no icon at {}, building without one", ico.display());
        return;
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc = out.join("hub.rc");
    let res = out.join("hub-res.o");

    // 1 is the first icon, and the shell shows the lowest-numbered one as the
    // program's own icon. The path is written with forward slashes because
    // windres reads a backslash as an escape.
    let rc_text = format!("1 ICON \"{}\"\n", ico.to_string_lossy().replace('\\', "/"));
    if std::fs::write(&rc, rc_text).is_err() {
        println!("cargo:warning=could not write the resource script, building without an icon");
        return;
    }

    // windres, or the target-prefixed one a cross toolchain installs.
    let candidates = ["windres", "x86_64-w64-mingw32-windres"];
    let mut done = false;
    for tool in candidates {
        match Command::new(tool).arg(&rc).arg("-O").arg("coff").arg("-o").arg(&res).status() {
            Ok(s) if s.success() => {
                done = true;
                break;
            }
            _ => continue,
        }
    }

    if !done {
        println!("cargo:warning=windres did not run, building without an icon");
        return;
    }

    // Hand the compiled resource to the linker. Passed as a plain argument
    // rather than a library because it is a single object file.
    println!("cargo:rustc-link-arg-bins={}", res.display());
}
