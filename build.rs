#[cfg(feature = "nauty")]
#[path = "native/build.rs"]
mod nauty_build;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "nauty")]
    nauty_build::build(std::path::Path::new(env!("CARGO_MANIFEST_DIR")));
}
