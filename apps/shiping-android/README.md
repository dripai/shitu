# ShiPing Android

这是与桌面端隔离的 Android 应用入口。当前阶段只验证以下边界：

- Slint Android 移动首页；
- Rust `android_main` 入口；
- Rust 调用 Kotlin `NativeActivity`；
- Kotlin 返回系统录屏授权结果给 Rust。

编码、封装、系统声音、麦克风和前台录制服务尚未实现。

## 构建要求

- Rust `aarch64-linux-android` target；
- Android SDK 35；
- Android NDK；
- 可供 Android Gradle 构建使用的 JDK；
- 从指定提交安装的 `xbuild`。

```powershell
rustup target add aarch64-linux-android
cargo install --git https://github.com/rust-mobile/xbuild.git --rev e67b501cbe0e9ca5436c223aa7ec0fe5e27544d1
x build --package shiping-android --platform android --arch arm64 --format apk --release
```

APK 预期输出到 `target/x/release/android/`。真机运行仍需验证触控、系统录屏授权和 Activity 生命周期。

标签发布时，GitHub Actions 会生成文件名带 `-test.apk` 的测试包。该包使用
`xbuild` 内置调试证书签名，只用于安装验证，不是应用商店正式发布包。
