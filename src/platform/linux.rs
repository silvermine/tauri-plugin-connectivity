use std::fs;
use std::path::Path;
use std::time::Duration;

use tracing::{debug, warn};
use zbus::blocking::connection::Builder as ConnectionBuilder;
use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::proxy::Builder as ProxyBuilder;
use zbus::blocking::{Connection, Proxy};
use zbus::names::BusName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

use crate::error::Result;
use crate::types::{ConnectionStatus, ConnectionType, ConnectionTypes};

// These local D-Bus calls read cached service state and normally complete within
// milliseconds. Bound each call so a stalled service cannot occupy the blocking
// thread for long before the query uses its fallback. This does not bound the
// initial socket connect/Hello handshake or the total Linux status query.
const DBUS_METHOD_TIMEOUT: Duration = Duration::from_millis(500);

const DBUS_SERVICE: &str = "org.freedesktop.DBus";

// NetworkManager exposes cached root properties for connection state. We read
// `Connectivity` instead of calling `CheckConnectivity()` because that method
// can issue a connectivity probe.
// https://networkmanager.pages.freedesktop.org/NetworkManager/NetworkManager/gdbus-org.freedesktop.NetworkManager.html
const NETWORK_MANAGER_SERVICE: &str = "org.freedesktop.NetworkManager";
const NETWORK_MANAGER_PATH: &str = "/org/freedesktop/NetworkManager";
const NETWORK_MANAGER_INTERFACE: &str = "org.freedesktop.NetworkManager";

// The primary active connection points at the NetworkManager devices that carry
// it; device properties provide transport, metered state, and the ModemManager
// object path for modem devices.
// https://networkmanager.pages.freedesktop.org/NetworkManager/NetworkManager/gdbus-org.freedesktop.NetworkManager.Connection.Active.html
// https://www.networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.Device.html
const NETWORK_MANAGER_ACTIVE_CONNECTION_INTERFACE: &str =
   "org.freedesktop.NetworkManager.Connection.Active";
const NETWORK_MANAGER_DEVICE_INTERFACE: &str = "org.freedesktop.NetworkManager.Device";

// ModemManager is only used for cellular roaming. Missing service, missing 3GPP
// interface, and read errors are treated as no roaming signal.
// https://www.freedesktop.org/software/ModemManager/api/latest/gdbus-org.freedesktop.ModemManager1.Modem.Modem3gpp.html
const MODEM_MANAGER_SERVICE: &str = "org.freedesktop.ModemManager1";
const MODEM_MANAGER_MODEM_PREFIX: &str = "/org/freedesktop/ModemManager1/Modem/";
const MODEM_MANAGER_3GPP_INTERFACE: &str = "org.freedesktop.ModemManager1.Modem.Modem3gpp";

// NetworkManager D-Bus enum values
// https://networkmanager.pages.freedesktop.org/NetworkManager/NetworkManager/nm-dbus-types.html
const NM_CONNECTIVITY_NONE: u32 = 1;
const NM_CONNECTIVITY_PORTAL: u32 = 2;
const NM_CONNECTIVITY_LIMITED: u32 = 3;
const NM_CONNECTIVITY_FULL: u32 = 4;

const NM_STATE_CONNECTED_GLOBAL: u32 = 70;

const NM_DEVICE_TYPE_ETHERNET: u32 = 1;
const NM_DEVICE_TYPE_WIFI: u32 = 2;
const NM_DEVICE_TYPE_MODEM: u32 = 8;

const NM_METERED_YES: u32 = 1;
const NM_METERED_GUESS_YES: u32 = 3;

const MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING: u32 = 5;

