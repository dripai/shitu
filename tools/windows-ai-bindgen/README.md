# Windows AI Rust bindings

`windows_ai_bindings.rs` is generated from official Windows App SDK metadata. Do not edit the generated file by hand.

Current source versions:

- `Microsoft.WindowsAppSDK` `1.8.260508005`
- `Microsoft.WindowsAppSDK.AI` `1.8.76`
- `windows-bindgen` `0.62.1`

After extracting the AI NuGet package, regenerate with:

```powershell
cargo run --offline --manifest-path tools/windows-ai-bindgen/Cargo.toml -- <path-to-extracted-metadata-directory>
```

The metadata directory must contain:

- `Microsoft.Windows.AI.winmd`
- `Microsoft.Windows.AI.Imaging.winmd`
- `Microsoft.Graphics.Imaging.winmd`

Official references:

- https://learn.microsoft.com/windows/apps/windows-app-sdk/release-notes/windows-app-sdk-1-8
- https://github.com/microsoft/windows-rs/tree/master/crates/libs/bindgen
