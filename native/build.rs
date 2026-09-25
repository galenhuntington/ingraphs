use std::{env, fs, path::Path, process::Command};

fn run(command: &mut Command) {
    let output = command
        .output()
        .unwrap_or_else(|e| panic!("{command:?}: {e}"));
    assert!(
        output.status.success(),
        "{command:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

pub fn build(root: &Path) {
    assert_eq!(
        env::var("HOST"),
        env::var("TARGET"),
        "The bundled nauty build currently supports native builds only; \
         use --no-default-features for the pure-Rust backend when cross-compiling"
    );
    assert_eq!(
        env::var("CARGO_CFG_TARGET_FAMILY").as_deref(),
        Ok("unix"),
        "The bundled nauty configuration requires POSIX sh; \
         use --no-default-features for the pure-Rust backend on other platforms"
    );
    let source = root.join("vendor/nauty-2.9.3");
    let adapter = root.join("native/nauty_shim.c");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed={}", adapter.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("native/build.rs").display()
    );

    let out = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("nauty");
    fs::create_dir_all(&out).unwrap();
    let mut build = cc::Build::new();
    // Configure and compile in the SAME language mode: in particular, the
    // detected TLS spelling must match. C11 also avoids old C23 noreturn issues.
    build.std("c11").warnings(false);
    if env::var_os("CARGO_FEATURE_NATIVE").is_some() {
        // An explicitly requested optimization must not silently be ignored.
        build.flag("-march=native");
    }
    let compiler = build.get_compiler();
    let cc = if compiler.cc_env().is_empty() {
        compiler.path().as_os_str().to_owned()
    } else {
        compiler.cc_env()
    };
    // Configure only generates headers. Cargo/cc builds the archive; no make,
    // preconfigured local headers, bindgen, libclang, or build-time downloads.
    run(Command::new("sh")
        .arg(source.join("configure"))
        .args(["--quiet", "--no-create", "--enable-tls", "--enable-generic"])
        .env("CC", cc)
        .env("CFLAGS", compiler.cflags_env())
        .envs(compiler.env().iter().cloned())
        .current_dir(&out));
    run(Command::new("sh")
        .args(["config.status", "nauty.h", "naututil.h"])
        .current_dir(&out));

    build
        .include(&out)
        .include(&source)
        .define("WORDSIZE", "32")
        .define("MAXN", "32")
        .define("USE_TLS", None)
        .file(adapter);
    for file in [
        "nauty.c",
        "nautil.c",
        "naugraph.c",
        "schreier.c",
        "naurng.c",
    ] {
        build.file(source.join(file));
    }
    fs::write(
        out.join("compiler.txt"),
        format!("{:?}\n", build.get_compiler().to_command()),
    )
    .unwrap();
    build.compile("graphy_nauty");
}
