#[path = "build_support/mod.rs"]
mod build_support;

fn main() -> std::io::Result<()> {
    // Subprocess tests must exercise this exact artifact, including under coverage.
    println!(
        "cargo:rustc-env=ROBOSTICKIES_BUILD_SCRIPT={}",
        std::env::current_exe().unwrap_or_default().display()
    );
    build_support::generate(
        std::path::Path::new(&std::env::var_os("CARGO_MANIFEST_DIR").unwrap()),
        std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()),
    )
}
