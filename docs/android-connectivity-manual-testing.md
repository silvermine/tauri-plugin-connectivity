# Android Connectivity Manual Testing

Manual Android scenarios for exercising the native Android backend under
`android/src/main/java`.

The Android implementation uses `ConnectivityManager.activeNetwork` and
`NetworkCapabilities` to read the current default network. It reports connected
when the active network has `NET_CAPABILITY_INTERNET`; networks without
`NET_CAPABILITY_VALIDATED`, such as captive portals, are reported as constrained.

## Reference Links

| Item | Link |
| ---- | ---- |
| Tauri Android prerequisites | <https://v2.tauri.app/start/prerequisites/> |
| Tauri mobile plugin development | <https://v2.tauri.app/develop/plugins/develop-mobile/> |
| Android network state guide | <https://developer.android.com/develop/connectivity/network-ops/reading-network-state> |
| `ConnectivityManager` API | <https://developer.android.com/reference/android/net/ConnectivityManager> |
| `NetworkCapabilities` API | <https://developer.android.com/reference/android/net/NetworkCapabilities> |

## Scenario Coverage

| Scenario | Status | Expected result |
| -------- | ------ | --------------- |
| Wi-Fi connected | Tested | `connected: true`, `connectionType: "wifi"` |
| Cellular connected | Tested | `connected: true`, `connectionType: "cellular"` |
| Airplane mode | Tested | disconnected |
| Captive portal / unvalidated network | Not tested | `connected: true`, `constrained: true` |
| Data Saver on metered network | Tested | `metered: true`, `constrained: true` |
| Data Saver on unmetered network | Not tested | `metered: false`, `constrained: false` |
| Metered Wi-Fi | Tested | `metered: true` if Android marks the network metered |
| Temporarily not metered network | Not tested | `metered: false` while capability is present |
| USB-C Ethernet | Not tested | `connectionType: "ethernet"` |

## Base Test Setup

Use a checkout of this repository with the Android branch selected.

```sh
npm install
npm test
```

Confirm that `adb` can see the test device:

```sh
adb devices
```

The device must appear with the `device` state before running the example app.

```text
List of devices attached
90859562    device
```

## Example App

Initialize Android for the example app if `src-tauri/gen/android` is not already
present:

```sh
cd examples/tauri-app
npm run tauri android init
```

Run the example app on the connected device:

```sh
cd examples/tauri-app
npm run tauri android dev
```

For each scenario, press `Refresh status` in the example app and record the
`Raw response`.

## Useful Observation Commands

Use these commands while changing network state:

```sh
adb devices
adb shell dumpsys connectivity
adb shell cmd netpolicy set restrict-background true
adb shell cmd netpolicy get restrict-background
adb logcat -s ConnectivityPlugin RustStdoutStderr Tauri
```

To toggle Data Saver from the Android UI, open:

```text
Settings > Network & internet > Data Saver
```

The exact Settings path can vary by Android vendor.

For a deterministic Data Saver test, set Android's global restrict-background
policy directly with `adb`:

```sh
adb shell cmd netpolicy set restrict-background true
adb shell cmd netpolicy get restrict-background
```

The expected policy output is:

```text
Restrict background status: enabled
```

Reset the policy after testing:

```sh
adb shell cmd netpolicy set restrict-background false
```

## Manual Scenarios

### Wi-Fi Connected

1. Disable mobile data if the device allows it.
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

If Android marks the Wi-Fi network as metered, `metered` can be `true`.

### Captive Portal Or Unvalidated Network

1. Connect to a Wi-Fi network with a captive portal.
2. Do not complete the captive portal sign-in.
3. Run or refresh the example app.

Expected response:

```json
{
   "connected": true,
   "constrained": true,
   "connectionType": "wifi"
}
```

The `metered` field should continue to reflect Android's active network
capabilities.

### Cellular Connected

1. Disconnect Wi-Fi.
2. Enable mobile data.
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

Some carriers or temporary promotions can expose cellular as temporarily
unmetered. In that case, `metered` can be `false`.

### Airplane Mode

1. Enable airplane mode.
2. Make sure Wi-Fi and mobile data are off.
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

### Data Saver On Metered Network

1. Connect to cellular or a metered Wi-Fi network.
2. Enable Data Saver.
3. Run or refresh the example app.

Expected response:

```json
{
   "connected": true,
   "metered": true,
   "constrained": true
}
```

The other fields should continue to reflect the active network.

If Data Saver is enabled while the active network is unmetered, Android should
not restrict that network and the expected response is:

```json
{
   "connected": true,
   "metered": false,
   "constrained": false
}
```
