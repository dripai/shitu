fn main() {
    let rustc_version =
        std::process::Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|value| value.trim().to_owned())
            .unwrap_or_else(|| "Rust 版本未知".to_owned());
    println!("cargo:rustc-env=RUSTC_VERSION={rustc_version}");
    slint_build::compile("ui/app.slint").expect("compile Slint UI");
}
