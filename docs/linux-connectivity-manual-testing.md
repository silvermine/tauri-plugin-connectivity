# Linux Connectivity Manual Testing

Manual Linux scenarios for exercising `src/platform/linux.rs`.

The Linux implementation has two runtime paths:

   * NetworkManager over system D-Bus. This is the preferred path when
     `org.freedesktop.NetworkManager` owns its bus name. For cellular devices,
     this path may also query ModemManager for roaming state.
   * Passive fallback using `/proc/net/route` and `/sys/class/net`. This path is
     used when the system bus is unavailable, NetworkManager is not running, or a
     NetworkManager query fails.

## Reference Links

| Item | Link |
| ---- | ---- |
| Tauri Linux prerequisites | <https://v2.tauri.app/start/prerequisites/> |
| VirtualBox networking manual | <https://www.virtualbox.org/manual/ch06.html> |
| NetworkManager `nmcli` manual | <https://networkmanager.pages.freedesktop.org/NetworkManager/NetworkManager/nmcli.html> |
| NetworkManager config manual | <https://networkmanager.pages.freedesktop.org/NetworkManager/NetworkManager/NetworkManager.conf.html> |
| NetworkManager D-Bus types | <https://networkmanager.pages.freedesktop.org/NetworkManager/NetworkManager/nm-dbus-types.html> |
| ModemManager 3GPP D-Bus interface | <https://www.freedesktop.org/software/ModemManager/api/latest/gdbus-org.freedesktop.ModemManager1.Modem.Modem3gpp.html> |
| WebKitGTK environment variables | <https://trac.webkit.org/wiki/EnvironmentVariables> |

## Scenario Coverage

All scenarios below were manually tested except the ModemManager scenarios.

| Scenario | Environment | Status | Expected result |
| -------- | ----------- | ------ | --------------- |
| Passive fallback connected | WSL2 or VM without NetworkManager | Tested | `connected: true`, `ethernet` |
| Passive fallback disconnected | WSL2 or VM without default route | Tested | disconnected |
| NetworkManager full connectivity | VirtualBox NAT or bridged adapter | Tested | `connected: true`, `ethernet` |
| NetworkManager metered | VirtualBox NAT or bridged adapter | Tested | `metered: true`, `constrained: true` |
| NetworkManager disabled | VirtualBox VM | Tested | disconnected |
| Virtual cable disconnected | VirtualBox VM | Tested | disconnected |
| Local-only `none` or `limited` | VirtualBox host-only adapter | Tested | disconnected |
| Captive portal | NetworkManager fake portal check | Tested | `connected: true`, `constrained: true` |
| Connectivity `unknown` fallback | NetworkManager config override | Tested | falls back to `State` |
| Wi-Fi unmetered and metered | Physical Wi-Fi or USB Wi-Fi pass-through | Tested | `connectionType: "wifi"` |
| Unknown transport | VM or uncommon primary interface | Tested | `connectionType: "unknown"` |
| Cellular modem | ModemManager and WWAN hardware | Not tested | `connectionType: "cellular"` |
| Cellular roaming | ModemManager and roaming SIM/network | Not tested | `constrained: true` |

VirtualBox NAT, bridged, host-only, and internal networks normally appear in the
guest as Ethernet. Use physical hardware or USB pass-through for Wi-Fi and WWAN
transport tests.

On Linux, `metered: true` always implies `constrained: true` because constrained
is derived as:

```text
constrained = network_manager_connectivity_is_portal
   || network_manager_device_is_metered
   || modem_manager_reports_roaming
```

The passive fallback always reports `metered: false` and `constrained: false`.

## Base Test Setup

Use one checkout of this repository in each Linux environment.

```sh
git clone https://github.com/silvermine/tauri-plugin-connectivity.git
cd tauri-plugin-connectivity
npm install
npm run build
```

Install the Linux system packages from the official Tauri prerequisites page for
the distribution under test. For Ubuntu or Debian based guests:

