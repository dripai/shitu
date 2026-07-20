fn main() {
    println!("cargo:rerun-if-changed=../../assets/app.ico");
    println!("cargo:rerun-if-changed=../../packaging/ShiTu.exe.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/app.ico");
        resource.set_manifest_file("../../packaging/ShiTu.exe.manifest");
        resource.compile().expect("compile Windows resources");
    }

    let library_paths =
        std::collections::HashMap::from([("shi-ui".to_owned(), shi_ui::slint_library_path())]);
    let config = slint_build::CompilerConfiguration::new()
        .with_library_paths(library_paths)
        .with_bundled_translations("translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config).expect("compile Slint UI");
}
