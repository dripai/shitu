# Microsoft Store MSIX packages

ShiTu is published to Microsoft Store as a complete x64 MSIX package. ShiPing is prepared for a separate x64 MSIX submission. Each package contains one product executable and its app icon, and uses the Microsoft Store identity reserved for that product.

The manifests are intentionally isolated by product:

- `packaging/shitu/AppxManifest.xml`
- `packaging/shiping/AppxManifest.xml`

`tools/package-store-msix.ps1` requires an explicit `-Product` argument and replaces `__PACKAGE_VERSION__` with the selected Cargo package version in MSIX format (`X.Y.Z.0`) while preparing the staging directory. It rejects an executable whose file name does not match the selected product.

ShiTu declares `systemAIModels` and the Windows App Runtime dependency for enhanced Windows AI OCR. ShiPing does not declare either dependency. Both packages declare `runFullTrust` because they are packaged Win32 desktop applications.

Build Store upload assets after compiling the release executables:

```powershell
.\tools\package-store-msix.ps1 -Product ShiTu -ExecutablePath .\target\release\ShiTu.exe -Version 0.1.9 -OutputDirectory .\release-assets
.\tools\package-store-msix.ps1 -Product ShiPing -ExecutablePath .\target\release\ShiPing.exe -Version 0.1.9 -OutputDirectory .\release-assets
```

For each selected product, the script produces:

- `<product>-<version>-windows-x64.msix`: the unsigned MSIX package.
- `<product>-<version>-store.msixupload`: the MSIX wrapped in the Store upload format.

The `.msixupload` file is for Partner Center. Microsoft Store signs accepted MSIX submissions, so no private signing certificate is stored in this repository or used by the release workflow. The unsigned package must not be distributed for sideloading.

Official references:

- https://learn.microsoft.com/windows/apps/package-and-deploy/choose-distribution-path
- https://learn.microsoft.com/windows/apps/publish/publish-your-app/msix/upload-app-packages
- https://learn.microsoft.com/windows/ai/apis/get-started
