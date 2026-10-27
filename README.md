# Tauri Plugin Connectivity

[![CI][ci-badge]][ci-url]

Cross-platform network connectivity detection for Tauri 2.x apps.

This plugin provides a unified API for querying network connection status,
including connection type (WiFi, Ethernet, Cellular), metered/constrained
flags, and reachability. It is designed to help apps make network policy
decisions.

[ci-badge]: https://github.com/silvermine/tauri-plugin-connectivity/actions/workflows/ci.yml/badge.svg
[ci-url]: https://github.com/silvermine/tauri-plugin-connectivity/actions/workflows/ci.yml

## Features

   * Detect connection type (WiFi, Ethernet, Cellular)
   * Query platform-reported transport classes for policy settings
   * Query metered and constrained status for network policy decisions
   * Check internet reachability
   * Cross-platform support (Windows, Linux, macOS, iOS, Android)

| Platform | Supported |
| -------- | --------- |
| Windows  | Yes       |
| Linux    | Yes       |
| macOS    | Yes       |
| Android  | Yes       |
| iOS      | Yes       |

## Architecture

The Rust implementation is split into two crates:

   * `connectivity` (`crates/connectivity`) is Tauri-independent and contains
     the Windows, Linux, and macOS detection backends, public models, and errors.
   * `tauri-plugin-connectivity` (the repository root) is the thin Tauri layer.
     It provides commands, managed state, permissions, and the Android/iOS
     native plugin bridge.

Desktop Rust code that does not need Tauri can depend on `connectivity`
directly. The Tauri plugin consumes the same crate and re-exports its public
models and error types.

## Getting Started

### Installation

1. Install NPM dependencies:

   ```bash
   npm install
   ```

2. Build the TypeScript bindings:

   ```bash
   npm run build
   ```

3. Build the Rust plugin:

   ```bash
   cargo build
   ```

### Tests

Run all tests (TypeScript and Rust):

```bash
npm test
```

Run TypeScript tests only:

```bash
npm run test:ts
```

Run Rust tests only:

```bash
cargo test --workspace --lib
```

Run the Swift tests for the iOS connectivity mapper (requires macOS and a Swift
toolchain):

```bash
swift test --package-path ios/ConnectivityCore
```

Run the Kotlin tests for the Android connectivity mapper (requires JDK 17+,
Gradle 9.6+ via the repo wrapper, and the Android SDK):

```bash
cd android && ./gradlew :lib:test
```

