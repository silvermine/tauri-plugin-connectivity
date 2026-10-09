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
| Android `PackageManager` API | <https://developer.android.com/reference/android/content/pm/PackageManager> |
| `ConnectivityManager` API | <https://developer.android.com/reference/android/net/ConnectivityManager> |
| `NetworkCapabilities` API | <https://developer.android.com/reference/android/net/NetworkCapabilities> |
| Android 17 behaviour changes | <https://developer.android.com/about/versions/17/behavior-changes-17> |

## Toolchain

| Component | Minimum for this plugin |
| --------- | ----------------------- |
| JDK | 17 |
| Gradle (standalone `android/`) | 9.6.1 (wrapper) |
| AGP | 9.3.1 (standalone `settings.gradle`) |
| Kotlin Gradle Plugin | 2.2.10 default in `android/settings.gradle` (override with `-PkotlinVersion=…`; not matrix-tested in CI) |
| `compileSdk` | 37 |
| Host app Gradle (Tauri 2.12 template) | 8.13+ |

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
| Supported transport classes | Tested by terminal inspection | `ConnectionType[]` without `unknown` |
| Android 17 (API 37) device or emulator | Not tested in CI | Same fields as API 36; re-run Wi-Fi, cellular, Data Saver, metered |

## Pre-release checklist (0.2.0)

Before publishing **0.2.0**, run the **Android 17 (API 37)** scenario on a device or
emulator and set the scenario table **Status** to `Tested` when complete. CI compiles
with `compileSdk` 37 but does not execute on-device checks.

## Android 17 / SDK 37 review

Reviewed `Connectivity.kt` against the Android 17 behaviour-change list. No
plugin code changes were required: connectivity still uses
`ConnectivityManager.activeNetwork`, `getNetworkCapabilities`, and
`PackageManager` system features. `ConnectivityManager.allNetworks` remains
available but is deprecated at compile time with SDK 37; behaviour is unchanged
for supplementing supported transports. `restrictBackgroundStatus` and
`NET_CAPABILITY_TEMPORARILY_NOT_METERED` are unchanged on API 37.

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
adb shell pm list features
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

## Supported Connection Types

The `supportedConnectionTypes()` API reports transport classes the device can
use. Android does not expose a complete public SDK inventory of inactive
removable adapters, so the backend combines system features from
`PackageManager` with currently tracked `ConnectivityManager` networks.

### Terminal Feature Check

Run:

```sh
adb shell pm list features | grep -E \
   'android.hardware.wifi|android.hardware.ethernet|android.hardware.telephony'
```

Expected mapping:

| Terminal feature | Expected API item |
| ---------------- | ----------------- |
| `android.hardware.wifi` | `"wifi"` |
| `android.hardware.ethernet` | `"ethernet"` |
| `android.hardware.telephony` | `"cellular"` |

Most phones should include Wi-Fi and telephony:

```json
["wifi", "cellular"]
```

An Android TV or tablet with Ethernet hardware can include Ethernet:

```json
["wifi", "ethernet"]
```

### Terminal Current-Network Check

Inspect Android's currently tracked networks:

```sh
adb shell dumpsys connectivity | grep -E \
   'NetworkAgentInfo|Transports:|Capabilities:'
```

Look for transport labels such as `WIFI`, `ETHERNET`, or `CELLULAR`. These can
add a transport class when Android exposes an attached adapter as a current
network even if that transport is not listed as a system feature.

### End-To-End Example App Check

Run the example app and press the **Refresh** button:

```sh
cd examples/tauri-app
npm run tauri android dev
```

The raw response should contain a deduplicated `supportedConnectionTypes` array
without `"unknown"`, ordered as Wi-Fi, Ethernet, Cellular.

Expected phone example:

```json
{
   "status": {
      "connected": true,
      "metered": false,
      "constrained": false,
      "connectionType": "wifi"
   },
   "supportedConnectionTypes": ["wifi", "cellular"]
}
```

Expected phone with USB-C Ethernet attached and visible to Android:

```json
{
   "status": {
      "connected": true,
      "metered": false,
      "constrained": false,
      "connectionType": "ethernet"
   },
   "supportedConnectionTypes": ["wifi", "ethernet", "cellular"]
}
```

If Android does not expose an inactive removable adapter through system features
or current networks, it will not appear until Android reports it.

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