// Passive fallback inputs. This path intentionally avoids DNS, ping, HTTP, or
// any other active reachability probe.
const PROC_NET_ROUTE: &str = "/proc/net/route";
const PROC_NET_IPV6_ROUTE: &str = "/proc/net/ipv6_route";
const SYS_CLASS_NET: &str = "/sys/class/net";
const LINUX_ARPHRD_ETHER: u32 = 1;
const LINUX_ROUTE_FLAG_UP: u32 = 0x1;
const IPV4_DEFAULT_DESTINATION: &str = "00000000";
const IPV6_DEFAULT_DESTINATION: &str = "00000000000000000000000000000000";
const IPV6_DEFAULT_PREFIX_LEN: &str = "00";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectedState {
   Connected,
   Constrained,
   Disconnected,
   Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectionDetails {
   metered: bool,
   roaming: bool,
   connection_type: ConnectionType,
}

impl Default for ConnectionDetails {
   fn default() -> Self {
      Self {
         metered: false,
         roaming: false,
         connection_type: ConnectionType::Unknown,
      }
   }
}

impl ConnectionDetails {
   fn metered_unknown() -> Self {
      Self {
         metered: true,
         ..Self::default()
      }
   }
}

/// Returns the current Linux network connection status.
///
/// NetworkManager is preferred when available because it exposes cached
/// connectivity, primary-route, transport, and metered state over D-Bus. Systems
/// without NetworkManager fall back to passive kernel state only.
pub fn connection_status() -> Result<ConnectionStatus> {
   debug!("querying Linux connection status");

   let connection = match system_bus_connection() {
      Ok(connection) => {
         debug!("connected to Linux system D-Bus");
         connection
      }
      Err(error) => {
         warn!(%error, "failed to connect to Linux system bus; using passive fallback");
         return Ok(fallback_connection_status());
      }
   };

   match service_has_owner(&connection, NETWORK_MANAGER_SERVICE) {
      Ok(true) => {
         debug!("NetworkManager service is present");

         match network_manager_connection_status(&connection) {
            Ok(status) => {
               debug!(
                  ?status,
                  "resolved Linux connection status via NetworkManager"
               );
               Ok(status)
            }
            Err(error) => {
               warn!(%error, "failed to query NetworkManager; using passive fallback");
               Ok(fallback_connection_status())
            }
         }
      }
      Ok(false) => {
         debug!("NetworkManager service is not present; using passive fallback");
         Ok(fallback_connection_status())
      }
      Err(error) => {
         warn!(%error, "failed to probe NetworkManager service; using passive fallback");
         Ok(fallback_connection_status())
      }
   }
}

/// Returns the supported physical connection transport classes.
pub fn supported_connection_types() -> Result<Vec<ConnectionType>> {
   debug!("querying Linux supported connection types");

   // Prefer NetworkManager's realized `Devices` list. Its D-Bus docs describe
   // `Devices` as the network devices currently known to NetworkManager, while
   // `AllDevices` can include placeholders that do not correspond to real,
   // present hardware:
   // https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.html
   let connection = match system_bus_connection() {
      Ok(connection) => connection,
      Err(error) => {
         warn!(%error, "failed to connect to Linux system bus; using sysfs fallback");
         return Ok(supported_types_from_sysfs(Path::new(SYS_CLASS_NET)));
      }
   };

   match service_has_owner(&connection, NETWORK_MANAGER_SERVICE) {
      Ok(true) => match network_manager_supported_connection_types(&connection) {
         Ok(connection_types) => Ok(connection_types),
         Err(error) => {
            warn!(%error, "failed to query NetworkManager devices; using sysfs fallback");
            Ok(supported_types_from_sysfs(Path::new(SYS_CLASS_NET)))
         }
      },
      Ok(false) => Ok(supported_types_from_sysfs(Path::new(SYS_CLASS_NET))),
      Err(error) => {
         warn!(%error, "failed to probe NetworkManager service; using sysfs fallback");
         Ok(supported_types_from_sysfs(Path::new(SYS_CLASS_NET)))
      }
   }
}

fn network_manager_supported_connection_types(
   connection: &Connection,
) -> zbus::Result<Vec<ConnectionType>> {
   let manager = dbus_proxy(
      connection,
      NETWORK_MANAGER_SERVICE,
      NETWORK_MANAGER_PATH,
      NETWORK_MANAGER_INTERFACE,
   )?;
   let devices = manager.get_property::<Vec<OwnedObjectPath>>("Devices")?;

   Ok(collect_supported_connection_types_from_devices(
      devices,
      |device| {
         // DeviceType is the NetworkManager transport enum. Values used below are
         // from the NetworkManager D-Bus type reference:
         // https://networkmanager.pages.freedesktop.org/NetworkManager/NetworkManager/nm-dbus-types.html
         let device_proxy = dbus_proxy(
            connection,
            NETWORK_MANAGER_SERVICE,
            device.as_str(),
            NETWORK_MANAGER_DEVICE_INTERFACE,
         )?;
         device_proxy.get_property::<u32>("DeviceType")
      },
   ))
}

fn system_bus_connection() -> zbus::Result<Connection> {
   ConnectionBuilder::system()?
      .method_timeout(DBUS_METHOD_TIMEOUT)
      .build()
}

fn network_manager_connection_status(connection: &Connection) -> zbus::Result<ConnectionStatus> {
   let manager = dbus_proxy(
      connection,
      NETWORK_MANAGER_SERVICE,
      NETWORK_MANAGER_PATH,
      NETWORK_MANAGER_INTERFACE,
   )?;

   // `Connectivity` is a cached property. `FULL` maps to full connectivity,
   // `PORTAL` and `LIMITED` map to connected but constrained, and `UNKNOWN`
   // falls back to NM's broader networking state.
   let connectivity = manager.get_property::<u32>("Connectivity")?;
   debug!(connectivity, "queried NetworkManager connectivity state");

   let connectivity_state = map_connectivity(connectivity);
   let connected = match connectivity_state {
      ConnectedState::Connected => true,
      ConnectedState::Constrained => true,
      ConnectedState::Disconnected => false,
      ConnectedState::Unknown => {
         let state = manager.get_property::<u32>("State")?;
         debug!(
            connectivity,
            state, "NetworkManager connectivity is unknown; falling back to state"
         );
         has_global_connectivity(state)
      }
   };

   if !connected {
      debug!(
         connectivity,
         "NetworkManager connectivity does not indicate active internet access"
      );
      return Ok(ConnectionStatus::disconnected());
   }

   let details = match primary_connection_details(connection, &manager) {
      Ok(details) => details,
      Err(error) => {
         warn!(%error, "failed to resolve Linux primary connection details; treating connection as metered");
         ConnectionDetails::metered_unknown()
      }
   };

   Ok(ConnectionStatus {
      connected: true,
      metered: details.metered,
      constrained: is_constrained(connectivity_state, details.metered, details.roaming),
      connection_type: details.connection_type,
   })
}

fn primary_connection_details(
   connection: &Connection,
   manager: &Proxy<'_>,
) -> zbus::Result<ConnectionDetails> {
   // NetworkManager chooses the primary connection for the default route. Its
   // active connection object is the stable way to find the devices that should
   // drive transport and metered decisions.
   let primary_connection = manager.get_property::<OwnedObjectPath>("PrimaryConnection")?;
   debug!(
      primary_connection = %primary_connection.as_str(),
      "queried NetworkManager primary connection"
   );

   if is_root_path(&primary_connection) {
      warn!("NetworkManager returned no primary connection; treating connection as metered");
      return Ok(ConnectionDetails::metered_unknown());
   }

   let active_connection = dbus_proxy(
      connection,
      NETWORK_MANAGER_SERVICE,
      primary_connection.as_str(),
      NETWORK_MANAGER_ACTIVE_CONNECTION_INTERFACE,
   )?;
   let devices = active_connection.get_property::<Vec<OwnedObjectPath>>("Devices")?;
   debug!(
      device_count = devices.len(),
      primary_connection = %primary_connection.as_str(),
      "queried NetworkManager primary connection devices"
   );

   if devices.is_empty() {
      warn!("NetworkManager primary connection has no devices; treating connection as metered");
      return Ok(ConnectionDetails::metered_unknown());
   }

   let mut details = ConnectionDetails::default();
   let mut read_any_device = false;

   for device in devices {
      match device_details(connection, &device) {
         Ok(device_details) => {
            read_any_device = true;
            details.metered |= device_details.metered;
            details.roaming |= device_details.roaming;

            if details.connection_type == ConnectionType::Unknown {
               details.connection_type = device_details.connection_type;
            }

            debug!(
               device = %device.as_str(),
               metered = device_details.metered,
               roaming = device_details.roaming,
               connection_type = ?device_details.connection_type,
               "resolved NetworkManager device details"
            );
         }
         Err(error) => {
            warn!(%error, device = %device.as_str(), "failed to read NetworkManager device");
         }
      }
   }

   if !read_any_device {
      warn!(
         "failed to read any NetworkManager primary connection devices; treating connection as metered"
      );
      details.metered = true;
   }

   Ok(details)
}

fn device_details(
   connection: &Connection,
   device: &OwnedObjectPath,
) -> zbus::Result<ConnectionDetails> {
   let device_proxy = dbus_proxy(
      connection,
      NETWORK_MANAGER_SERVICE,
      device.as_str(),
      NETWORK_MANAGER_DEVICE_INTERFACE,
   )?;

   // DeviceType gives the transport class; Metered lives on the device, not on
   // the active connection.
   let device_type = device_proxy.get_property::<u32>("DeviceType")?;
   let connection_type = map_device_type(device_type);
   debug!(
      device = %device.as_str(),
      device_type,
      connection_type = ?connection_type,
      "queried NetworkManager device type"
   );

   let metered = match device_proxy.get_property::<u32>("Metered") {
      Ok(metered) => {
         let is_metered = is_metered(metered);
         debug!(
            device = %device.as_str(),
            metered,
            is_metered,
            "queried NetworkManager device metered state"
         );
         is_metered
      }
      Err(error) => {
         warn!(%error, device = %device.as_str(), "failed to read NetworkManager device metered state; treating device as metered");
         true
      }
   };
   let roaming = if device_type == NM_DEVICE_TYPE_MODEM {
      modem_is_roaming(connection, &device_proxy)
   } else {
      false
   };

   Ok(ConnectionDetails {
      metered,
      roaming,
      connection_type,
   })
}

fn modem_is_roaming(connection: &Connection, device_proxy: &Proxy<'_>) -> bool {
   // NM modem devices expose a `Udi` that usually points at the corresponding
   // ModemManager object. Only that object can tell us whether the cellular
   // registration state is roaming.
   match service_has_owner(connection, MODEM_MANAGER_SERVICE) {
      Ok(true) => {}
      Ok(false) => {
         debug!("ModemManager service is not present; skipping roaming check");
         return false;
      }
      Err(error) => {
         warn!(%error, "failed to probe ModemManager service; skipping roaming check");
         return false;
      }
   }

   let udi = match device_proxy.get_property::<String>("Udi") {
      Ok(udi) => {
         debug!(udi, "queried NetworkManager modem Udi");
         udi
      }
      Err(error) => {
         warn!(%error, "failed to read NetworkManager modem Udi; skipping roaming check");
         return false;
      }
   };

   if !is_modem_manager_modem_path(&udi) {
      debug!(
         udi,
         "NetworkManager modem Udi is not a ModemManager modem path"
      );
      return false;
   }

   let modem_path = match ObjectPath::try_from(udi.as_str()) {
      Ok(path) => path,
      Err(error) => {
         warn!(%error, udi, "NetworkManager modem Udi is not a valid D-Bus object path");
         return false;
      }
   };

   let modem = match dbus_proxy(
      connection,
      MODEM_MANAGER_SERVICE,
      modem_path.as_str(),
      MODEM_MANAGER_3GPP_INTERFACE,
   ) {
      Ok(modem) => modem,
      Err(error) => {
         warn!(%error, "failed to create ModemManager proxy; skipping roaming check");
         return false;
      }
   };

   match modem.get_property::<u32>("RegistrationState") {
      Ok(registration_state) => {
         let roaming = is_roaming(registration_state);
         debug!(
            registration_state,
            roaming, "queried ModemManager 3GPP registration state"
         );
         roaming
      }
      Err(error) => {
         warn!(%error, "failed to read ModemManager 3GPP registration state");
         false
      }
   }
}

fn dbus_proxy<'a>(
   connection: &'a Connection,
   destination: &'a str,
   path: &'a str,
   interface: &'a str,
) -> zbus::Result<Proxy<'a>> {
   ProxyBuilder::new(connection)
      .destination(destination)?
      .path(path)?
      .interface(interface)?
      .cache_properties(CacheProperties::No)
      .build()
}

