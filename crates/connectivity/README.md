# Connectivity

Tauri-independent network connectivity detection for desktop Rust applications.

The crate provides synchronous APIs for Windows, Linux, and macOS:

   * `connection_status()` returns reachability, metered/constrained flags, and
     the primary connection type.
   * `supported_connection_types()` returns the transport classes reported by
     the current platform backend.

## Installation

```toml
[dependencies]
connectivity = { git = "https://github.com/silvermine/tauri-plugin-connectivity" }
```

## Usage

```rust,no_run
fn main() -> connectivity::Result<()> {
   let status = connectivity::connection_status()?;

   if status.connected
      && status.metered == Some(false)
      && status.constrained == Some(false)
   {
      println!("network is suitable for unrestricted work");
   }

   let supported_types = connectivity::supported_connection_types()?;
   println!("supported transports: {supported_types:?}");

   Ok(())
}
```

Both detection functions are synchronous because they call platform APIs
directly. Applications using an async runtime should run them on a blocking
worker thread.

The `metered` and `constrained` fields are `Option<bool>`. `None` means the
backend cannot determine the value; in particular, Linux's passive fallback
does not have cost or constraint information. Callers enforcing a restrictive
network policy should require `Some(false)` as shown above.

Android and iOS detection remain part of the Tauri plugin's native mobile
bridge and are not implemented by this crate.

## Tests

Run `cargo test -p connectivity --lib`. On Linux, the D-Bus regression tests
require `dbus-daemon` and start private buses with mock NetworkManager and
ModemManager services. They do not use or modify the system bus.

## License

MIT