To compile the full Android plugin module (AGP 9.3, `compileSdk` 37), populate
`android/.tauri/tauri-api` first by building the crate for Android (see
[Tauri mobile plugin development](https://v2.tauri.app/develop/plugins/develop-mobile/)),
then run `./gradlew assembleDebug` from `android/`. CI runs the same flow with
`-PembeddedHostPluginBuild` to compile the host-embedded plugin layout (see
`.github/workflows/ci.yml`). Host apps on the Tauri 2.12 template need Gradle
9.3.1+ for the Kotlin `compilerOptions` DSL.

Before releasing a version that bumps `compileSdk`, complete the **Android 17
(API 37)** row in
[Android Connectivity Manual Testing](docs/android-connectivity-manual-testing.md)
on a device or emulator (not run in CI).

The Swift and Kotlin tests cover the platform mapping logic, which lives in
modules that do not depend on the Tauri mobile APIs
(`ios/ConnectivityCore` and `android/lib`) so the suites can run without an
emulator, a simulator, or a Tauri app build.

### Manual Linux scenario testing

See [Linux Connectivity Manual Testing](docs/linux-connectivity-manual-testing.md)
for WSL2, VirtualBox, NetworkManager, ModemManager, metered, constrained, and
transport-type test scenarios.

### Manual macOS scenario testing

See [macOS Connectivity Manual Testing](docs/macos-connectivity-manual-testing.md)
for current-path transport inventory, disconnected, VPN, metered, and
constrained scenarios.

## Install

_This plugin requires a Rust version of at least **1.94.0**_

### Rust

Add the plugin to your `Cargo.toml`:

`src-tauri/Cargo.toml`

```toml
[dependencies]
tauri-plugin-connectivity = { git = "https://github.com/silvermine/tauri-plugin-connectivity" }
```

### JavaScript/TypeScript

Install the JavaScript bindings:

```sh
npm install @silvermine/tauri-plugin-connectivity
```

## Usage

### Prerequisites

Initialize the plugin in your `tauri::Builder`:

```rust
fn main() {
   tauri::Builder::default()
      .plugin(tauri_plugin_connectivity::init())
      .run(tauri::generate_context!())
      .expect("error while running tauri application");
}
```

### API

#### Query connection status

```ts
import { connectionStatus } from '@silvermine/tauri-plugin-connectivity';

async function checkConnection() {
   const status = await connectionStatus();

   console.debug(`Connected: ${status.connected}`);
   console.debug(`Type: ${status.connectionType}`);
   console.debug(`Metered: ${status.metered}`);
   console.debug(`Constrained: ${status.constrained}`);
}
```

#### Make network policy decisions

Use the connection status to defer expensive operations unless the connection
is confirmed unrestricted:

```ts
import { connectionStatus } from '@silvermine/tauri-plugin-connectivity';

async function shouldDownload(): Promise<boolean> {
   const status = await connectionStatus();

   if (!status.connected) {
      console.debug('No internet connection');
      return false;
   }

   if (status.metered !== false || status.constrained !== false) {
      console.debug('Restricted or unknown connection policy — deferring download');
      return false;
   }

   return true;
}
```

#### Query supported transport classes

Use `supportedConnectionTypes()` when the app needs to show policy settings for
transport classes reported by the current platform backend. On Android,
transports declared as `PackageManager` system features are reported even when
inactive; other transports are reported only while currently active:

```ts
import { supportedConnectionTypes } from '@silvermine/tauri-plugin-connectivity';

async function showPolicyOptions() {
   const supportedTypes = await supportedConnectionTypes();

   console.debug(`Supported transports: ${supportedTypes.join(', ')}`);
}
```

#### Use from Rust

Access the connectivity API from any Tauri manager type via the
extension trait:

```rust
use tauri_plugin_connectivity::ConnectivityExt;

fn check_connection<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
   match app.connectivity().connection_status() {
      Ok(status) => {
         println!("Connected: {}", status.connected);
         println!("Type: {:?}", status.connection_type);
      }
      Err(e) => eprintln!("Could not detect connection status: {e}"),
   }
}
```

#### Use the standalone desktop crate from Rust

Add the Tauri-independent workspace crate directly when connectivity detection
is needed outside the plugin:

```toml
[dependencies]
connectivity = { git = "https://github.com/silvermine/tauri-plugin-connectivity" }
```

```rust
fn check_connection() -> connectivity::Result<bool> {
   let status = connectivity::connection_status()?;
   Ok(status.connected
      && status.metered == Some(false)
      && status.constrained == Some(false))
}
```

The desktop detection calls are synchronous. Async applications should run
them on a blocking worker thread. The Rust-facing `metered` and `constrained`
fields are `Option<bool>`; `None` means the backend could not determine the
value. Restrictive policies should require `Some(false)` rather than treating
an unknown value as safe.

### Connection Status

The `connectionStatus()` function returns a `ConnectionStatus` object:

| Field            | Type              | Description                                                       |
| ---------------- | ----------------- | ----------------------------------------------------------------- |
| `connected`      | `boolean`         | Whether the device has an active network path                     |
| `metered`        | `boolean \| null` | Whether data usage is billed or limited, or `null` when unknown   |
| `constrained`    | `boolean \| null` | Whether the connection is constrained, or `null` when unknown     |
| `connectionType` | `ConnectionType`  | The physical transport: `wifi`, `ethernet`, `cellular`, `unknown` |

For `metered` and `constrained`, `true` means the restriction is confirmed,
`false` means the connection is confirmed unrestricted, and `null` means the
platform could not determine the value. Restrictive policies should require
both fields to be exactly `false`, as shown above, rather than treating an
unknown value as safe.

ModemManager registration states `5`, `7`, and `10` are recognized as roaming
and therefore report `constrained: true` in both Rust and JavaScript. States `7`
and `10` previously reported `false` to JavaScript; this is an intentional
correction.

### Supported Connection Types

The `supportedConnectionTypes()` function returns `ConnectionType[]`. The array
is deduplicated and excludes `unknown`. Its availability semantics are
platform-specific, as described below. An empty array means detection succeeded
but found no supported transports. Detection failures reject the promise when
no supported transport can be recovered; when at least one transport is
recovered, the array can be a best-effort partial inventory if another
interface cannot be inspected.

| Platform | Mapping |
| -------- | ------- |
| Windows  | Present hardware-backed adapters from Win32 `GetAdaptersAddresses()` and `GetIfEntry2()` mapped by IANA interface type |
| Linux    | NetworkManager realized `Devices` mapped by `DeviceType`; sysfs fallback when NetworkManager is unavailable |
| macOS    | Interfaces of the current satisfied `NWPath` mapped by `nw_interface_get_type()` |
| iOS      | Interfaces of the current satisfied `NWPath.availableInterfaces` mapped by `NWInterface.InterfaceType` |
| Android  | `PackageManager` hardware features plus current `ConnectivityManager` networks |

Android does not expose a complete public SDK inventory of inactive removable
network adapters. Transports that are not declared as `PackageManager` system
features are reported only while Android exposes them as currently active
through `ConnectivityManager`.

On macOS and iOS, the array reflects the interfaces of the current network
path reported by `NWPathMonitor`, so inactive transports are not listed.

#### Platform mapping

| Field            | Windows                                                                             | Linux                                             | macOS                                          | iOS                         | Android                            |
| ---------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------- | ---------------------------------------------- | --------------------------- | ---------------------------------- |
| `connected`      | `InternetAccess` or `ConstrainedInternetAccess`                                     | NetworkManager `FULL`/`PORTAL`/`LIMITED` or up IPv4/IPv6 default route fallback | `nw_path_get_status == satisfied`              | `NWPath.status` satisfied   | `NET_CAPABILITY_INTERNET`          |
| `metered`        | `NetworkCostType` Fixed/Variable; Unknown is `None` in Rust and `null` in JavaScript | NetworkManager primary device `Metered`; unknown/unavailable signals are `None` in Rust and `null` in JavaScript | `nw_path_is_expensive`                         | `NWPath.isExpensive`        | absence of `NOT_METERED`           |
| `constrained`    | `ConstrainedInternetAccess`, data-limit, roaming, or background data restrictions; computed independently of `metered`, so unknown cost alone does not make this `None` | NetworkManager portal/limited/metered or cellular roaming; unknown/unavailable signals are `None` in Rust and `null` in JavaScript | `nw_path_is_constrained`                       | `NWPath.isConstrained`      | missing `VALIDATED`, or Data Saver / `RESTRICT_BACKGROUND_STATUS` on a metered active network |
| `connectionType` | WWAN/WLAN/IANA interface type                                                       | NetworkManager device type or sysfs fallback      | First `nw_path_enumerate_interfaces` entry     | `NWPath.usesInterfaceType(_:)` priority | `TRANSPORT_*` capabilities         |

## Development Standards

This project follows the
[Silvermine standardization](https://github.com/silvermine/standardization)
guidelines. Key standards include:

   * **EditorConfig**: Consistent editor settings across the team
   * **Markdownlint**: Markdown linting for documentation
   * **Commitlint**: Conventional commit message format
   * **Code Style**: 3-space indentation, LF line endings

### Running Standards Checks

```bash
npm run standards
```

### Continuous Integration

[CI](.github/workflows/ci.yml) selects the project's jobs and calls local reusable
workflows with `workflow_call`. Each job has an explicit timeout and read-only
repository permissions. New commits cancel older runs for the same branch or PR.
Linux package setup also has a ten-minute step limit and bounded network retries.

| Job | Checks | Default timeout |
| --- | --- | --- |
| Checks | Workflow lint, JavaScript standards/tests, Rust format, commitlint | 10 min |
| Rust | Workspace Clippy and tests on Linux, Windows, and macOS | 30 min |
| Android | Kotlin tests, Rust plugin build, Tauri API scaffolding, AAR | 45 min |
| iOS | Swift core tests and scoped Rust cross-compilation | 30 min |
| Continuous Integration | Requires every CI job to succeed | 5 min |
| Rust Dependency Policy | Advisories, licenses, bans, and sources | 15 min |

The aggregate `Continuous Integration` check fails when a required job fails, is
cancelled, or is skipped, including dependency policy. Pushes and PRs invoke the
policy from CI. Its separate scheduled/manual entry point runs only the policy,
without starting platform jobs or duplicating the push/PR check.

Setup reads Node.js from `.nvmrc`, npm from `package.json`'s `packageManager`, and
Rust from the root `Cargo.toml`'s `rust-version`. The Rust setup action requires
Python 3.11+ with `tomllib`, available on the selected hosted runners. Rust
compilation uses the lockfile and cached build dependencies. Java setup owns the
Gradle cache. Android SDK and NDK packages are explicit, and the project keeps its
Gradle wrapper. The Rust
plugin build generates `android/.tauri/tauri-api` before Gradle assembles the AAR.
Android reports are retained for seven days when a job fails.

For local checks, `npm run standards` still runs the JavaScript and Rust standards
and commitlint together. `npm run standards:js` runs the JavaScript standards and
npm version check separately. `npm ci` runs `prepare`, which builds the bindings.
CI checks all PR commits against the base SHA, checks the pushed range when its
previous commit is still an ancestor, and checks the last commit for a new branch,
rewritten history, or a manual run.

The reusable jobs expose timeout and command/task arguments, plus runner inputs
where needed. Checks uses a Linux runner because workflow lint runs in Docker;
Android setup requires Linux x86_64 for the NDK host tools. A project can
override JavaScript commands, Rust feature/test arguments, Linux packages, Android
SDK/Gradle settings, and Swift package or iOS compilation settings in the caller.
Cargo and Gradle argument inputs are space-separated tokens, not shell programs;
wildcards remain literal. JavaScript command inputs are trusted workflow code.

To adopt the template in another plugin, copy the workflow and local action files
together, declare the exact npm version in `packageManager`, and adjust its caller.
Provide `standards:js` and the JavaScript test command, preserving the project's
existing checks. Keep special procedures, such as emulator tests, minified consumers, and
example builds, in clearly named project jobs. Add each required job to the
aggregate check's `needs` list. Custom mobile jobs can reuse the Android setup
composite without using the AAR workflow. The standard Android test task runs
before Tauri scaffolding and supports isolated JVM tests; tests that require the
plugin module must run after scaffolding in a project-owned sequence.

Connectivity's iOS job tests `ConnectivityCore` and checks the Rust plugin for
`aarch64-apple-ios`. It does not build or test the Swift plugin inside a consumer
host. Other callers select `swift-package`, `rust-target`, and package-specific
`cargo-args`; the iOS job must enable at least one check.

Preserve these project-specific checks when adopting the template:

   * **audio:** ALSA, Java 21, the consumer APK, native unit tests, and compilation
     of device tests. Its wrapper is inside the example; use a custom Android job.
   * **connectivity:** desktop tests, Kotlin mapping tests, the AAR, Swift core
     tests, and the iOS Rust check selected by this caller.
   * **download:** Kotlin tests, the AAR, Swift package tests, and a package-scoped
     iOS Rust check. Device instrumentation remains separate validation.
   * **fs-resolver:** the Gradle contract check, Linux workspace/example linting,
     package-scoped native tests, and iOS Clippy in a custom platform job.
   * **http-client:** an Android Rust cross-check using shared SDK/NDK setup,
     without inventing an AAR or Gradle build.
   * **media-parser:** full workspace integration tests and doctests, native
     thumbnail backends, iOS plugin/backend builds, Android test linking, and the
     minified consumer/emulator tests. Keep its platform orchestration explicit.
   * **sqlite:** integration tests, `check:iife`, and both example builds. Use
     `test-args: --workspace --lib --test *` without shell quotes around `*`.
   * **text-scale:** Swift/Kotlin tests, mobile Clippy, and both standalone and
     embedded-host AAR builds as separate Gradle invocations in a custom job.

Local actions resolve in the checked-out project. Moving these workflows to a
shared repository later also requires versioned references for the shared setup
actions; changing only the workflow's `uses` path is not sufficient. No shared
hosting repository is required for this pilot.

### Dependency Policy

Rust dependency policy is checked on pushes, pull requests, and weekly against the
committed workspace lockfile. Install cargo-deny and run the same check locally:

```bash
cargo install cargo-deny --version 0.20.2 --locked
cargo deny --workspace --all-features --locked check
```

`deny.toml` checks advisories, licenses, duplicate versions, and dependency sources
across all platforms, including development dependencies. Duplicate versions produce
warnings; wildcard dependencies and unapproved sources fail the check. The example
app has a separate lockfile and is not included in this workspace audit.

Update affected dependencies before adding advisory exceptions. Each exception must
identify the upstream constraint and explain why an update cannot resolve it. Remove
exceptions when upstream dependencies no longer require them. Review license additions
and allow only the registries and Git repositories the workspace needs.

## License

MIT

## Contributing

Contributions are welcome! Please follow the established coding standards
and commit message conventions.
