# ShiPing Android

这是与桌面端隔离的 Android 应用入口。桌面端继续使用 Rust + Slint，Android
使用 Kotlin + Jetpack Compose Material 3；两条 UI 管线不会互相引入平台依赖。

## 当前实现

- `MainActivity` 使用 Activity Result API 请求系统录屏、录音和悬浮窗权限；
- Compose Material 3 提供紧凑录制首页、录像记录和关于页；
- 录像记录按文件逐行展示，并通过系统播放器打开可播放的 MP4；
- `RecordingService` 维持前台录制、常驻通知和可拖动的悬浮停止计时条；
- `MediaProjection` + `VirtualDisplay` 捕获屏幕；
- `MediaCodec` 通过 Surface 硬件编码 H.264；
- 可选系统声音或麦克风，经 `AudioRecord` + `MediaCodec` 编码 AAC；
- `MediaMuxer` 封装 MP4，并通过 `MediaStore` 保存到 `Movies/ShiPing`；
- Rust `cdylib` 保存跨 JNI 的录制状态快照。

声音来源当前为“无声音 / 系统声音 / 麦克风”三选一。系统声音要求 Android 10
或更高版本，并且只能捕获目标应用允许被录制的媒体、游戏或未知用途音频。录制中
旋转屏幕需要停止后重新开始；编码能力和 60 FPS 是否可用由设备的
`MediaCodec` 实现决定，不提供静默降级。

开始录制前必须授予“显示在其他应用上层”权限。悬浮计时条使用系统
`TYPE_APPLICATION_OVERLAY`，并设置 `FLAG_SECURE` 以避免进入录制画面；不同厂商
仍可能调整悬浮窗位置或可见性，需要逐台真机确认。

## 构建要求

- Rust `aarch64-linux-android` target；
- Android SDK 36；
- Android Build Tools 35.0.0；
- Android NDK；
- JDK 17；
- Gradle 8.13。

先编译 Rust 动态库，并放入 Gradle 的原生库目录：

```powershell
rustup target add aarch64-linux-android
$llvmBin = Join-Path $env:ANDROID_NDK_ROOT 'toolchains\llvm\prebuilt\windows-x86_64\bin'
$env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = Join-Path $llvmBin 'aarch64-linux-android26-clang.cmd'
cargo build --release --package shiping-android --target aarch64-linux-android

$nativeDir = 'apps\shiping-android\android\native-libs\arm64-v8a'
New-Item -ItemType Directory -Force -Path $nativeDir | Out-Null
Copy-Item `
  'target\aarch64-linux-android\release\libshiping_android.so' `
  (Join-Path $nativeDir 'libshiping_android.so')

Push-Location 'apps\shiping-android\android'
gradle :app:assembleDebug
Pop-Location
```

Android 目标的 Rust 动态库固定为 `libshiping_android.so`；Gradle 按 Android
ABI 目录约定将其打进 APK。

标签发布时，GitHub Actions 先用 NDK 编译 Rust `.so`，再用 Gradle 构建
Compose APK，并生成文件名带 `-test.apk` 的测试包。流水线使用临时测试证书
完成对齐、签名和签名验证；证书只存在于当前 runner，任务结束后销毁。

该测试证书每次构建都会变化，因此安装新标签的测试包前需要先卸载旧测试版。
测试包只用于安装和真机功能验证，不是应用商店正式发布包。

## 尚未由 CI 证明的内容

CI 只能证明 Rust/JNI、Kotlin/Compose 和 APK 编译通过。悬浮窗行为、系统播放器
跳转、厂商编码器、系统音频捕获、长时间录制和旋转屏幕必须在真实 Android 设备上
验证。基础系统录屏和 MP4 保存已完成一次真机验证，本次 UI 与悬浮窗改动仍需复测。