fn service_has_owner(connection: &Connection, service: &str) -> zbus::Result<bool> {
   let proxy = DBusProxy::builder(connection)
      .destination(DBUS_SERVICE)?
      .cache_properties(CacheProperties::No)
      .build()?;
   let service_name = BusName::try_from(service)?;

   Ok(proxy.name_has_owner(service_name)?)
}

fn fallback_connection_status() -> ConnectionStatus {
   // Systems that do not run NetworkManager still commonly expose kernel route
   // tables through /proc. An up, non-loopback default route is the strongest
   // passive signal available without probing the network.
   let ipv4_route_table = match fs::read_to_string(PROC_NET_ROUTE) {
      Ok(route_table) => route_table,
      Err(error) => {
         warn!(%error, "failed to read Linux IPv4 route table");
         String::new()
      }
   };
   let ipv6_route_table = match fs::read_to_string(PROC_NET_IPV6_ROUTE) {
      Ok(route_table) => route_table,
      Err(error) => {
         warn!(%error, "failed to read Linux IPv6 route table");
         String::new()
      }
   };

   let Some(iface) = default_ipv4_route_interface(&ipv4_route_table)
      .or_else(|| default_ipv6_route_interface(&ipv6_route_table))
   else {
      debug!("Linux route table does not contain an up, non-loopback default route");
      return ConnectionStatus::disconnected();
   };

   let connection_type = infer_transport_from_sysfs(Path::new(SYS_CLASS_NET), &iface);
   let status = ConnectionStatus {
      connected: true,
      metered: false,
      constrained: false,
      connection_type,
   };

   debug!(
      iface,
      connection_type = ?status.connection_type,
      "resolved Linux connection status via passive fallback"
   );

   status
}

