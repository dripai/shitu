# Slint popup boundary fix

This is the crates.io source of `i-slint-core` 1.17.0, copied without Cargo's
cache marker and the dependency crate's own lockfile. The original copyright
headers, license metadata and source files are preserved.

Upstream commit: `fdde7a535305d2ab2d4072dee637bad186a49723`.

Only `window.rs` is changed:

- `WindowInner::update_popup_properties` applies `popup::place_popup` with the
  native window bounds, as `show_popup` already does. Pointer movement no longer
  puts an initially constrained tooltip outside the window.
- The dirty region uses the final window-relative position, so moving a popup
  redraws its actual old and new locations.
- The misleading comment above the existing initial clamp is corrected.

The application continues to use Slint's native `Tooltip`. Its hover tracking,
delay, dismissal, input transparency and focus behavior are unchanged. There is
no added window, timer, focus handler or platform API.

Slint 1.17.0 exposes `Tooltip.text` and custom content, but no public placement
property. The local patch fixes the runtime behavior rather than introducing a
second tooltip implementation. Cargo uses this source through `[patch.crates-io]`.

Official source:

- [Tooltip declaration](https://github.com/slint-ui/slint/blob/fdde7a535305d2ab2d4072dee637bad186a49723/internal/compiler/builtins.slint)
- [Popup position updates](https://github.com/slint-ui/slint/blob/fdde7a535305d2ab2d4072dee637bad186a49723/internal/core/window.rs#L1439)
- [Popup boundary constraints](https://github.com/slint-ui/slint/blob/fdde7a535305d2ab2d4072dee637bad186a49723/internal/core/window/popup.rs)

Remove this override when a verified upstream version includes the same fix.
