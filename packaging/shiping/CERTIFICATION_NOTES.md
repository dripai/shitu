# ShiPing certification notes

ShiPing is an offline-first Windows desktop screen recorder. It does not require an account or a network connection and does not contain advertising, analytics, or telemetry.

## Test flow

1. Launch ShiPing.
2. Select a display, a visible window, or a fixed desktop region.
3. Select MP4 or GIF output and choose the output quality and frame rate.
4. For MP4, optionally enable system audio and microphone input.
5. Select **Start recording**. Use the toolbar, system tray, or configured global shortcuts to pause, resume, and stop.
6. The completed recording is saved in the folder shown in Preferences.

## Restricted capability

`runFullTrust` is required because ShiPing is a packaged Win32 desktop application. It uses Win32 APIs for:

- capturing the selected visible screen pixels;
- WASAPI system-audio and microphone capture;
- Media Foundation MP4 encoding;
- global keyboard shortcuts and the system tray;
- writing recordings to the user-selected local folder.

ShiPing does not upload recordings, screenshots, audio, settings, or usage data.

## Known behavior

- Window recording captures the pixels currently visible in the selected window area. Content covered by another window or located off-screen is not captured as an independent window surface.
- GIF output is silent. System audio and microphone input are available only for MP4.
- ShiPing records one selected display at a time and does not combine multiple displays into one recording.
