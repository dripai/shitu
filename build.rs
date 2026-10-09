#[path = "tools/translations.rs"]
mod translations;

fn main() {
    println!("cargo:rerun-if-env-changed=SHITU_BUILD_DATE");
    println!("cargo:rerun-if-changed=assets/app.ico");
    let build_date = std::env::var("SHITU_BUILD_DATE")
        .unwrap_or_else(|_| chrono::Utc::now().format("%Y-%m-%d").to_string());
    println!("cargo:rustc-env=SHITU_BUILD_DATE={build_date}");
    translations::compile();

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/app.ico");
        // GPUI embeds the process DPI/common-controls manifest. The installed
        // MSIX obtains its identity from packaging/AppxManifest.xml. There is
        // no external-location/sparse-package registration in this application.
        resource.compile().expect("compile Windows resources");
    }
}
