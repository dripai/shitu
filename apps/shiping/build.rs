fn main() {
    let library_paths =
        std::collections::HashMap::from([("shi-ui".to_owned(), shi_ui::slint_library_path())]);
    let config = slint_build::CompilerConfiguration::new()
        .with_library_paths(library_paths)
        .with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("compile ShiPing Slint UI");
}