```sh
sudo apt update
sudo apt install -y \
   build-essential \
   curl \
   file \
   libayatana-appindicator3-dev \
   librsvg2-dev \
   libssl-dev \
   libwebkit2gtk-4.1-dev \
   libxdo-dev \
   network-manager \
   pkg-config \
   wget
```

Install Rust if the VM does not already have the toolchain:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
rustup show
```

Run the automated checks once before starting manual scenario work:

```sh
npm test
```

Run the example app:

```sh
cd examples/tauri-app
npm install
npm run dev
```

### Lubuntu Blank Window Workaround

If the Tauri window opens but the page is blank on Lubuntu, set
`WEBKIT_DISABLE_COMPOSITING_MODE=1`. Tauri uses WebKitGTK on Linux, and this
forces WebKitGTK to disable accelerated compositing for that process.

Test it for the current shell first:

```sh
cd examples/tauri-app
WEBKIT_DISABLE_COMPOSITING_MODE=1 npm run dev
```

If that fixes the blank page, add it globally on the test machine:

```sh
echo WEBKIT_DISABLE_COMPOSITING_MODE=1 | sudo tee -a /etc/environment
```

Then log out and back in, or reboot, and verify:

```sh
printenv WEBKIT_DISABLE_COMPOSITING_MODE
```

Use the global setting only for the Linux test environment. If another WebKitGTK
rendering issue still produces a blank page, also try this one-command variant:

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 npm run dev
```

For each manual scenario, press `Refresh status` in the example app and record
the `Raw response`.

The plugin reads NetworkManager's cached `Connectivity` property. Run
`nmcli networking connectivity check` before refreshing the app when the scenario
changes connectivity.

## Useful Observation Commands

Run these commands before and after changing network state.

```sh
nmcli general status
nmcli networking connectivity
nmcli connection show --active
ip route
cat /proc/net/route
ls -l /sys/class/net
```

For the active NetworkManager profile and device:

```sh
ACTIVE="$(nmcli -t -f NAME,DEVICE connection show --active | \
   awk -F: '$2 != "lo" { print; exit }')"
PROFILE="${ACTIVE%%:*}"
IFACE="${ACTIVE##*:}"
DEVICE_PATH="$(nmcli -g GENERAL.DBUS-PATH device show "$IFACE")"

printf 'profile=%s\niface=%s\ndevice_path=%s\n' \
   "$PROFILE" "$IFACE" "$DEVICE_PATH"

nmcli -f GENERAL.DEVICE,GENERAL.TYPE,GENERAL.STATE,GENERAL.CONNECTION \
   device show "$IFACE"

nmcli -g connection.metered connection show "$PROFILE"

busctl get-property \
   org.freedesktop.NetworkManager \
   /org/freedesktop/NetworkManager \
   org.freedesktop.NetworkManager \
   Connectivity

busctl get-property \
   org.freedesktop.NetworkManager \
   /org/freedesktop/NetworkManager \
   org.freedesktop.NetworkManager \
   State

busctl get-property \
   org.freedesktop.NetworkManager \
   "$DEVICE_PATH" \
   org.freedesktop.NetworkManager.Device \
   DeviceType

busctl get-property \
   org.freedesktop.NetworkManager \
   "$DEVICE_PATH" \
   org.freedesktop.NetworkManager.Device \
   Metered
```

Important enum values used by the plugin:

| Source | Value | Meaning in plugin |
| ------ | ----- | ----------------- |
| NetworkManager `Connectivity` | `4` | Connected |
| NetworkManager `Connectivity` | `2` | Connected and constrained |
| NetworkManager `Connectivity` | `1`, `3` | Disconnected |
| NetworkManager `Connectivity` | `0` | Fall back to `State` |
| NetworkManager `State` | `70` | Connected if connectivity is unknown |
| NetworkManager `DeviceType` | `1` | Ethernet |
| NetworkManager `DeviceType` | `2` | Wi-Fi |
| NetworkManager `DeviceType` | `8` | Cellular modem |
| NetworkManager `Metered` | `1`, `3` | Metered |
| NetworkManager `Metered` | `0`, `2`, `4` | Not metered |
| ModemManager `RegistrationState` | `5` | Roaming |

