// SPDX-License-Identifier: MIT OR Apache-2.0

//! Build script for `floresta-dossel`.
//!
//! Two jobs:
//!   1. Locate the system Guile 3.0 development headers and libraries via
//!      `pkg-config`, emitting the link directives Cargo needs.
//!   2. Compile the C shim (`csrc/dossel_shim.c`) and generate Rust bindings
//!      for it plus `libguile.h` using bindgen.
//!
//! Guile is a system C library, so it is not vendored. If it is missing we fail
//! with an actionable message rather than a link error a thousand lines later.

use std::env;
use std::path::PathBuf;

const GUILE_PKG: &str = "guile-3.0";

fn main() {
    println!("cargo:rerun-if-changed=csrc/dossel_shim.c");
    println!("cargo:rerun-if-changed=csrc/dossel_shim.h");
    println!("cargo:rerun-if-changed=csrc/wrapper.h");

    let guile = match pkg_config::Config::new()
        .atleast_version("3.0")
        .probe(GUILE_PKG)
    {
        Ok(lib) => lib,
        Err(e) => {
            panic!(
                "floresta-dossel requires the GNU Guile 3.0 development files, but \
                 pkg-config could not find `{GUILE_PKG}`.\n\n\
                 Install them with one of:\n  \
                   Debian/Ubuntu: apt install guile-3.0-dev\n  \
                   Fedora:        dnf install guile30-devel\n  \
                   macOS:         brew install guile\n  \
                   Nix:           nix-shell -p guile_3_0\n\n\
                 If Guile is installed somewhere unusual, point PKG_CONFIG_PATH at the \
                 directory containing {GUILE_PKG}.pc.\n\n\
                 Underlying pkg-config error: {e}"
            );
        }
    };

    // Compile the shim against the same include paths pkg-config reported.
    let mut build = cc::Build::new();
    build.file("csrc/dossel_shim.c").include("csrc");
    for path in &guile.include_paths {
        build.include(path);
    }
    build.compile("dossel_shim");

    let mut bindings = bindgen::Builder::default()
        .header("csrc/wrapper.h")
        .clang_arg("-Icsrc")
        // Guile's public surface is entirely `scm_*` / `SCM_*`; the shim adds
        // `dossel_*`. Everything else in the transitive include graph (libc,
        // gmp, pthreads) is noise we do not want bound.
        .allowlist_function("scm_.*")
        .allowlist_function("dossel_.*")
        .allowlist_type("SCM.*")
        .allowlist_type("scm_.*")
        // `SCM` is an opaque pointer. Blocking layout tests keeps the generated
        // file small and avoids alignment assertions that vary by platform.
        .layout_tests(false)
        .generate_comments(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

    for path in &guile.include_paths {
        bindings = bindings.clang_arg(format!("-I{}", path.display()));
    }

    let bindings = bindings
        .generate()
        .expect("failed to generate bindings for libguile");

    let out_path = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is always set by Cargo"));
    bindings
        .write_to_file(out_path.join("guile_bindings.rs"))
        .expect("failed to write generated Guile bindings");
}
