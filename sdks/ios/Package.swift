// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Experimentation",
    platforms: [.iOS(.v15), .macOS(.v13)],
    products: [
        .library(name: "Experimentation", targets: ["Experimentation"]),
    ],
    dependencies: [
        // ConnectRPC Swift will be added when Agent-1 implements transport
        // .package(url: "https://github.com/connectrpc/connect-swift.git", from: "0.12.0"),
    ],
    targets: [
        .target(
            name: "Experimentation",
            dependencies: [],
            path: "Sources/Experimentation"
        ),
        .testTarget(
            name: "ExperimentationTests",
            dependencies: ["Experimentation"],
            path: "Tests/ExperimentationTests"
        ),
    ]
)