## WSL2 Fallback Scenarios

Install Ubuntu or another WSL distribution:

```powershell
wsl --install -d Ubuntu
wsl --set-version Ubuntu 2
```

Inside WSL2, verify the fallback path:

```sh
systemctl is-active NetworkManager || true
busctl --system list | grep NetworkManager || true
ip route
cat /proc/net/route
```

Expected connected fallback response when a non-loopback default route exists:

```json
{
   "connected": true,
   "metered": false,
   "constrained": false,
   "connectionType": "ethernet"
}
```

To test fallback disconnected, delete the default route temporarily:

```sh
DEFAULT_ROUTE="$(ip route show default | head -n 1)"
sudo ip route del default
ip route
```

Expected response:

```json
{
   "connected": false,
   "metered": false,
   "constrained": false,
   "connectionType": "unknown"
}
```

Restore the WSL network by restarting the distribution from PowerShell:

```powershell
wsl --shutdown
wsl -d Ubuntu
```

## VirtualBox VM Setups

Use snapshots before changing network state.

Create at least two VMs:

| VM | Suggested distro | Purpose |
| -- | ---------------- | ------- |
| `nm-vm` | Ubuntu Desktop or Fedora Workstation | NetworkManager D-Bus branch |
| `fallback-vm` | Ubuntu Server, Debian, or Ubuntu Desktop | Passive fallback branch |
| `hardware-vm` | Ubuntu Desktop or Fedora Workstation | USB Wi-Fi or USB modem tests |

Start `nm-vm` with a single VirtualBox network adapter:

| VirtualBox adapter mode | Useful coverage |
| ----------------------- | --------------- |
| NAT | Connected Ethernet with internet |
| Bridged Adapter | Connected Ethernet with LAN behavior closer to hardware |
| Host-only Adapter | Local network without internet |
| Internal Network | Isolated guest network |
| Cable connected unchecked | Disconnected |

The plugin sees the virtual NIC as Ethernet for all of these modes.

## NetworkManager Ethernet Scenarios

Use `nm-vm` with a NAT or bridged adapter. Confirm NetworkManager owns the bus:

```sh
busctl --system list | grep org.freedesktop.NetworkManager
nmcli networking on
nmcli networking connectivity check
```

### Ethernet Unmetered

```sh
ACTIVE="$(nmcli -t -f NAME,DEVICE connection show --active | \
   awk -F: '$2 != "lo" { print; exit }')"
PROFILE="${ACTIVE%%:*}"
IFACE="${ACTIVE##*:}"

sudo nmcli connection modify "$PROFILE" connection.metered no
sudo nmcli connection down "$PROFILE"
sudo nmcli connection up "$PROFILE"
nmcli networking connectivity check
```

Expected response:

```json
{
   "connected": true,
   "metered": false,
   "constrained": false,
   "connectionType": "ethernet"
}
```

### Ethernet Metered

```sh
sudo nmcli connection modify "$PROFILE" connection.metered yes
sudo nmcli connection down "$PROFILE"
sudo nmcli connection up "$PROFILE"
nmcli networking connectivity check
```

Expected response:

```json
{
   "connected": true,
   "metered": true,
   "constrained": true,
   "connectionType": "ethernet"
}
```

Reset after the test:

```sh
sudo nmcli connection modify "$PROFILE" connection.metered no
sudo nmcli connection down "$PROFILE"
sudo nmcli connection up "$PROFILE"
```

### Ethernet Unknown Or Guessed Metered