fn map_connectivity(connectivity: u32) -> ConnectedState {
   match connectivity {
      NM_CONNECTIVITY_FULL => ConnectedState::Connected,
      NM_CONNECTIVITY_PORTAL | NM_CONNECTIVITY_LIMITED => ConnectedState::Constrained,
      NM_CONNECTIVITY_NONE => ConnectedState::Disconnected,
      _ => ConnectedState::Unknown,
   }
}

fn has_global_connectivity(state: u32) -> bool {
   state == NM_STATE_CONNECTED_GLOBAL
}

fn map_device_type(device_type: u32) -> ConnectionType {
   match device_type {
      NM_DEVICE_TYPE_ETHERNET => ConnectionType::Ethernet,
      NM_DEVICE_TYPE_WIFI => ConnectionType::Wifi,
      NM_DEVICE_TYPE_MODEM => ConnectionType::Cellular,
      _ => ConnectionType::Unknown,
   }
}

fn collect_supported_connection_types(
   device_types: impl IntoIterator<Item = u32>,
) -> Vec<ConnectionType> {
   let mut connection_types = ConnectionTypes::new();

   for device_type in device_types {
      connection_types.insert(map_device_type(device_type));
   }

   connection_types.into_vec()
}

fn collect_supported_connection_types_from_devices<E>(
   devices: impl IntoIterator<Item = OwnedObjectPath>,
   mut read_device_type: impl FnMut(&OwnedObjectPath) -> std::result::Result<u32, E>,
) -> Vec<ConnectionType>
where
   E: std::fmt::Display,
{
   let device_types = devices
      .into_iter()
      .filter_map(|device| match read_device_type(&device) {
         Ok(device_type) => {
            debug!(
               device = %device.as_str(),
               device_type,
               "queried NetworkManager supported device type"
            );
            Some(device_type)
         }
         Err(error) => {
            warn!(%error, device = %device.as_str(), "failed to read NetworkManager device type");
            None
         }
      });

   collect_supported_connection_types(device_types)
}

