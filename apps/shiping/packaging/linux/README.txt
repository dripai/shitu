ShiPing for Linux
==================

Run ./ShiPing from this directory.

Runtime requirements:
- A Wayland desktop with XDG Desktop Portal ScreenCast support.
- PipeWire and the pw-record command.
- FFmpeg available as the ffmpeg command.

The share directory contains an optional desktop entry and application icon.
To integrate ShiPing into your desktop environment, place them in the
corresponding locations below and make sure ShiPing is available on PATH:

  share/applications/com.dripai.shiping.desktop
  share/icons/hicolor/256x256/apps/com.dripai.shiping.png

This build has passed CI compilation and unit tests. Screen permissions, audio
devices, multiple displays, scaling, and long recordings require validation on
real Linux hardware.