NetworkManager can guess a device's runtime metered state when the profile is
`unknown`. The plugin treats NetworkManager `Metered` value `3` as metered.

```sh
sudo nmcli connection modify "$PROFILE" connection.metered unknown
sudo nmcli connection down "$PROFILE"
sudo nmcli connection up "$PROFILE"
```

Verify the runtime property:

```sh
DEVICE_PATH="$(nmcli -g GENERAL.DBUS-PATH device show "$IFACE")"
busctl get-property \
   org.freedesktop.NetworkManager \
   "$DEVICE_PATH" \
   org.freedesktop.NetworkManager.Device \
   Metered
```

If the value is `u 3`, expect the metered response. If the value is `u 4` or
`u 2`, expect the unmetered response.

## NetworkManager Disconnected Scenarios

### Networking Off

```sh
sudo nmcli networking off
nmcli networking connectivity
```

Expected response:

```json
{
   "connected": false,
   "metered": false,
   "constrained": false,
   "connectionType": "unknown"
}
```

Restore:

```sh
sudo nmcli networking on
sudo nmcli connection up "$PROFILE"
nmcli networking connectivity check
```

### Virtual Cable Disconnected

In VirtualBox, open the VM network settings and uncheck `Cable connected`.
Refresh the app.

Expected response:

```json
{
   "connected": false,
   "metered": false,
   "constrained": false,
   "connectionType": "unknown"
}
```

Reconnect the cable and run:

```sh
sudo nmcli connection up "$PROFILE"
nmcli networking connectivity check
```

### Limited, Local-Only, Or Captive Connectivity

Use a Host-only Adapter for local-only VirtualBox networking.

1. Shut down the VM.
2. In VirtualBox, open `Tools` > `Network Manager` > `Host-only Networks`.
3. Create a host-only network if one does not already exist. The default name is
   usually `vboxnet0`.
4. Keep DHCP enabled for the host-only network.
5. Open the VM settings.
6. Go to `Network` > `Adapter 1`.
7. Set `Attached to` to `Host-only Adapter`.
8. Set `Name` to the host-only network, for example `vboxnet0`.
9. Keep `Cable connected` checked.
10. Start the VM.

After the VM boots, reconnect NetworkManager and capture the active profile:

```sh
sudo nmcli networking on

ACTIVE="$(nmcli -t -f NAME,DEVICE connection show --active | \
   awk -F: '$2 != "lo" { print; exit }')"
PROFILE="${ACTIVE%%:*}"
IFACE="${ACTIVE##*:}"

sudo nmcli connection up "$PROFILE"
```

Verify that the guest has local network access but not internet access:

```sh
ip addr show "$IFACE"
ip route
ping -c 1 192.168.56.1 || true
ping -c 1 1.1.1.1 || true
```

The host-only network commonly uses `192.168.56.1` for the host side. Use the
address shown by VirtualBox if it differs.

Then refresh NetworkManager's cached connectivity state:

```sh
nmcli networking connectivity check
nmcli networking connectivity
```

If NetworkManager reports `none` or `limited`, the plugin should return
disconnected:

```json
{
   "connected": false,
   "metered": false,
   "constrained": false,
   "connectionType": "unknown"
}
```

To cover captive portal behavior, use a real captive portal network or configure
a temporary NetworkManager connectivity check in a test VM. NetworkManager
reports `portal` when its check URI is reachable, but the response does not
match the configured online response.

Create a fake portal check:

```sh
sudo tee /etc/NetworkManager/conf.d/99-connectivity-portal-test.conf >/dev/null <<'EOF'
[connectivity]
enabled=true
uri=http://example.com/
response=NetworkManager is online
interval=5
EOF

sudo systemctl restart NetworkManager
```

Force a connectivity check:

```sh
nmcli networking connectivity check
nmcli networking connectivity
```

Expected `nmcli` output:

```text
portal
```

Confirm the D-Bus value read by the plugin:

```sh
busctl get-property \
   org.freedesktop.NetworkManager \
   /org/freedesktop/NetworkManager \
   org.freedesktop.NetworkManager \
   Connectivity
```

Expected D-Bus output:

```text
u 2
```

Run the example app and refresh the status:

```sh
cd examples/tauri-app
WEBKIT_DISABLE_COMPOSITING_MODE=1 npm run dev
```

If NetworkManager reports `portal`, the plugin should return connected and
constrained:

```json
{
   "connected": true,
   "metered": false,
   "constrained": true,
   "connectionType": "ethernet"
}
```

The `connectionType` may be `wifi` when testing on a real Wi-Fi captive portal.

Remove the temporary portal check when the scenario is complete:

```sh
sudo rm /etc/NetworkManager/conf.d/99-connectivity-portal-test.conf
sudo systemctl restart NetworkManager
nmcli networking connectivity check
```

Restore the VM's normal internet path by shutting it down and changing
`Adapter 1` back to `NAT` or `Bridged Adapter`.

## NetworkManager Unknown Connectivity

This scenario covers the branch where `Connectivity` is `unknown` and the plugin
falls back to NetworkManager `State`.

Create a temporary NetworkManager config:

```sh
sudo mkdir -p /etc/NetworkManager/conf.d
printf '%s\n' \
   '[connectivity]' \
   'enabled=false' \
   | sudo tee /etc/NetworkManager/conf.d/99-connectivity-test.conf

sudo systemctl restart NetworkManager
```

Check the D-Bus values:

```sh
busctl get-property \
   org.freedesktop.NetworkManager \
   /org/freedesktop/NetworkManager \
   org.freedesktop.NetworkManager \
   Connectivity

busctl get-property \
   org.freedesktop.NetworkManager \
   /org/freedesktop/NetworkManager \
   org.freedesktop.NetworkManager \
   State
```

If `Connectivity` is `u 0` and `State` is `u 70`, expected response is connected
with the active device details. If `State` is not `u 70`, expected response is
disconnected.

Remove the temporary config after the scenario:

```sh
sudo rm /etc/NetworkManager/conf.d/99-connectivity-test.conf
sudo systemctl restart NetworkManager
nmcli networking connectivity check
```

## Passive Fallback In A VM

Use `fallback-vm`. The simplest way to force fallback is to stop
NetworkManager while keeping the current interface and route in place.

```sh
systemctl is-active NetworkManager || true
ip route

sudo systemctl stop NetworkManager
busctl --system list | grep org.freedesktop.NetworkManager || true
ip route
```

If the default route is still present, expected response is:

```json
{
   "connected": true,
   "metered": false,
   "constrained": false,
   "connectionType": "ethernet"
}
```

If stopping NetworkManager removes the route, re-add a temporary route inside
the VM. Replace the interface and gateway with values from `ip route` before the
service was stopped.

```sh
sudo ip link set "$IFACE" up
sudo ip route add default via "$GATEWAY" dev "$IFACE"
```

To cover fallback disconnected:

```sh
sudo ip route del default
```

Expected response:

```json
{
   "connected": false,
   "metered": false,
   "constrained": false,
   "connectionType": "unknown"
}
```

Restore:

```sh
sudo systemctl start NetworkManager
sudo nmcli networking on
sudo nmcli connection up "$PROFILE"
```

## Wi-Fi Scenarios

Use a physical Linux machine with Wi-Fi or pass a USB Wi-Fi adapter through to a
VirtualBox VM.

Install Wi-Fi tooling if needed:

```sh
sudo apt install -y network-manager iw wireless-tools
```

Connect using NetworkManager:

```sh
nmcli device wifi list
sudo nmcli device wifi connect "$SSID" password "$PASSWORD"
nmcli connection show --active
```

Set the Wi-Fi profile to unmetered:

