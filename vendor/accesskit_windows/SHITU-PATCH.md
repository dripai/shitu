# ShiTu stale-window fix

Based on the published `accesskit_windows` 0.34.0 source, upstream commit
`c88605b96d04431f9c3c792464a0f2f253480e94`.

- Upstream source: https://github.com/AccessKit/accesskit/tree/c88605b96d04431f9c3c792464a0f2f253480e94/platforms/windows
- Windows error contract: https://learn.microsoft.com/windows/win32/winauto/uiauto-error-codes

ShiTu v0.5.2 crash logs show `util.rs:380` panicking on an invalid HWND.
GPUI's delayed window teardown can leave a UIA request holding its context
after the native window is destroyed. The upstream main branch still uses
the same `ClientToScreen(...).unwrap()` as of 2026-10-28.

The local patch changes the private `client_top_left` helper to return a
`Result<Point>` and propagates failure as `UIA_E_ELEMENTNOTAVAILABLE` through
element bounds, hit testing, text range lookup, and text bounding rectangles.
Live-window coordinates and public Rust APIs are unchanged. Accessibility
remains enabled; no replacement coordinates are returned for destroyed windows.

`src/regression_tests.rs` checks valid windows and retained COM providers after
32 hidden-window destruction cycles. It does not send user input or show windows.

Run the targeted tests with:

```powershell
cargo test --manifest-path vendor/accesskit_windows/Cargo.toml --lib regression_tests -- --test-threads=1
```

Original copyright headers and Apache, MIT, and Chromium license files are retained.
