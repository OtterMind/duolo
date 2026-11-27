# 发布

签名打包由 `.github/workflows/release.yml` 做：

- **推 `v*` 标签**（例如 `git tag v0.2.0 && git push origin v0.2.0`）：打 Mac 版、iPhone 版和 Android 版。Mac 版的 dmg 放进同名的草稿 Release，
  看过没问题再在 GitHub 上发布；iPhone 版的 ipa 在这次运行的 Artifacts 里。
- **手动运行**（Actions → Release → Run workflow）：可以只打其中一个；勾上 TestFlight 时 iPhone 版直接传到 App Store Connect。

## Mac 版

Developer ID Application 证书签名，再交给苹果公证，下载后能直接打开。签名后系统强制开启加固运行时，
`crates/duolo-app/Entitlements.plist` 里声明了麦克风（按住说话要用）。打的是通用二进制，一个 dmg 同时支持 Apple 芯片和 Intel。

## iPhone 版

手动签名：发布证书加上 App Store 描述文件（`IOS_APPSTORE_PROFILE`），导出成上架用的 ipa（`app-store-connect`）。
不用 Xcode 的自动签名：API Key 是 App Manager 权限，用不了它的"云端签名"。勾上 TestFlight 时用 API Key 上传。
构建号是这次运行的编号加 100，每次都比上次大；版本号取标签（`v1.2.3` → `1.2.3`，`v1.2.3-rc1` 也是 `1.2.3`，App Store 只认纯数字），手动运行时用 `mobile/ios/project.yml` 里的。

传到 TestFlight 之前，App Store Connect 里要先建好 Bundle ID 为 `io.duolo.app` 的 App。

## Android 版

用上传密钥签名的 aab（`mobile/android/app/build.gradle.kts` 从环境变量读密钥），在这次运行的 Artifacts 里。
Google Play 开了"Play 应用签名"：用户装到的包由 Google 重新签名，上传密钥丢了可以在 Play Console 里申请换一把。
versionCode 是这次运行的编号加 100，versionName 取标签。

## 仓库里的密钥（Settings → Secrets and variables → Actions）

| 名字 | 是什么 |
|---|---|
| `APPLE_TEAM_ID` | 苹果开发者团队编号 |
| `MACOS_CERTIFICATE`、`MACOS_CERTIFICATE_PASSWORD`、`MACOS_SIGNING_IDENTITY` | Developer ID Application 证书（p12，base64）、密码、证书名 |
| `IOS_CERTIFICATE`、`IOS_CERTIFICATE_PASSWORD` | Apple Development 证书（p12，base64）和密码，装测试手机用；发布流程不用 |
| `IOS_DISTRIBUTION_CERTIFICATE`、`IOS_DISTRIBUTION_CERTIFICATE_PASSWORD` | Apple Distribution 证书（p12，base64）和密码 |
| `IOS_APPSTORE_PROFILE` | App Store 描述文件 "Duolo App Store"（base64），和发布证书配对 |
| `IOS_PROVISIONING_PROFILE` | 开发描述文件（base64），装测试手机用；发布流程不用 |
| `ANDROID_UPLOAD_KEYSTORE`、`ANDROID_UPLOAD_KEYSTORE_PASSWORD`、`ANDROID_UPLOAD_KEY_ALIAS` | Google Play 上传密钥（PKCS12，base64）、密码（密钥库和密钥同一个）、别名 |
| `APPLE_API_KEY_P8`、`APPLE_API_KEY_ID`、`APPLE_API_ISSUER` | App Store Connect API Key（App Manager）：Mac 版公证、iPhone 版传 TestFlight |

证书一年到期，到期前在开发者后台新建一张，换掉对应的两项（p12 和密码）；换了发布证书，App Store 描述文件也要重新生成。这些文件和密码的本地副本由负责发布的人保管，不进仓库。
