// swift-tools-version:6.0
import PackageDescription
let package = Package(
    name: "fah-metal",
    platforms: [.macOS(.v15), .iOS(.v18)],
    products: [.library(name: "FAHMetal", targets: ["FAHMetal"])],
    targets: [.target(name: "FAHMetal", resources: [.copy("Shaders"), .copy("NOTICE")])],
    swiftLanguageModes: [.v5]
)
