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
    //
    // VERSIONINFO goes in the same script, and is why a teacher can right-click
    // hub.exe, open Details, and see the version. Until now that pane was
    // empty: the menu screen has named the version since 0.9.3, but somebody
    // holding a copy that came off a memory stick is not in the menu, they are
    // in Explorer, and an executable with no publisher and no version is the
    // exact shape of the thing an IT department tells people to delete.
    //
    // FILEVERSION takes four numbers where the crate version has three, so the
    // fourth is 0. They are parsed from CARGO_PKG_VERSION rather than typed,
    // because a version that has to be updated in two places is a version that
    // will disagree with itself; this project already has a test that fails on
    // exactly that drift in the packaging files.
    //
    // 0809 is British English and 04b0 is codepage 1200, Unicode. The pair has
    // to appear twice and agree, once as the block name and once in the
    // Translation value, or Explorer quietly shows nothing.
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut parts: Vec<String> = version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string())
        .collect();
    parts.resize(4, "0".to_string());
    let quad = parts[..4].join(",");

    let icopath = ico.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
    let rc_text = format!(
        "1 ICON \"{icopath}\"

1 VERSIONINFO
FILEVERSION {quad}
PRODUCTVERSION {quad}
FILEOS 0x4L
FILETYPE 0x1L
BEGIN
  BLOCK \"StringFileInfo\"
  BEGIN
    BLOCK \"080904b0\"
    BEGIN
      VALUE \"CompanyName\", \"Gorilla\"
      VALUE \"FileDescription\", \"Gorilla Portable Network Hub\"
      VALUE \"FileVersion\", \"{version}\"
      VALUE \"InternalName\", \"hub\"
      VALUE \"LegalCopyright\", \"Free software under the GNU AGPL v3\"
      VALUE \"OriginalFilename\", \"hub.exe\"
      VALUE \"ProductName\", \"Gorilla Portable Network Hub\"
      VALUE \"ProductVersion\", \"{version}\"
    END
  END
  BLOCK \"VarFileInfo\"
  BEGIN
    VALUE \"Translation\", 0x809, 1200
  END
END
"
    );
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
