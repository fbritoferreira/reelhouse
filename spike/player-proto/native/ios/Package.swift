// swift-tools-version:5.9
import PackageDescription

// dx builds the product named after the Rust opaque type (PlayerBridge) into DioxusSwiftPlugins.framework.
let package = Package(
    name: "PlayerBridge",
    platforms: [.iOS(.v15), .macOS(.v12)],
    products: [.library(name: "PlayerBridge", type: .dynamic, targets: ["PlayerBridge"])],
    targets: [.target(name: "PlayerBridge", path: "Sources/PlayerBridge")]
)