```sh
ACTIVE="$(nmcli -t -f NAME,DEVICE connection show --active | \
   awk -F: '$2 != "lo" { print; exit }')"
PROFILE="${ACTIVE%%:*}"

sudo nmcli connection modify "$PROFILE" connection.metered no
sudo nmcli connection down "$PROFILE"
sudo nmcli connection up "$PROFILE"
nmcli networking connectivity check
```

Expected unmetered response:

```json
{
   "connected": true,
   "metered": false,
   "constrained": false,
   "connectionType": "wifi"
}
```

Set the Wi-Fi profile to metered:

```sh
sudo nmcli connection modify "$PROFILE" connection.metered yes
sudo nmcli connection down "$PROFILE"
sudo nmcli connection up "$PROFILE"
nmcli networking connectivity check
```

Expected metered response:

```json
{
   "connected": true,
   "metered": true,
   "constrained": true,
   "connectionType": "wifi"
}
```

To test Wi-Fi fallback classification, stop NetworkManager after the Wi-Fi
connection is active and make sure the default route remains. The passive
fallback checks `/sys/class/net/$IFACE/wireless` or an `80211` marker.

## Cellular And ModemManager Scenarios

These scenarios were not manually tested in this pass.

Use a physical Linux machine with a WWAN modem or pass a USB modem through to a
VirtualBox VM. Install ModemManager and enable WWAN:

```sh
sudo apt install -y modemmanager network-manager usbutils
sudo systemctl enable --now ModemManager NetworkManager
sudo nmcli radio wwan on
mmcli -L
```

Create a GSM profile. Replace `$APN` and add username, password, or SIM PIN
settings if the carrier requires them.

```sh
sudo nmcli connection add \
   type gsm \
   ifname "*" \
   con-name test-cellular \
   apn "$APN"

sudo nmcli connection up test-cellular
nmcli connection show --active
```

Expected unmetered, non-roaming response when the profile is not metered:

```sh
sudo nmcli connection modify test-cellular connection.metered no
sudo nmcli connection down test-cellular
sudo nmcli connection up test-cellular
```

```json
{
   "connected": true,
   "metered": false,
   "constrained": false,
   "connectionType": "cellular"
}
```

Expected metered response:

```sh
sudo nmcli connection modify test-cellular connection.metered yes
sudo nmcli connection down test-cellular
sudo nmcli connection up test-cellular
```

```json
{
   "connected": true,
   "metered": true,
   "constrained": true,
   "connectionType": "cellular"
}
```

To cover the ModemManager roaming branch, the modem must expose a roaming 3GPP
registration state.

Find the modem object path:

```sh
mmcli -L
```

Then verify the registration state:

```sh
busctl get-property \
   org.freedesktop.ModemManager1 \
   /org/freedesktop/ModemManager1/Modem/0 \
   org.freedesktop.ModemManager1.Modem.Modem3gpp \
   RegistrationState
```

When that value is `u 5`, expected response with `connection.metered no` is:

```json
{
   "connected": true,
   "metered": false,
   "constrained": true,
   "connectionType": "cellular"
}
```

## Unknown Connection Type Scenarios

Unknown type can happen when NetworkManager reports an unsupported device type,
when no primary connection is available, or when the passive fallback finds a
default route but no Wi-Fi, WWAN, or Ethernet marker in sysfs.

Practical options:

   * Connect through a VPN, tunnel, or uncommon virtual device and make it the
     primary route.
   * Use an isolated test VM with a custom interface managed outside
     NetworkManager.
   * Record any real environment where the observation commands show a default
     route but the plugin returns `connectionType: "unknown"`.

Expected connected unknown response:

```json
{
   "connected": true,
   "metered": false,
   "constrained": false,
   "connectionType": "unknown"
}
```

If NetworkManager is active and the unknown device is metered, then constrained
should also be true:

```json
{
   "connected": true,
   "metered": true,
   "constrained": true,
   "connectionType": "unknown"
}
```
