// swift-tools-version: 6.0
import PackageDescription

// 手机端的核心：协议、加密、同步、本地保存、列表和时间线的推导。不依赖界面和第三方库，`swift test` 就能测。
let package = Package(
    name: "DuoloKit",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [.library(name: "DuoloKit", targets: ["DuoloKit"])],
    targets: [
        .target(name: "DuoloKit", linkerSettings: [.linkedLibrary("sqlite3")]),
        .testTarget(name: "DuoloKitTests", dependencies: ["DuoloKit"]),
    ]
)
