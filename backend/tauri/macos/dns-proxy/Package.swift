// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "ChimeraDNSProxy",
    platforms: [.macOS(.v12)],
    products: [
        .library(name: "ChimeraDNSProxy", targets: ["ChimeraDNSProxy"]),
    ],
    targets: [
        .target(name: "ChimeraDNSProxy"),
        .testTarget(name: "ChimeraDNSProxyTests", dependencies: ["ChimeraDNSProxy"]),
    ]
)
