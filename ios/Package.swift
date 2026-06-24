// swift-tools-version: 5.5

import PackageDescription

let package = Package(
   name: "tauri-plugin-connectivity",
   platforms: [
      .iOS(.v15),
   ],
   products: [
      .library(
         name: "tauri-plugin-connectivity",
         type: .static,
         targets: ["tauri-plugin-connectivity"])
   ],
   dependencies: [
      .package(name: "Tauri", path: "../.tauri/tauri-api"),
      .package(name: "ConnectivityCore", path: "ConnectivityCore"),
   ],
   targets: [
      .target(
         name: "tauri-plugin-connectivity",
         dependencies: [
            .byName(name: "Tauri"),
            .product(name: "ConnectivityCore", package: "ConnectivityCore"),
         ],
         path: "Sources")
   ]
)