fn is_metered(metered: u32) -> bool {
   matches!(metered, NM_METERED_YES | NM_METERED_GUESS_YES)
}

fn is_constrained(connectivity_state: ConnectedState, metered: bool, roaming: bool) -> bool {
   // NetworkManager does not expose a separate background-data policy signal.
   // Treat an explicitly or guessed metered primary device as constrained so
   // callers can avoid discretionary data use on Linux.
   connectivity_state == ConnectedState::Constrained || metered || roaming
}

fn is_roaming(registration_state: u32) -> bool {
   registration_state == MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING
}

fn is_modem_manager_modem_path(path: &str) -> bool {
   path.starts_with(MODEM_MANAGER_MODEM_PREFIX) && ObjectPath::try_from(path).is_ok()
}

fn is_root_path(path: &OwnedObjectPath) -> bool {
   path.as_str() == "/"
}

fn default_ipv4_route_interface(route_table: &str) -> Option<String> {
   route_table.lines().skip(1).find_map(|line| {
      let fields: Vec<_> = line.split_whitespace().collect();

      if fields.len() < 4 {
         return None;
      }

      let iface = fields[0];
      let destination = fields[1];
      let flags = fields[3];

      if destination == IPV4_DEFAULT_DESTINATION && iface != "lo" && route_is_up(flags) {
         Some(iface.to_owned())
      } else {
         None
      }
   })
}

fn default_ipv6_route_interface(route_table: &str) -> Option<String> {
   route_table.lines().find_map(|line| {
      let fields: Vec<_> = line.split_whitespace().collect();

      if fields.len() < 10 {
         return None;
      }

      let destination = fields[0];
      let prefix_len = fields[1];
      let flags = fields[8];
      let iface = fields[9];

      if destination == IPV6_DEFAULT_DESTINATION
         && prefix_len == IPV6_DEFAULT_PREFIX_LEN
         && iface != "lo"
         && route_is_up(flags)
      {
         Some(iface.to_owned())
      } else {
         None
      }
   })
}

fn route_is_up(flags: &str) -> bool {
   u32::from_str_radix(flags, 16).is_ok_and(|flags| flags & LINUX_ROUTE_FLAG_UP != 0)
}

fn infer_transport_from_sysfs(sys_class_net: &Path, iface: &str) -> ConnectionType {
   let interface_path = sys_class_net.join(iface);

   if has_wifi_marker(&interface_path) {
      debug!(iface, "sysfs classified fallback interface as Wi-Fi");
      return ConnectionType::Wifi;
   }

   if has_wwan_marker(&interface_path) {
      debug!(iface, "sysfs classified fallback interface as cellular");
      return ConnectionType::Cellular;
   }

   if read_u32(interface_path.join("type")).is_some_and(|value| value == LINUX_ARPHRD_ETHER) {
      debug!(iface, "sysfs classified fallback interface as Ethernet");
      return ConnectionType::Ethernet;
   }

   debug!(iface, "sysfs could not classify fallback interface");
   ConnectionType::Unknown
}

