# Microsoft Store MSIX package

ShiTu is published to Microsoft Store as a complete x64 MSIX package. The package contains `ShiTu.exe` and its app icon, and uses the Store identity reserved for this product.

`AppxManifest.xml` is a template: `tools/package-store-msix.ps1` replaces `__PACKAGE_VERSION__` with the Cargo package version in MSIX format (`X.Y.Z.0`) while preparing the staging directory. The Store submission version therefore follows the Git tag, which is already required to match `Cargo.toml`.

The package declares `systemAIModels` for Windows AI OCR. It also declares `runFullTrust` because ShiTu is a packaged Win32 desktop application.

Build a Store upload asset after compiling the release executable:

```powershell
.\tools\package-store-msix.ps1 -ExecutablePath .\target\release\ShiTu.exe -Version 0.1.1 -OutputDirectory .\release-assets
```

The script produces:

- `ShiTu-<version>-windows-x64.msix`: the unsigned MSIX package.
- `ShiTu-<version>-store.msixupload`: the MSIX wrapped in the Store upload format.

The `.msixupload` file is for Partner Center. Microsoft Store re-signs accepted MSIX submissions, so no private signing certificate is stored in this repository or used by the release workflow. The unsigned package must not be distributed for sideloading.

Official references:

- https://learn.microsoft.com/windows/apps/package-and-deploy/choose-distribution-path
- https://learn.microsoft.com/windows/apps/publish/publish-your-app/msix/upload-app-packages
- https://learn.microsoft.com/windows/ai/apis/get-started
