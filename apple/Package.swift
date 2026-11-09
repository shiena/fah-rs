// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "FAHApple",
    platforms: [.macOS(.v15), .iOS(.v18)],
    products: [.library(name: "FAHAppleNative", type: .dynamic, targets: ["FAHAppleNative"]),
               .library(name: "FAHApple", targets: ["FAHApple"]),
               .executable(name: "fah-apple-check", targets: ["fah-apple-check"]),
               .executable(name: "fah-apple-camera", targets: ["fah-apple-camera"])],
    dependencies: [.package(path: "Vendor/fah-metal")],
    targets: [
        .binaryTarget(name: "FAHCore", path: "Artifacts/FAHCore.xcframework"),
        .target(name: "FAHAppleNative", dependencies: [.product(name: "FAHMetal", package: "fah-metal")]),
        .target(name: "FAHApple", dependencies: ["FAHCore", "FAHAppleNative"],
                linkerSettings: [.linkedFramework("Security"), .linkedLibrary("resolv"), .linkedLibrary("iconv")]),
        .executableTarget(name: "fah-apple-check", dependencies: ["FAHApple"]),
        .executableTarget(name: "fah-apple-camera", dependencies: ["FAHApple"]),
        .testTarget(name: "FAHAppleTests", dependencies: ["FAHApple", "FAHAppleNative"]),
    ],
    swiftLanguageModes: [.v5]
)
