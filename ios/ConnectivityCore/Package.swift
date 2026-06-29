// swift-tools-version: 5.5

import PackageDescription

// Pure connectivity mapping, isolated from the tauri-plugin-connectivity
// target — that target depends on the UIKit/WebKit-based Tauri package
let package = Package(
   name: "ConnectivityCore",
   platforms: [
      .iOS(.v15),
      .macOS(.v10_13),
   ],
   products: [
      .library(
         name: "ConnectivityCore",
         targets: ["ConnectivityCore"])
   ],
   targets: [
      .target(
         name: "ConnectivityCore",
         path: "Sources/ConnectivityCore"),
      .testTarget(
         name: "ConnectivityCoreTests",
         dependencies: ["ConnectivityCore"],
         path: "Tests/ConnectivityCoreTests"),
   ]
)
