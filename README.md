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
   * Query metered and constrained status for network policy decisions
   * Check internet reachability
   * Cross-platform support (Windows, iOS, Android)

| Platform | Supported |
| -------- | --------- |
| Windows  | Planned   |
| macOS    | Planned   |
| Android  | Planned   |
| iOS      | Planned   |

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

## Install

_This plugin requires a Rust version of at least **1.89**_

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

Use the connection status to defer expensive operations on
constrained or metered connections:

```ts
import { connectionStatus } from '@silvermine/tauri-plugin-connectivity';

async function shouldDownload(): Promise<boolean> {
   const status = await connectionStatus();

   if (!status.connected) {
      console.debug('No internet connection');
      return false;
   }

   if (status.metered || status.constrained) {
      console.debug('Metered or constrained connection — deferring download');
      return false;
   }

   return true;
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

### Connection Status

The `connectionStatus()` function returns a `ConnectionStatus` object:

| Field            | Type             | Description                                                      |
| ---------------- | ---------------- | ---------------------------------------------------------------- |
| `connected`      | `boolean`        | Whether the device has an active internet connection             |
| `metered`        | `boolean`        | Whether data usage is billed or limited                          |
| `constrained`    | `boolean`        | Whether the connection is data-constrained or restricted         |
| `connectionType` | `ConnectionType` | The physical transport: `wifi`, `ethernet`, `cellular`, `unknown`|

#### Platform mapping

| Field         | Windows                                   | iOS                       | Android                          |
| ------------- | ----------------------------------------- | ------------------------- | -------------------------------- |
| `metered`     | `NetworkCostType` Fixed/Variable          | `NWPath.isExpensive`      | absence of `NOT_METERED`         |
| `constrained` | `ApproachingDataLimit`, `OverDataLimit`, `Roaming` | `NWPath.isConstrained`    | Data Saver / `RESTRICT_BACKGROUND` |
| `connectionType` | WWAN/WLAN/IANA interface type          | `NWInterface.InterfaceType` | `TRANSPORT_*` capabilities       |

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

## License

MIT

## Contributing

Contributions are welcome! Please follow the established coding standards
and commit message conventions.