fn supported_types_from_sysfs(sys_class_net: &Path) -> Vec<ConnectionType> {
   // `/sys/class/net` is the kernel's sysfs view of present network interfaces.
   // This fallback is intentionally passive, matching the status fallback above:
   // https://docs.kernel.org/networking/net-sysfs.html
   let Ok(entries) = fs::read_dir(sys_class_net) else {
      warn!(path = %sys_class_net.display(), "failed to read Linux sysfs network interfaces");
      return Vec::new();
   };

   let mut connection_types = ConnectionTypes::new();

   for entry in entries.flatten() {
      let iface = entry.file_name();
      let iface = iface.to_string_lossy();

      if iface == "lo" || is_virtual_sysfs_interface(&entry.path()) {
         continue;
      }

      connection_types.insert(infer_transport_from_sysfs(sys_class_net, &iface));
   }

   connection_types.into_vec()
}

fn is_virtual_sysfs_interface(interface_path: &Path) -> bool {
   // Virtual interfaces are exposed under sysfs `virtual/net`; physical
   // devices normally resolve through their backing device path. Excluding this
   // keeps bridges, tunnels, loopback-like devices, and veth pairs out of the
   // supported physical transport list.
   // https://docs.kernel.org/filesystems/sysfs.html
   path_has_exact_component(interface_path, "virtual")
      || path_has_exact_component(interface_path.join("device"), "virtual")
}

fn has_wifi_marker(interface_path: &Path) -> bool {
   interface_path.join("wireless").exists()
      || interface_path.join("phy80211").exists()
      || interface_path.join("ieee80211").exists()
      || interface_path.join("device").join("ieee80211").exists()
}

fn has_wwan_marker(interface_path: &Path) -> bool {
   interface_path.join("wwan").exists()
      || interface_path.join("device").join("wwan").exists()
      || path_has_exact_component(interface_path.join("device").join("subsystem"), "wwan")
}

fn path_has_exact_component(path: impl AsRef<Path>, marker: &str) -> bool {
   let Ok(path) = fs::canonicalize(path) else {
      return false;
   };

   path.components().any(|component| {
      component
         .as_os_str()
         .to_string_lossy()
         .eq_ignore_ascii_case(marker)
   })
}

