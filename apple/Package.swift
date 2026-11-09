// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "FAHApple",
    platforms: [.macOS(.v15), .iOS(.v18)],
    products: [.library(name: "FAHApple", targets: ["FAHApple"]),
               .executable(name: "fah-apple-check", targets: ["fah-apple-check"])],
    dependencies: [.package(url: "https://github.com/LuckyIYI/fah-metal.git",
                            revision: "3ddc269a6a4ea8d921630ac4fc20d7d260375bca")],
    targets: [
        .binaryTarget(name: "FAHCore", path: "Artifacts/FAHCore.xcframework"),
        .target(name: "FAHApple", dependencies: ["FAHCore", .product(name: "FAHMetal", package: "fah-metal")],
                linkerSettings: [.linkedFramework("Security"), .linkedLibrary("resolv"), .linkedLibrary("iconv")]),
        .executableTarget(name: "fah-apple-check", dependencies: ["FAHApple"]),
        .testTarget(name: "FAHAppleTests", dependencies: ["FAHApple"]),
    ],
    swiftLanguageModes: [.v5]
)
