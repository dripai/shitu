# ShiPing Android

这是与桌面端隔离的 Android 应用入口。当前阶段只验证以下边界：

- Slint Android 移动首页；
- Rust `android_main` 入口；
- Rust 调用 Kotlin `NativeActivity`；
- Kotlin 返回系统录屏授权结果给 Rust。

编码、封装、系统声音、麦克风和前台录制服务尚未实现。

## 构建要求

- Rust `aarch64-linux-android` target；
- Android SDK 33（当前固定版本 xbuild 的 NDK/AGP 打包上限）；
- Android NDK；
- 可供 Android Gradle 构建使用的 JDK；
- 从指定提交安装的 `xbuild`。

```powershell
rustup target add aarch64-linux-android
cargo install --git https://github.com/rust-mobile/xbuild.git --rev e67b501cbe0e9ca5436c223aa7ec0fe5e27544d1
x build --package shiping-android --platform android --arch arm64 --format apk --release
```

APK 预期输出到 `target/x/release/android/`。真机运行仍需验证触控、系统录屏授权和 Activity 生命周期。

标签发布时，GitHub Actions 会生成文件名带 `-test.apk` 的测试包。`xbuild`
产生的 release APK 本身未签名，流水线随后使用临时测试证书完成对齐、签名和
签名验证。临时证书只存在于当前 GitHub Actions runner，任务结束后销毁。

该测试证书每次构建都会变化，因此安装新标签的测试包前需要先卸载旧测试版。
测试包只用于安装和真机功能验证，不是应用商店正式发布包。
