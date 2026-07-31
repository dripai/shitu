# ShiPing Android

这是与桌面端隔离的 Android 应用入口。桌面端继续使用 Rust + Slint，Android
使用 Kotlin + Jetpack Compose Material 3；两条 UI 管线不会互相引入平台依赖。

## 当前实现

- `MainActivity` 使用 Activity Result API 请求系统录屏和录音权限，授权成功后自动将应用退到后台；
- Compose Material 3 提供紧凑录制首页、录像记录和关于页；
- 录像记录支持系统播放器打开、长按重命名、删除和查看媒体详情；
- `RecordingService` 维持前台录制和带停止操作的常驻通知；
- `MediaProjection` + `VirtualDisplay` 捕获屏幕；
- `MediaCodec` 通过 Surface 硬件编码 H.264；
- 可选系统声音或麦克风，经 `AudioRecord` + `MediaCodec` 编码 AAC；
- `MediaMuxer` 封装 MP4，并通过 `MediaStore` 保存到 `Movies/ShiPing`；
- Kotlin `StateFlow` 保存录制状态并驱动 Compose 界面。

声音来源当前为“无声音 / 系统声音 / 麦克风”三选一。系统声音要求 Android 10
或更高版本，并且只能捕获目标应用允许被录制的媒体、游戏或未知用途音频。录制中
旋转屏幕需要停止后重新开始；编码能力和 60 FPS 是否可用由设备的
`MediaCodec` 实现决定，不提供静默降级。

应用不创建录制悬浮窗，也不申请“显示在其他应用上层”权限。录制时可从常驻通知
停止，或者返回 ShiPing 后停止。部分聊天、金融或隐私页面受系统和目标应用的安全
策略保护，录制结果可能显示黑屏或模糊，ShiPing 不会绕过这些安全限制。

## 构建要求

- Android SDK 36；
- Android Build Tools 35.0.0；
- JDK 17；
- Gradle 8.13。

直接使用 Gradle 构建 Kotlin/Compose APK：

```powershell
Push-Location 'apps\shiping-android\android'
gradle :app:assembleDebug
Pop-Location
```

标签发布时，GitHub Actions 用 Gradle 构建 Compose APK，并生成文件名带
`-test.apk` 的测试包。流水线使用临时测试证书
完成对齐、签名和签名验证；证书只存在于当前 runner，任务结束后销毁。

该测试证书每次构建都会变化，因此安装新标签的测试包前需要先卸载旧测试版。
测试包只用于安装和真机功能验证，不是应用商店正式发布包。

## 尚未由 CI 证明的内容

CI 只能证明 Kotlin/Compose 单元测试和 APK 编译通过。系统播放器跳转、媒体重命名
和删除、厂商编码器、系统音频捕获、自动退到后台、长时间录制和旋转屏幕必须在真实
Android 设备上验证。基础系统录屏和 MP4 保存已完成一次真机验证，本次交互与纯
Kotlin 构建改动仍需复测。
