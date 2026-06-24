# iOS Connectivity Manual Testing

Manual iOS scenarios for exercising the native iOS backend under
`ios/Sources`.

The iOS implementation uses `NWPathMonitor` to observe the current default
network path. It reports connected when the latest `NWPath` has status
`.satisfied`, `metered` from `NWPath.isExpensive`, and `constrained` from
`NWPath.isConstrained`. The connection type is resolved by the pure
`IosConnectivityMapper` seam (wifi → ethernet → cellular → unknown), which is
unit tested in the `ios/ConnectivityCore` package.

## Automated Tests

The connection-type mapping has unit tests that run without a device or
simulator:

```sh
cd ios/ConnectivityCore
swift test
```

## Reference Links

| Item | Link |
| ---- | ---- |
| Tauri iOS prerequisites | <https://v2.tauri.app/start/prerequisites/> |
| Tauri mobile plugin development | <https://v2.tauri.app/develop/plugins/develop-mobile/> |
| `NWPathMonitor` API | <https://developer.apple.com/documentation/network/nwpathmonitor> |
| `NWPath` API | <https://developer.apple.com/documentation/network/nwpath> |
| `NWInterface.InterfaceType` API | <https://developer.apple.com/documentation/network/nwinterface/interfacetype> |

## Scenario Coverage

| Scenario | Status | Expected result |
| -------- | ------ | --------------- |
| Wi-Fi connected | Not tested | `connected: true`, `connectionType: "wifi"` |
| Cellular connected | Not tested | `connected: true`, `connectionType: "cellular"` |
| Airplane mode | Not tested | disconnected |
| Expensive (cellular/personal hotspot) network | Not tested | `metered: true` |
| Low Data Mode | Not tested | `constrained: true` |
| USB / Thunderbolt Ethernet | Not tested | `connectionType: "ethernet"` |
| Connectivity check immediately on launch | Not tested | first call reflects the real path, not an early `.requiresConnection` snapshot |

## Base Test Setup

Use a checkout of this repository with the iOS branch selected.

```sh
npm install
npm test
```

A physical device or the iOS Simulator can be used. The Simulator inherits the
Mac's network, so cellular-only scenarios require a physical device.

## Example App

Initialize iOS for the example app if `src-tauri/gen/apple` is not already
present:

```sh
cd examples/tauri-app
npm run tauri ios init
```

Run the example app:

```sh
cd examples/tauri-app
npm run tauri ios dev
```

For each scenario, press `Refresh status` in the example app and record the
`Raw response`.

## Manual Scenarios

### Wi-Fi Connected

1. Disable cellular data if the device allows it.
2. Connect to Wi-Fi.
3. Run or refresh the example app.

Expected response:

```json
{
   "connected": true,
   "metered": false,
   "constrained": false,
   "connectionType": "wifi"
}
```

### Cellular Connected

1. Disconnect Wi-Fi.
2. Enable cellular data.
3. Run or refresh the example app.

Expected response:

```json
{
   "connected": true,
   "metered": true,
   "constrained": false,
   "connectionType": "cellular"
}
```

`metered` reflects `NWPath.isExpensive`, which iOS sets for cellular and
Personal Hotspot connections.

### Low Data Mode

1. Open `Settings > Wi-Fi` (or `Settings > Cellular > Cellular Data Options`).
2. Enable `Low Data Mode` for the active network.
3. Run or refresh the example app.

Expected response:

```json
{
   "connected": true,
   "constrained": true
}
```

`constrained` reflects `NWPath.isConstrained`, which iOS sets when Low Data Mode
is active.

### Airplane Mode

1. Enable airplane mode.
2. Make sure Wi-Fi and cellular data are off.
3. Run or refresh the example app.

Expected response:

```json
{
   "connected": false,
   "metered": false,
   "constrained": false,
   "connectionType": "unknown"
}
```

### Connectivity Check Immediately On Launch

1. Fully quit the example app.
2. Relaunch and read the connection status as early as possible.
3. Confirm an online device reports `connected: true` on the first call.

`NWPathMonitor` delivers its first path update asynchronously after `start()`.
The first `connectionStatus()` call briefly waits (bounded, 200 ms) for that
update so it does not under-report connectivity from an early
`.requiresConnection` snapshot of `monitor.currentPath`.
