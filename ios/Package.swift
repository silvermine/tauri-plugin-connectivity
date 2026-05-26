// swift-tools-version:5.3

import PackageDescription

let package = Package(
  name: "tauri-plugin-connectivity",
  platforms: [
    .iOS(.v12),
  ],
  products: [
    .library(
      name: "tauri-plugin-connectivity",
      type: .static,
      targets: ["tauri-plugin-connectivity"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-connectivity",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