fn read_u32(path: impl AsRef<Path>) -> Option<u32> {
   fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::fs::{self, File};
   use std::io::Write;
   use std::os::unix::fs as unix_fs;
   use std::path::PathBuf;
   use std::sync::atomic::{AtomicUsize, Ordering};

   static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

   #[test]
   fn limits_each_dbus_method_call_to_500_milliseconds() {
      assert_eq!(DBUS_METHOD_TIMEOUT, Duration::from_millis(500));
   }

   #[test]
   fn maps_connectivity_states() {
      assert_eq!(
         map_connectivity(NM_CONNECTIVITY_FULL),
         ConnectedState::Connected
      );
      assert_eq!(
         map_connectivity(NM_CONNECTIVITY_NONE),
         ConnectedState::Disconnected
      );
      assert_eq!(
         map_connectivity(NM_CONNECTIVITY_PORTAL),
         ConnectedState::Constrained
      );
      assert_eq!(
         map_connectivity(NM_CONNECTIVITY_LIMITED),
         ConnectedState::Constrained
      );
      assert_eq!(map_connectivity(0), ConnectedState::Unknown);
      assert_eq!(map_connectivity(99), ConnectedState::Unknown);
   }

   #[test]
   fn falls_back_to_global_state_only_for_unknown_connectivity() {
      assert!(has_global_connectivity(NM_STATE_CONNECTED_GLOBAL));
      assert!(!has_global_connectivity(60));
      assert!(!has_global_connectivity(20));
   }

   #[test]
   fn identifies_metered_states() {
      assert!(!is_metered(0));
      assert!(is_metered(NM_METERED_YES));
      assert!(!is_metered(2));
      assert!(is_metered(NM_METERED_GUESS_YES));
      assert!(!is_metered(4));
   }

   #[test]
   fn maps_network_manager_device_types() {
      assert_eq!(
         map_device_type(NM_DEVICE_TYPE_ETHERNET),
         ConnectionType::Ethernet
      );
      assert_eq!(map_device_type(NM_DEVICE_TYPE_WIFI), ConnectionType::Wifi);
      assert_eq!(
         map_device_type(NM_DEVICE_TYPE_MODEM),
         ConnectionType::Cellular
      );
      assert_eq!(map_device_type(999), ConnectionType::Unknown);
   }

   #[test]
   fn collects_supported_connection_types_from_network_manager_device_types() {
      assert_eq!(
         collect_supported_connection_types([
            NM_DEVICE_TYPE_MODEM,
            999,
            NM_DEVICE_TYPE_WIFI,
            NM_DEVICE_TYPE_MODEM,
            NM_DEVICE_TYPE_ETHERNET,
         ]),
         vec![
            ConnectionType::Wifi,
            ConnectionType::Ethernet,
            ConnectionType::Cellular,
         ]
      );
   }

   #[test]
   fn skips_network_manager_devices_that_cannot_be_read() {
      let devices = [
         OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/Devices/1").unwrap(),
         OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/Devices/2").unwrap(),
         OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/Devices/3").unwrap(),
      ];

      assert_eq!(
         collect_supported_connection_types_from_devices(devices, |device| {
            match device.as_str() {
               "/org/freedesktop/NetworkManager/Devices/1" => Ok(NM_DEVICE_TYPE_ETHERNET),
               "/org/freedesktop/NetworkManager/Devices/2" => Err("device disappeared"),
               "/org/freedesktop/NetworkManager/Devices/3" => Ok(NM_DEVICE_TYPE_WIFI),
               _ => unreachable!(),
            }
         }),
         vec![ConnectionType::Wifi, ConnectionType::Ethernet]
      );
   }

   #[test]
   fn treats_metering_or_roaming_as_constrained() {
      assert!(!is_constrained(ConnectedState::Connected, false, false));
      assert!(is_constrained(ConnectedState::Constrained, false, false));
      assert!(is_constrained(ConnectedState::Connected, true, false));
      assert!(is_constrained(ConnectedState::Connected, false, true));
      assert!(is_constrained(ConnectedState::Connected, true, true));
   }

   #[test]
   fn treats_unknown_connection_details_as_metered() {
      let details = ConnectionDetails::metered_unknown();

      assert!(details.metered);
      assert!(!details.roaming);
      assert_eq!(details.connection_type, ConnectionType::Unknown);
   }

   #[test]
   fn detects_roaming_registration_state() {
      assert!(is_roaming(MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING));
      assert!(!is_roaming(1));
   }

   #[test]
   fn validates_modem_manager_modem_paths() {
      assert!(is_modem_manager_modem_path(
         "/org/freedesktop/ModemManager1/Modem/0"
      ));
      assert!(!is_modem_manager_modem_path("/"));
      assert!(!is_modem_manager_modem_path(
         "/org/freedesktop/NetworkManager/Devices/0"
      ));
   }

   #[test]
   fn parses_default_ipv4_route_interface() {
      let route_table = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
eth0\t00000000\t015018AC\t0003\t0\t0\t0\t00000000\t0\t0\t0
";

      assert_eq!(
         default_ipv4_route_interface(route_table),
         Some("eth0".into())
      );
   }

   #[test]
   fn ignores_loopback_default_route() {
      let route_table = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
lo\t00000000\t00000000\t0003\t0\t0\t0\t00000000\t0\t0\t0
";

      assert_eq!(default_ipv4_route_interface(route_table), None);
   }

   #[test]
   fn ignores_down_default_route() {
      let route_table = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
eth0\t00000000\t015018AC\t0002\t0\t0\t0\t00000000\t0\t0\t0
";

      assert_eq!(default_ipv4_route_interface(route_table), None);
   }

   #[test]
   fn returns_none_without_default_route() {
      let route_table = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
eth0\t005018AC\t00000000\t0001\t0\t0\t0\t00F0FFFF\t0\t0\t0
";

      assert_eq!(default_ipv4_route_interface(route_table), None);
   }

   #[test]
   fn ignores_malformed_route_rows() {
      let route_table = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
malformed
eth0\t00000000\t015018AC\t0003\t0\t0\t0\t00000000\t0\t0\t0
";

      assert_eq!(
         default_ipv4_route_interface(route_table),
         Some("eth0".into())
      );
   }

   #[test]
   fn parses_ipv6_default_route_interface() {
      let route_table = "\
00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000400 00000000 00000000 00000003 eth0
";

      assert_eq!(
         default_ipv6_route_interface(route_table),
         Some("eth0".into())
      );
   }

   #[test]
   fn ignores_ipv6_loopback_default_route() {
      let route_table = "\
00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo
";

      assert_eq!(default_ipv6_route_interface(route_table), None);
   }

   #[test]
   fn ignores_down_ipv6_default_route() {
      let route_table = "\
00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000400 00000000 00000000 00000002 eth0
";

      assert_eq!(default_ipv6_route_interface(route_table), None);
   }

   #[test]
   fn returns_none_without_ipv6_default_route() {
      let route_table = "\
fe800000000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001 eth0
";

      assert_eq!(default_ipv6_route_interface(route_table), None);
   }

   #[test]
   fn ignores_malformed_ipv6_route_rows() {
      let route_table = "\
malformed
00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000400 00000000 00000000 00000003 eth0
";

      assert_eq!(
         default_ipv6_route_interface(route_table),
         Some("eth0".into())
      );
   }

   #[test]
   fn infers_wifi_from_wireless_directory() {
      let temp = TempDir::new();
      let iface = temp.path().join("wlp0s20f3");
      fs::create_dir_all(iface.join("wireless")).unwrap();

      assert_eq!(
         infer_transport_from_sysfs(temp.path(), "wlp0s20f3"),
         ConnectionType::Wifi
      );
   }

   #[test]
   fn infers_wifi_from_ieee80211_marker() {
      let temp = TempDir::new();
      let iface = temp.path().join("net0");
      fs::create_dir_all(iface.join("ieee80211")).unwrap();

      assert_eq!(
         infer_transport_from_sysfs(temp.path(), "net0"),
         ConnectionType::Wifi
      );
   }

   #[test]
   fn infers_wifi_from_phy80211_marker() {
      let temp = TempDir::new();
      let iface = temp.path().join("net0");
      fs::create_dir_all(iface.join("phy80211")).unwrap();

      assert_eq!(
         infer_transport_from_sysfs(temp.path(), "net0"),
         ConnectionType::Wifi
      );
   }

   #[test]
   fn does_not_infer_wifi_from_80211_path_fragment() {
      let temp = TempDir::new();
      let sys_class_net = temp.path().join("not80211-device");
      let iface = sys_class_net.join("net0");
      fs::create_dir_all(&iface).unwrap();
      write_file(iface.join("type"), "1\n");

      assert_eq!(
         infer_transport_from_sysfs(&sys_class_net, "net0"),
         ConnectionType::Ethernet
      );
   }

   #[test]
   fn infers_cellular_from_wwan_marker() {
      let temp = TempDir::new();
      let iface = temp.path().join("net0");
      fs::create_dir_all(iface.join("device").join("wwan")).unwrap();

      assert_eq!(
         infer_transport_from_sysfs(temp.path(), "net0"),
         ConnectionType::Cellular
      );
   }

   #[test]
   fn does_not_infer_cellular_from_wwan_path_fragment() {
      let temp = TempDir::new();
      let iface = temp.path().join("net0");
      let subsystem_target = temp.path().join("notwwan-bus");
      fs::create_dir_all(iface.join("device")).unwrap();
      fs::create_dir_all(&subsystem_target).unwrap();
      unix_fs::symlink(&subsystem_target, iface.join("device").join("subsystem")).unwrap();
      write_file(iface.join("type"), "1\n");

      assert_eq!(
         infer_transport_from_sysfs(temp.path(), "net0"),
         ConnectionType::Ethernet
      );
   }

   #[test]
   fn infers_ethernet_from_arphrd_ether_type() {
      let temp = TempDir::new();
      let iface = temp.path().join("enp0s1");
      fs::create_dir_all(&iface).unwrap();
      write_file(iface.join("type"), "1\n");

      assert_eq!(
         infer_transport_from_sysfs(temp.path(), "enp0s1"),
         ConnectionType::Ethernet
      );
   }

   #[test]
   fn supported_types_from_sysfs_ignores_virtual_interfaces() {
      let temp = TempDir::new();
      let physical = temp.path().join("enp0s1");
      let virtual_iface = temp.path().join("virtual").join("net").join("veth0");
      fs::create_dir_all(&physical).unwrap();
      fs::create_dir_all(&virtual_iface).unwrap();
      write_file(physical.join("type"), "1\n");
      write_file(virtual_iface.join("type"), "1\n");
      unix_fs::symlink(&virtual_iface, temp.path().join("veth0")).unwrap();

      assert_eq!(
         supported_types_from_sysfs(temp.path()),
         vec![ConnectionType::Ethernet]
      );
   }

   #[test]
   fn returns_unknown_when_sysfs_has_no_transport_signal() {
      let temp = TempDir::new();
      let iface = temp.path().join("net0");
      fs::create_dir_all(&iface).unwrap();
      write_file(iface.join("type"), "772\n");

      assert_eq!(
         infer_transport_from_sysfs(temp.path(), "net0"),
         ConnectionType::Unknown
      );
   }

   fn write_file(path: impl AsRef<Path>, contents: &str) {
      let mut file = File::create(path).unwrap();
      file.write_all(contents.as_bytes()).unwrap();
   }

   struct TempDir {
      path: PathBuf,
   }

   impl TempDir {
      fn new() -> Self {
         let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
         let path = std::env::temp_dir().join(format!(
            "tauri-plugin-connectivity-linux-test-{}-{id}",
            std::process::id()
         ));

         if path.exists() {
            fs::remove_dir_all(&path).unwrap();
         }
         fs::create_dir_all(&path).unwrap();

         Self { path }
      }

      fn path(&self) -> &Path {
         &self.path
      }
   }

   impl Drop for TempDir {
      fn drop(&mut self) {
         let _ = fs::remove_dir_all(&self.path);
      }
   }
}
