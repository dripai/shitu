# Windows package identity

Windows AI OCR requires package identity and the `systemAIModels` capability. GridStart keeps its existing Win32 installation layout and uses an external-location identity package.

Files:

- `AppxManifest.xml`: identity package template, Windows App Runtime 1.8 dependency, and `systemAIModels` declaration.
- `ShiTu.exe.manifest`: side-by-side identity metadata embedded into `ShiTu.exe` by `build.rs`.

Before release, replace `CN=GridStart Development` in both manifests with the Microsoft Store publisher or signing certificate subject. The values for package name, publisher, and application ID must remain identical in both manifests.

Validate and build the unsigned identity package:

```powershell
MakeAppx.exe pack /o /d packaging /nv /p GridStart.Identity.msix
```

The package must be signed before registration. A development certificate must be trusted in `CurrentUser\TrustedPeople`; never commit a `.pfx` private key. Register the signed package against the directory containing `ShiTu.exe`:

```powershell
Add-AppxPackage -Path GridStart.Identity.msix -ExternalLocation <install-directory>
```

The Microsoft Store re-signs accepted MSIX submissions. Store identity and purchase integration are separate release tasks and are not implemented by this template.

Official references:

- https://learn.microsoft.com/windows/ai/apis/get-started
- https://learn.microsoft.com/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps
- https://learn.microsoft.com/windows/apps/package-and-deploy/packaging/
