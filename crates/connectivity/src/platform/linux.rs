use std::fs;
use std::path::Path;
use std::time::Duration;

use tracing::{debug, warn};
use zbus::connection::Builder as ConnectionBuilder;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::proxy::Builder as ProxyBuilder;
use zbus::proxy::CacheProperties;
use zbus::{Connection, ObjectPath, OwnedObjectPath, Proxy};

use crate::error::{Error, Result};
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

// ModemManager is only used for cellular roaming. A missing service or 3GPP
// interface, an unknown registration state, and read errors leave the roaming
// signal unknown.
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

const NM_METERED_UNKNOWN: u32 = 0;
const NM_METERED_YES: u32 = 1;
const NM_METERED_NO: u32 = 2;
const NM_METERED_GUESS_YES: u32 = 3;
const NM_METERED_GUESS_NO: u32 = 4;

const MM_MODEM_3GPP_REGISTRATION_STATE_UNKNOWN: u32 = 4;
const MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING: u32 = 5;
const MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING_SMS_ONLY: u32 = 7;
const MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING_CSFB_NOT_PREFERRED: u32 = 10;
const MM_MODEM_3GPP_REGISTRATION_STATE_ATTACHED_RLOS: u32 = 11;

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

struct FallbackPaths<'a> {
   ipv4_routes: &'a Path,
   ipv6_routes: &'a Path,
   sys_class_net: &'a Path,
}

impl Default for FallbackPaths<'static> {
   fn default() -> Self {
      Self {
         ipv4_routes: Path::new(PROC_NET_ROUTE),
         ipv6_routes: Path::new(PROC_NET_IPV6_ROUTE),
         sys_class_net: Path::new(SYS_CLASS_NET),
      }
   }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectedState {
   Connected,
   Constrained,
   Disconnected,
   Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConnectionDetails {
   metered: Option<bool>,
   roaming: Option<bool>,
   connection_type: ConnectionType,
}

impl Default for ConnectionDetails {
   fn default() -> Self {
      Self {
         metered: None,
         roaming: None,
         connection_type: ConnectionType::Unknown,
      }
   }
}

/// Returns the current Linux network connection status.
///
/// NetworkManager is preferred when available because it exposes cached
/// connectivity, primary-route, transport, and metered state over D-Bus. Systems
/// without NetworkManager fall back to passive kernel state only.
pub(crate) fn connection_status() -> Result<ConnectionStatus> {
   // Drive connection creation and every D-Bus operation on the same runtime.
   zbus::block_on(async {
      connection_status_from_connection(system_bus_connection().await, &FallbackPaths::default())
         .await
   })
}

async fn connection_status_from_connection(
   connection: zbus::Result<Connection>,
   fallback_paths: &FallbackPaths<'_>,
) -> Result<ConnectionStatus> {
   debug!("querying Linux connection status");

   let connection = match connection {
      Ok(connection) => {
         debug!("connected to Linux system D-Bus");
         connection
      }
      Err(error) => {
         warn!(%error, "failed to connect to Linux system bus; using passive fallback");
         return Ok(fallback_connection_status(fallback_paths));
      }
   };

   match service_has_owner(&connection, NETWORK_MANAGER_SERVICE).await {
      Ok(true) => {
         debug!("NetworkManager service is present");

         match network_manager_connection_status(&connection).await {
            Ok(status) => {
               debug!(
                  ?status,
                  "resolved Linux connection status via NetworkManager"
               );
               Ok(status)
            }
            Err(error) => {
               warn!(%error, "failed to query NetworkManager; using passive fallback");
               Ok(fallback_connection_status(fallback_paths))
            }
         }
      }
      Ok(false) => {
         debug!("NetworkManager service is not present; using passive fallback");
         Ok(fallback_connection_status(fallback_paths))
      }
      Err(error) => {
         warn!(%error, "failed to probe NetworkManager service; using passive fallback");
         Ok(fallback_connection_status(fallback_paths))
      }
   }
}

/// Returns the supported physical connection transport classes.
pub(crate) fn supported_connection_types() -> Result<Vec<ConnectionType>> {
   zbus::block_on(async {
      supported_connection_types_from_connection(
         system_bus_connection().await,
         &FallbackPaths::default(),
      )
      .await
   })
}

async fn supported_connection_types_from_connection(
   connection: zbus::Result<Connection>,
   fallback_paths: &FallbackPaths<'_>,
) -> Result<Vec<ConnectionType>> {
   debug!("querying Linux supported connection types");

   // Prefer NetworkManager's realized `Devices` list. Its D-Bus docs describe
   // `Devices` as the network devices currently known to NetworkManager, while
   // `AllDevices` can include placeholders that do not correspond to real,
   // present hardware:
   // https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.html
   let connection = match connection {
      Ok(connection) => connection,
      Err(error) => {
         warn!(%error, "failed to connect to Linux system bus; using sysfs fallback");
         return supported_types_from_sysfs(fallback_paths.sys_class_net);
      }
   };

   match service_has_owner(&connection, NETWORK_MANAGER_SERVICE).await {
      Ok(true) => match network_manager_supported_connection_types(&connection).await {
         Ok(connection_types) => Ok(connection_types),
         Err(error) => {
            warn!(%error, "failed to query NetworkManager devices; using sysfs fallback");
            supported_types_from_sysfs(fallback_paths.sys_class_net)
         }
      },
      Ok(false) => supported_types_from_sysfs(fallback_paths.sys_class_net),
      Err(error) => {
         warn!(%error, "failed to probe NetworkManager service; using sysfs fallback");
         supported_types_from_sysfs(fallback_paths.sys_class_net)
      }
   }
}

async fn network_manager_supported_connection_types(
   connection: &Connection,
) -> zbus::Result<Vec<ConnectionType>> {
   let manager = dbus_proxy(
      connection,
      NETWORK_MANAGER_SERVICE,
      NETWORK_MANAGER_PATH,
      NETWORK_MANAGER_INTERFACE,
   )
   .await?;
   let devices = manager
      .get_property::<Vec<OwnedObjectPath>>("Devices")
      .await?;

   collect_supported_connection_types_from_devices(devices, async |device| {
      // DeviceType is the NetworkManager transport enum. Values used below are
      // from the NetworkManager D-Bus type reference:
      // https://networkmanager.pages.freedesktop.org/NetworkManager/NetworkManager/nm-dbus-types.html
      let device_proxy = dbus_proxy(
         connection,
         NETWORK_MANAGER_SERVICE,
         device.as_str(),
         NETWORK_MANAGER_DEVICE_INTERFACE,
      )
      .await?;
      device_proxy.get_property::<u32>("DeviceType").await
   })
   .await
}

async fn system_bus_connection() -> zbus::Result<Connection> {
   bus_connection(ConnectionBuilder::system()).await
}

async fn bus_connection(builder: ConnectionBuilder<'_>) -> zbus::Result<Connection> {
   builder.method_timeout(DBUS_METHOD_TIMEOUT).build().await
}

async fn network_manager_connection_status(
   connection: &Connection,
) -> zbus::Result<ConnectionStatus> {
   let manager = dbus_proxy(
      connection,
      NETWORK_MANAGER_SERVICE,
      NETWORK_MANAGER_PATH,
      NETWORK_MANAGER_INTERFACE,
   )
   .await?;

   // `Connectivity` is a cached property. `FULL` maps to full connectivity,
   // `PORTAL` and `LIMITED` map to connected but constrained, and `UNKNOWN`
   // falls back to NM's broader networking state.
   let connectivity = manager.get_property::<u32>("Connectivity").await?;
   debug!(connectivity, "queried NetworkManager connectivity state");

   let connectivity_state = match map_connectivity(connectivity) {
      ConnectedState::Unknown => {
         let state = manager.get_property::<u32>("State").await?;
         debug!(
            connectivity,
            state, "NetworkManager connectivity is unknown; falling back to state"
         );
         connectivity_state_from_global_state(state)
      }
      connectivity_state => connectivity_state,
   };
   let connected = matches!(
      connectivity_state,
      ConnectedState::Connected | ConnectedState::Constrained
   );

   if !connected {
      debug!(
         connectivity,
         "NetworkManager connectivity does not indicate active internet access"
      );
      return Ok(ConnectionStatus::disconnected());
   }

   let details = match primary_connection_details(connection, &manager).await {
      Ok(details) => details,
      Err(error) => {
         warn!(%error, "failed to resolve Linux primary connection details; policy state is unknown");
         ConnectionDetails::default()
      }
   };

   Ok(ConnectionStatus {
      connected: true,
      metered: details.metered,
      constrained: constrained_status(connectivity_state, details.metered, details.roaming),
      connection_type: details.connection_type,
   })
}

async fn primary_connection_details(
   connection: &Connection,
   manager: &Proxy<'_>,
) -> zbus::Result<ConnectionDetails> {
   // NetworkManager chooses the primary connection for the default route. Its
   // active connection object is the stable way to find the devices that should
   // drive transport and metered decisions.
   let primary_connection = manager
      .get_property::<OwnedObjectPath>("PrimaryConnection")
      .await?;
   debug!(
      primary_connection = %primary_connection.as_str(),
      "queried NetworkManager primary connection"
   );

   if is_root_path(&primary_connection) {
      warn!("NetworkManager returned no primary connection; policy state is unknown");
      return Ok(ConnectionDetails::default());
   }

   let active_connection = dbus_proxy(
      connection,
      NETWORK_MANAGER_SERVICE,
      primary_connection.as_str(),
      NETWORK_MANAGER_ACTIVE_CONNECTION_INTERFACE,
   )
   .await?;
   let devices = active_connection
      .get_property::<Vec<OwnedObjectPath>>("Devices")
      .await?;
   debug!(
      device_count = devices.len(),
      primary_connection = %primary_connection.as_str(),
      "queried NetworkManager primary connection devices"
   );

   if devices.is_empty() {
      warn!("NetworkManager primary connection has no devices; policy state is unknown");
      return Ok(ConnectionDetails::default());
   }

   let mut details = ConnectionDetails::default();
   let mut read_any_device = false;
   let mut metered_states = Vec::with_capacity(devices.len());
   let mut roaming_states = Vec::with_capacity(devices.len());

   for device in devices {
      match device_details(connection, &device).await {
         Ok(device_details) => {
            read_any_device = true;
            metered_states.push(device_details.metered);
            roaming_states.push(device_details.roaming);
            if details.connection_type == ConnectionType::Unknown {
               details.connection_type = device_details.connection_type;
            }

            debug!(
               device = %device.as_str(),
               metered = ?device_details.metered,
               roaming = ?device_details.roaming,
               connection_type = ?device_details.connection_type,
               "resolved NetworkManager device details"
            );
         }
         Err(error) => {
            warn!(%error, device = %device.as_str(), "failed to read NetworkManager device");
            metered_states.push(None);
            roaming_states.push(None);
         }
      }
   }

   if !read_any_device {
      warn!(
         "failed to read any NetworkManager primary connection devices; policy state is unknown"
      );
   }

   details.metered = combine_policy_states(metered_states);
   details.roaming = combine_policy_states(roaming_states);

   Ok(details)
}

async fn device_details(
   connection: &Connection,
   device: &OwnedObjectPath,
) -> zbus::Result<ConnectionDetails> {
   let device_proxy = dbus_proxy(
      connection,
      NETWORK_MANAGER_SERVICE,
      device.as_str(),
      NETWORK_MANAGER_DEVICE_INTERFACE,
   )
   .await?;

   // DeviceType gives the transport class; Metered lives on the device, not on
   // the active connection.
   let device_type = device_proxy.get_property::<u32>("DeviceType").await?;
   let connection_type = map_device_type(device_type);
   debug!(
      device = %device.as_str(),
      device_type,
      connection_type = ?connection_type,
      "queried NetworkManager device type"
   );

   let metered = match device_proxy.get_property::<u32>("Metered").await {
      Ok(metered) => {
         let metered_status = metered_status(metered);
         debug!(
            device = %device.as_str(),
            metered,
            metered_status = ?metered_status,
            "queried NetworkManager device metered state"
         );
         metered_status
      }
      Err(error) => {
         warn!(%error, device = %device.as_str(), "failed to read NetworkManager device metered state; metering is unknown");
         None
      }
   };
   let roaming = connection_type_roaming_status(connection_type, async || {
      modem_is_roaming(connection, &device_proxy).await
   })
   .await;

   Ok(ConnectionDetails {
      metered,
      roaming,
      connection_type,
   })
}

async fn connection_type_roaming_status(
   connection_type: ConnectionType,
   cellular_status: impl AsyncFnOnce() -> Option<bool>,
) -> Option<bool> {
   if connection_type == ConnectionType::Cellular {
      cellular_status().await
   } else {
      // NetworkManager's modem type maps to Cellular. Every other mapped or
      // unknown transport is therefore definitively outside roaming policy.
      Some(false)
   }
}

async fn modem_is_roaming(connection: &Connection, device_proxy: &Proxy<'_>) -> Option<bool> {
   // NM modem devices expose a `Udi` that usually points at the corresponding
   // ModemManager object. Only that object can tell us whether the cellular
   // registration state is roaming.
   match service_has_owner(connection, MODEM_MANAGER_SERVICE).await {
      Ok(true) => {}
      Ok(false) => {
         debug!("ModemManager service is not present; roaming is unknown");
         return None;
      }
      Err(error) => {
         warn!(%error, "failed to probe ModemManager service; roaming is unknown");
         return None;
      }
   }

   let udi = match device_proxy.get_property::<String>("Udi").await {
      Ok(udi) => {
         debug!(udi, "queried NetworkManager modem Udi");
         udi
      }
      Err(error) => {
         warn!(%error, "failed to read NetworkManager modem Udi; roaming is unknown");
         return None;
      }
   };

   if !is_modem_manager_modem_path(&udi) {
      debug!(
         udi,
         "NetworkManager modem Udi is not a ModemManager modem path"
      );
      return None;
   }

   let modem_path = match ObjectPath::try_from(udi.as_str()) {
      Ok(path) => path,
      Err(error) => {
         warn!(%error, udi, "NetworkManager modem Udi is not a valid D-Bus object path");
         return None;
      }
   };

   let modem = match dbus_proxy(
      connection,
      MODEM_MANAGER_SERVICE,
      modem_path.as_str(),
      MODEM_MANAGER_3GPP_INTERFACE,
   )
   .await
   {
      Ok(modem) => modem,
      Err(error) => {
         warn!(%error, "failed to create ModemManager proxy; roaming is unknown");
         return None;
      }
   };

   match modem.get_property::<u32>("RegistrationState").await {
      Ok(registration_state) => {
         let roaming = roaming_status(registration_state);
         debug!(
            registration_state,
            roaming = ?roaming,
            "queried ModemManager 3GPP registration state"
         );
         roaming
      }
      Err(error) => {
         warn!(%error, "failed to read ModemManager 3GPP registration state; roaming is unknown");
         None
      }
   }
}

async fn dbus_proxy<'a>(
   connection: &'a Connection,
   destination: &'a str,
   path: &'a str,
   interface: &'a str,
) -> zbus::Result<Proxy<'a>> {
   ProxyBuilder::new(connection)
      .destination(destination)
      .path(path)
      .interface(interface)
      .cache_properties(CacheProperties::No)
      .build()
      .await
}

async fn service_has_owner(connection: &Connection, service: &str) -> zbus::Result<bool> {
   let proxy = DBusProxy::builder(connection)
      .destination(DBUS_SERVICE)
      .cache_properties(CacheProperties::No)
      .build()
      .await?;
   let service_name = BusName::try_from(service)?;

   Ok(proxy.name_has_owner(service_name).await?)
}

fn fallback_connection_status(paths: &FallbackPaths<'_>) -> ConnectionStatus {
   // Systems that do not run NetworkManager still commonly expose kernel route
   // tables through /proc. An up, non-loopback default route is the strongest
   // passive signal available without probing the network.
   let ipv4_route_table = match fs::read_to_string(paths.ipv4_routes) {
      Ok(route_table) => route_table,
      Err(error) => {
         warn!(%error, "failed to read Linux IPv4 route table");
         String::new()
      }
   };
   let ipv6_route_table = match fs::read_to_string(paths.ipv6_routes) {
      Ok(route_table) => route_table,
      Err(error) => {
         warn!(%error, "failed to read Linux IPv6 route table");
         String::new()
      }
   };

   fallback_connection_status_from_routes(&ipv4_route_table, &ipv6_route_table, paths.sys_class_net)
}

fn fallback_connection_status_from_routes(
   ipv4_route_table: &str,
   ipv6_route_table: &str,
   sys_class_net: &Path,
) -> ConnectionStatus {
   let Some(iface) = default_ipv4_route_interface(ipv4_route_table)
      .or_else(|| default_ipv6_route_interface(ipv6_route_table))
   else {
      debug!("Linux route table does not contain an up, non-loopback default route");
      return ConnectionStatus::disconnected();
   };

   let connection_type = infer_transport_from_sysfs(sys_class_net, &iface);
   let status = ConnectionStatus {
      connected: true,
      metered: None,
      constrained: None,
      connection_type,
   };

   debug!(
      iface,
      connection_type = ?status.connection_type,
      "resolved Linux connection status via passive fallback without cost information"
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

fn connectivity_state_from_global_state(state: u32) -> ConnectedState {
   if has_global_connectivity(state) {
      ConnectedState::Connected
   } else {
      ConnectedState::Disconnected
   }
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

async fn collect_supported_connection_types_from_devices<E>(
   devices: impl IntoIterator<Item = OwnedObjectPath>,
   mut read_device_type: impl AsyncFnMut(&OwnedObjectPath) -> std::result::Result<u32, E>,
) -> std::result::Result<Vec<ConnectionType>, E>
where
   E: std::fmt::Display,
{
   let mut device_types = Vec::new();
   let mut first_error = None;

   for device in devices {
      match read_device_type(&device).await {
         Ok(device_type) => {
            debug!(
               device = %device.as_str(),
               device_type,
               "queried NetworkManager supported device type"
            );
            device_types.push(device_type);
         }
         Err(error) => {
            warn!(%error, device = %device.as_str(), "failed to read NetworkManager device type");
            first_error.get_or_insert(error);
         }
      }
   }

   let connection_types = collect_supported_connection_types(device_types);

   if connection_types.is_empty()
      && let Some(error) = first_error
   {
      return Err(error);
   }

   Ok(connection_types)
}

fn metered_status(metered: u32) -> Option<bool> {
   if metered == NM_METERED_UNKNOWN {
      return None;
   }

   match metered {
      NM_METERED_YES | NM_METERED_GUESS_YES => Some(true),
      NM_METERED_NO | NM_METERED_GUESS_NO => Some(false),
      _ => None,
   }
}

fn constrained_status(
   connectivity_state: ConnectedState,
   metered: Option<bool>,
   roaming: Option<bool>,
) -> Option<bool> {
   // NetworkManager does not expose a separate background-data policy signal.
   // Treat an explicitly or guessed metered primary device as constrained so
   // callers can avoid discretionary data use on Linux.
   let connectivity_constrained = match connectivity_state {
      ConnectedState::Constrained => Some(true),
      ConnectedState::Connected | ConnectedState::Disconnected => Some(false),
      ConnectedState::Unknown => None,
   };

   combine_policy_states([connectivity_constrained, metered, roaming])
}

/// Combines independent policy signals without losing uncertainty. A confirmed
/// restriction wins, but a safe result requires every signal to be known false.
fn combine_policy_states(states: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
   let mut saw_known_state = false;
   let mut all_states_known = true;

   for state in states {
      match state {
         Some(true) => return Some(true),
         Some(false) => saw_known_state = true,
         None => all_states_known = false,
      }
   }

   (saw_known_state && all_states_known).then_some(false)
}

fn roaming_status(registration_state: u32) -> Option<bool> {
   match registration_state {
      MM_MODEM_3GPP_REGISTRATION_STATE_UNKNOWN => None,
      MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING
      | MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING_SMS_ONLY
      | MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING_CSFB_NOT_PREFERRED => Some(true),
      ..=MM_MODEM_3GPP_REGISTRATION_STATE_ATTACHED_RLOS => Some(false),
      _ => None,
   }
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
   match try_infer_transport_from_sysfs(sys_class_net, iface) {
      Ok(connection_type) => connection_type,
      Err(error) => {
         warn!(message = %error.message, iface, "failed to classify sysfs fallback interface");
         ConnectionType::Unknown
      }
   }
}

struct SysfsTransportError {
   message: String,
   code: Option<i32>,
}

fn try_infer_transport_from_sysfs(
   sys_class_net: &Path,
   iface: &str,
) -> std::result::Result<ConnectionType, SysfsTransportError> {
   let interface_path = sys_class_net.join(iface);

   if has_wifi_marker(&interface_path) {
      debug!(iface, "sysfs classified fallback interface as Wi-Fi");
      return Ok(ConnectionType::Wifi);
   }

   if has_wwan_marker(&interface_path) {
      debug!(iface, "sysfs classified fallback interface as cellular");
      return Ok(ConnectionType::Cellular);
   }

   let type_path = interface_path.join("type");
   let interface_type = fs::read_to_string(&type_path).map_err(|error| SysfsTransportError {
      message: format!(
         "failed to read Linux sysfs interface type at {}: {error}",
         type_path.display()
      ),
      code: error.raw_os_error(),
   })?;
   let interface_type =
      interface_type
         .trim()
         .parse::<u32>()
         .map_err(|error| SysfsTransportError {
            message: format!(
               "failed to parse Linux sysfs interface type at {}: {error}",
               type_path.display()
            ),
            code: None,
         })?;

   if interface_type == LINUX_ARPHRD_ETHER {
      debug!(iface, "sysfs classified fallback interface as Ethernet");
      return Ok(ConnectionType::Ethernet);
   }

   debug!(iface, "sysfs could not classify fallback interface");
   Ok(ConnectionType::Unknown)
}

fn supported_types_from_sysfs(sys_class_net: &Path) -> Result<Vec<ConnectionType>> {
   // `/sys/class/net` is the kernel's sysfs view of present network interfaces.
   // This fallback is intentionally passive, matching the status fallback above:
   // https://docs.kernel.org/networking/net-sysfs.html
   let entries = fs::read_dir(sys_class_net).map_err(|error| {
      Error::SupportedConnectionTypesDetectionFailed {
         message: format!(
            "failed to read Linux sysfs network interfaces at {}: {error}",
            sys_class_net.display()
         ),
         code: error.raw_os_error(),
      }
   })?;

   let mut connection_types = ConnectionTypes::new();
   let mut first_error = None;

   for entry in entries {
      let entry = match entry {
         Ok(entry) => entry,
         Err(error) => {
            let error = SysfsTransportError {
               message: format!(
                  "failed to enumerate Linux sysfs network interfaces at {}: {error}",
                  sys_class_net.display()
               ),
               code: error.raw_os_error(),
            };

            warn!(message = %error.message, "failed to enumerate supported sysfs interface");
            first_error.get_or_insert(error);
            continue;
         }
      };
      let iface = entry.file_name();
      let iface = iface.to_string_lossy();

      if iface == "lo" || is_virtual_sysfs_interface(&entry.path()) {
         continue;
      }

      match try_infer_transport_from_sysfs(sys_class_net, &iface) {
         Ok(connection_type) => connection_types.insert(connection_type),
         Err(error) => {
            warn!(message = %error.message, iface = %iface, "failed to classify supported sysfs interface");
            first_error.get_or_insert(error);
         }
      }
   }

   let connection_types = connection_types.into_vec();

   if connection_types.is_empty()
      && let Some(error) = first_error
   {
      return Err(Error::SupportedConnectionTypesDetectionFailed {
         message: error.message,
         code: error.code,
      });
   }

   Ok(connection_types)
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

#[cfg(test)]
mod tests {
   use super::*;
   use rstest::rstest;
   use std::fs::{self, File};
   use std::io::Write;
   use std::os::unix::fs as unix_fs;
   use std::path::PathBuf;
   use std::sync::atomic::{AtomicUsize, Ordering};

   // NetworkManager and ModemManager enum members, and kernel ARPHRD values,
   // that only the tests use.
   pub(super) const NM_CONNECTIVITY_UNKNOWN: u32 = 0;
   pub(super) const NM_STATE_DISCONNECTED: u32 = 20;
   const NM_STATE_CONNECTED_SITE: u32 = 60;
   pub(super) const MM_MODEM_3GPP_REGISTRATION_STATE_HOME: u32 = 1;
   const LINUX_ARPHRD_LOOPBACK: u32 = 772;

   static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

   #[test]
   fn limits_each_dbus_method_call_to_500_milliseconds() {
      assert_eq!(DBUS_METHOD_TIMEOUT, Duration::from_millis(500));
   }

   #[rstest]
   #[case::full(NM_CONNECTIVITY_FULL, ConnectedState::Connected)]
   #[case::none(NM_CONNECTIVITY_NONE, ConnectedState::Disconnected)]
   #[case::portal(NM_CONNECTIVITY_PORTAL, ConnectedState::Constrained)]
   #[case::limited(NM_CONNECTIVITY_LIMITED, ConnectedState::Constrained)]
   #[case::unknown(NM_CONNECTIVITY_UNKNOWN, ConnectedState::Unknown)]
   #[case::unrecognized(99, ConnectedState::Unknown)]
   fn maps_connectivity_states(#[case] connectivity: u32, #[case] expected: ConnectedState) {
      assert_eq!(map_connectivity(connectivity), expected);
   }

   #[rstest]
   #[case::connected_global(NM_STATE_CONNECTED_GLOBAL, ConnectedState::Connected)]
   #[case::connected_site(NM_STATE_CONNECTED_SITE, ConnectedState::Disconnected)]
   #[case::disconnected(NM_STATE_DISCONNECTED, ConnectedState::Disconnected)]
   fn resolves_unknown_connectivity_from_global_state(
      #[case] state: u32,
      #[case] expected: ConnectedState,
   ) {
      assert_eq!(connectivity_state_from_global_state(state), expected);
   }

   #[rstest]
   #[case::unknown(NM_METERED_UNKNOWN, None)]
   #[case::yes(NM_METERED_YES, Some(true))]
   #[case::no(NM_METERED_NO, Some(false))]
   #[case::guess_yes(NM_METERED_GUESS_YES, Some(true))]
   #[case::guess_no(NM_METERED_GUESS_NO, Some(false))]
   #[case::unrecognized(99, None)]
   fn maps_metered_states_without_collapsing_unknown(
      #[case] metered: u32,
      #[case] expected: Option<bool>,
   ) {
      assert_eq!(metered_status(metered), expected);
   }

   #[rstest]
   #[case::ethernet(NM_DEVICE_TYPE_ETHERNET, ConnectionType::Ethernet)]
   #[case::wifi(NM_DEVICE_TYPE_WIFI, ConnectionType::Wifi)]
   #[case::modem(NM_DEVICE_TYPE_MODEM, ConnectionType::Cellular)]
   #[case::unrecognized(999, ConnectionType::Unknown)]
   fn maps_network_manager_device_types(
      #[case] device_type: u32,
      #[case] expected: ConnectionType,
   ) {
      assert_eq!(map_device_type(device_type), expected);
   }

   #[rstest]
   #[case::wifi(ConnectionType::Wifi)]
   #[case::ethernet(ConnectionType::Ethernet)]
   #[case::unknown(ConnectionType::Unknown)]
   fn treats_non_cellular_connection_types_as_not_roaming(#[case] connection_type: ConnectionType) {
      assert_eq!(
         zbus::block_on(connection_type_roaming_status(
            connection_type,
            async || { panic!("non-cellular transport must not query ModemManager") }
         )),
         Some(false)
      );
   }

   #[test]
   fn uses_cellular_status_for_cellular_connection_type() {
      assert_eq!(
         zbus::block_on(connection_type_roaming_status(
            ConnectionType::Cellular,
            async || None
         )),
         None
      );
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
         zbus::block_on(collect_supported_connection_types_from_devices(
            devices,
            async |device| {
               match device.as_str() {
                  "/org/freedesktop/NetworkManager/Devices/1" => Ok(NM_DEVICE_TYPE_ETHERNET),
                  "/org/freedesktop/NetworkManager/Devices/2" => Err("device disappeared"),
                  "/org/freedesktop/NetworkManager/Devices/3" => Ok(NM_DEVICE_TYPE_WIFI),
                  _ => unreachable!(),
               }
            }
         ))
         .unwrap(),
         vec![ConnectionType::Wifi, ConnectionType::Ethernet]
      );
   }

   #[test]
   fn reports_failure_when_no_network_manager_device_can_be_read() {
      let devices = [
         OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/Devices/1").unwrap(),
         OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/Devices/2").unwrap(),
      ];

      assert!(
         zbus::block_on(collect_supported_connection_types_from_devices(
            devices,
            async |_| { Err::<u32, _>("device disappeared") }
         ))
         .is_err()
      );
   }

   #[test]
   fn reports_failure_when_only_unknown_network_manager_devices_can_be_read() {
      let devices = [
         OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/Devices/1").unwrap(),
         OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/Devices/2").unwrap(),
      ];

      assert!(
         zbus::block_on(collect_supported_connection_types_from_devices(
            devices,
            async |device| {
               match device.as_str() {
                  "/org/freedesktop/NetworkManager/Devices/1" => Ok(999),
                  "/org/freedesktop/NetworkManager/Devices/2" => Err("device disappeared"),
                  _ => unreachable!(),
               }
            }
         ))
         .is_err()
      );
   }

   #[rstest]
   #[case::unmetered_not_roaming(ConnectedState::Connected, Some(false), Some(false), Some(false))]
   #[case::constrained_with_unknown_signals(ConnectedState::Constrained, None, None, Some(true))]
   #[case::metered_roaming_unknown(ConnectedState::Connected, Some(true), None, Some(true))]
   #[case::metered_unknown_roaming(ConnectedState::Connected, None, Some(true), Some(true))]
   #[case::unmetered_roaming_unknown(ConnectedState::Connected, Some(false), None, None)]
   #[case::metered_unknown_roaming_unknown(ConnectedState::Connected, None, None, None)]
   fn combines_constraint_signals_without_collapsing_unknown(
      #[case] connectivity_state: ConnectedState,
      #[case] metered: Option<bool>,
      #[case] roaming: Option<bool>,
      #[case] expected: Option<bool>,
   ) {
      assert_eq!(
         constrained_status(connectivity_state, metered, roaming),
         expected
      );
   }

   #[test]
   fn global_state_fallback_is_not_constrained_when_unmetered_and_not_roaming() {
      assert_eq!(
         constrained_status(
            connectivity_state_from_global_state(NM_STATE_CONNECTED_GLOBAL),
            Some(false),
            Some(false),
         ),
         Some(false)
      );
   }

   #[test]
   fn defaults_to_unknown_connection_details() {
      let details = ConnectionDetails::default();

      assert_eq!(details.metered, None);
      assert_eq!(details.roaming, None);
      assert_eq!(details.connection_type, ConnectionType::Unknown);
   }

   #[rstest]
   #[case::roaming(MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING)]
   #[case::roaming_sms_only(MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING_SMS_ONLY)]
   #[case::roaming_csfb_not_preferred(MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING_CSFB_NOT_PREFERRED)]
   fn marks_roaming_registration_states_as_roaming_and_constrained(
      #[case] registration_state: u32,
   ) {
      let roaming = roaming_status(registration_state);

      assert_eq!(roaming, Some(true));
      assert_eq!(
         constrained_status(ConnectedState::Connected, Some(false), roaming),
         Some(true)
      );
   }

   #[rstest]
   #[case::home(MM_MODEM_3GPP_REGISTRATION_STATE_HOME, Some(false))]
   #[case::unknown(MM_MODEM_3GPP_REGISTRATION_STATE_UNKNOWN, None)]
   #[case::unrecognized(99, None)]
   fn maps_roaming_registration_states_without_collapsing_unknown(
      #[case] registration_state: u32,
      #[case] expected: Option<bool>,
   ) {
      assert_eq!(roaming_status(registration_state), expected);
   }

   #[rstest]
   #[case::all_false(&[Some(false), Some(false)], Some(false))]
   #[case::false_then_unknown(&[Some(false), None], None)]
   #[case::unknown_then_false(&[None, Some(false)], None)]
   #[case::all_unknown(&[None, None], None)]
   #[case::true_then_unknown(&[Some(true), None], Some(true))]
   #[case::unknown_then_true(&[None, Some(true)], Some(true))]
   #[case::empty(&[], None)]
   fn combines_multiple_policy_signals_conservatively(
      #[case] states: &[Option<bool>],
      #[case] expected: Option<bool>,
   ) {
      assert_eq!(combine_policy_states(states.iter().copied()), expected);
   }

   #[rstest]
   #[case::modem_manager_modem("/org/freedesktop/ModemManager1/Modem/0", true)]
   #[case::root("/", false)]
   #[case::network_manager_device("/org/freedesktop/NetworkManager/Devices/0", false)]
   fn validates_modem_manager_modem_paths(#[case] path: &str, #[case] expected: bool) {
      assert_eq!(is_modem_manager_modem_path(path), expected);
   }

   #[rstest]
   #[case::default_route_is_parsed(
      "eth0\t00000000\t015018AC\t0003\t0\t0\t0\t00000000\t0\t0\t0\n",
      Some("eth0")
   )]
   #[case::loopback_is_ignored("lo\t00000000\t00000000\t0003\t0\t0\t0\t00000000\t0\t0\t0\n", None)]
   #[case::down_route_is_ignored(
      "eth0\t00000000\t015018AC\t0002\t0\t0\t0\t00000000\t0\t0\t0\n",
      None
   )]
   #[case::no_default_route_returns_none(
      "eth0\t005018AC\t00000000\t0001\t0\t0\t0\t00F0FFFF\t0\t0\t0\n",
      None
   )]
   #[case::malformed_row_is_ignored(
      "malformed\neth0\t00000000\t015018AC\t0003\t0\t0\t0\t00000000\t0\t0\t0\n",
      Some("eth0")
   )]
   fn finds_default_ipv4_route_interface(#[case] rows: &str, #[case] expected: Option<&str>) {
      const HEADER: &str =
         "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n";
      let route_table = format!("{HEADER}{rows}");

      assert_eq!(
         default_ipv4_route_interface(&route_table).as_deref(),
         expected
      );
   }

   #[test]
   fn passive_fallback_reports_unknown_policy_flags() {
      let temp = TempDir::new();
      let iface = temp.path().join("eth0");
      fs::create_dir_all(&iface).unwrap();
      write_interface_type(&iface, LINUX_ARPHRD_ETHER);
      let route_table = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
eth0\t00000000\t015018AC\t0003\t0\t0\t0\t00000000\t0\t0\t0
";

      let status = fallback_connection_status_from_routes(route_table, "", temp.path());

      assert!(status.connected);
      assert_eq!(status.metered, None);
      assert_eq!(status.constrained, None);
      assert_eq!(status.connection_type, ConnectionType::Ethernet);
   }

   #[rstest]
   #[case::default_route_is_parsed(
      "00000000000000000000000000000000 00 00000000000000000000000000000000 00 \
       fe800000000000000000000000000001 00000400 00000000 00000000 00000003 eth0\n",
      Some("eth0")
   )]
   #[case::loopback_is_ignored(
      "00000000000000000000000000000000 00 00000000000000000000000000000000 00 \
       00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo\n",
      None
   )]
   #[case::down_route_is_ignored(
      "00000000000000000000000000000000 00 00000000000000000000000000000000 00 \
       fe800000000000000000000000000001 00000400 00000000 00000000 00000002 eth0\n",
      None
   )]
   #[case::no_default_route_returns_none(
      "fe800000000000000000000000000000 40 00000000000000000000000000000000 00 \
       00000000000000000000000000000000 00000100 00000001 00000000 00000001 eth0\n",
      None
   )]
   #[case::malformed_row_is_ignored(
      "malformed\n\
       00000000000000000000000000000000 00 00000000000000000000000000000000 00 \
       fe800000000000000000000000000001 00000400 00000000 00000000 00000003 eth0\n",
      Some("eth0")
   )]
   fn finds_default_ipv6_route_interface(
      #[case] route_table: &str,
      #[case] expected: Option<&str>,
   ) {
      assert_eq!(
         default_ipv6_route_interface(route_table).as_deref(),
         expected
      );
   }

   #[rstest]
   #[case::wireless_directory_is_wifi("wireless", ConnectionType::Wifi)]
   #[case::ieee80211_marker_is_wifi("ieee80211", ConnectionType::Wifi)]
   #[case::phy80211_marker_is_wifi("phy80211", ConnectionType::Wifi)]
   #[case::wwan_marker_is_cellular("device/wwan", ConnectionType::Cellular)]
   fn infers_transport_from_sysfs_marker_directory(
      #[case] marker: &str,
      #[case] expected: ConnectionType,
   ) {
      let temp = TempDir::new();
      let iface = temp.path().join("net0");
      fs::create_dir_all(iface.join(marker)).unwrap();

      assert_eq!(infer_transport_from_sysfs(temp.path(), "net0"), expected);
   }

   #[test]
   fn does_not_infer_wifi_from_80211_path_fragment() {
      let temp = TempDir::new();
      let sys_class_net = temp.path().join("not80211-device");
      let iface = sys_class_net.join("net0");
      fs::create_dir_all(&iface).unwrap();
      write_interface_type(&iface, LINUX_ARPHRD_ETHER);

      assert_eq!(
         infer_transport_from_sysfs(&sys_class_net, "net0"),
         ConnectionType::Ethernet
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
      write_interface_type(&iface, LINUX_ARPHRD_ETHER);

      assert_eq!(
         infer_transport_from_sysfs(temp.path(), "net0"),
         ConnectionType::Ethernet
      );
   }

   #[rstest]
   #[case::arphrd_ether_type_is_ethernet(LINUX_ARPHRD_ETHER, ConnectionType::Ethernet)]
   #[case::no_transport_signal_is_unknown(LINUX_ARPHRD_LOOPBACK, ConnectionType::Unknown)]
   fn infers_transport_from_sysfs_interface_type(
      #[case] interface_type: u32,
      #[case] expected: ConnectionType,
   ) {
      let temp = TempDir::new();
      let iface = temp.path().join("net0");
      fs::create_dir_all(&iface).unwrap();
      write_interface_type(&iface, interface_type);

      assert_eq!(infer_transport_from_sysfs(temp.path(), "net0"), expected);
   }

   #[test]
   fn supported_types_from_sysfs_ignores_virtual_interfaces() {
      let temp = TempDir::new();
      let physical = temp.path().join("enp0s1");
      let virtual_iface = temp.path().join("virtual").join("net").join("veth0");
      fs::create_dir_all(physical.join("wireless")).unwrap();
      fs::create_dir_all(&virtual_iface).unwrap();
      write_interface_type(&virtual_iface, LINUX_ARPHRD_ETHER);
      unix_fs::symlink(&virtual_iface, temp.path().join("veth0")).unwrap();

      assert_eq!(
         supported_types_from_sysfs(temp.path()).unwrap(),
         vec![ConnectionType::Wifi]
      );
   }

   #[test]
   fn supported_types_from_sysfs_returns_empty_without_transport_signal() {
      let temp = TempDir::new();
      let iface = temp.path().join("net0");
      fs::create_dir_all(&iface).unwrap();
      write_interface_type(&iface, LINUX_ARPHRD_LOOPBACK);

      assert_eq!(
         supported_types_from_sysfs(temp.path()).unwrap(),
         Vec::<ConnectionType>::new()
      );
   }

   #[test]
   fn supported_types_from_sysfs_reports_interface_type_failure() {
      let temp = TempDir::new();
      let iface = temp.path().join("net0");
      fs::create_dir_all(&iface).unwrap();
      write_file(iface.join("type"), "not-an-interface-type\n");

      assert!(matches!(
         supported_types_from_sysfs(temp.path()),
         Err(Error::SupportedConnectionTypesDetectionFailed { .. })
      ));
   }

   #[test]
   fn supported_types_from_sysfs_reports_missing_interface_type() {
      let temp = TempDir::new();
      fs::create_dir_all(temp.path().join("net0")).unwrap();

      assert!(matches!(
         supported_types_from_sysfs(temp.path()),
         Err(Error::SupportedConnectionTypesDetectionFailed { .. })
      ));
   }

   #[test]
   fn supported_types_from_sysfs_returns_partial_results_despite_failure() {
      let temp = TempDir::new();
      let wifi = temp.path().join("wlp0s20f3");
      let unreadable = temp.path().join("net0");
      fs::create_dir_all(wifi.join("wireless")).unwrap();
      fs::create_dir_all(unreadable).unwrap();

      assert_eq!(
         supported_types_from_sysfs(temp.path()).unwrap(),
         vec![ConnectionType::Wifi]
      );
   }

   #[test]
   fn supported_types_from_sysfs_reports_enumeration_failure() {
      let temp = TempDir::new();
      let missing_path = temp.path().join("missing");

      assert!(supported_types_from_sysfs(&missing_path).is_err());
   }

   fn write_file(path: impl AsRef<Path>, contents: &str) {
      let mut file = File::create(path).unwrap();
      file.write_all(contents.as_bytes()).unwrap();
   }

   fn write_interface_type(iface: &Path, interface_type: u32) {
      write_file(iface.join("type"), &format!("{interface_type}\n"));
   }

   pub(super) struct TempDir {
      path: PathBuf,
   }

   impl TempDir {
      pub(super) fn new() -> Self {
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

      pub(super) fn path(&self) -> &Path {
         &self.path
      }
   }

   impl Drop for TempDir {
      fn drop(&mut self) {
         let _ = fs::remove_dir_all(&self.path);
      }
   }
}

#[cfg(test)]
mod dbus_tests {
   use super::tests::{
      MM_MODEM_3GPP_REGISTRATION_STATE_HOME, NM_CONNECTIVITY_UNKNOWN, NM_STATE_DISCONNECTED,
      TempDir,
   };
   use super::*;
   use rstest::rstest;
   use std::io::{BufRead, BufReader};
   use std::os::unix::fs::FileTypeExt;
   use std::path::PathBuf;
   use std::process::{Child, Command, Stdio};
   use std::sync::Arc;
   use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
   use std::time::Instant;

   const ACTIVE_PATH: &str = "/org/freedesktop/NetworkManager/ActiveConnection/1";
   const DEVICE_PATH: &str = "/org/freedesktop/NetworkManager/Devices/1";
   const BAD_DEVICE_PATH: &str = "/org/freedesktop/NetworkManager/Devices/2";
   const MODEM_PATH: &str = "/org/freedesktop/ModemManager1/Modem/0";

   struct PrivateBus {
      child: Child,
      address: String,
      directory: TempDir,
   }

   impl PrivateBus {
      fn new() -> Self {
         let directory = TempDir::new();
         let child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .arg(format!("--address=unix:dir={}", directory.path().display()))
            .stdout(Stdio::piped())
            .spawn()
            .expect("Linux D-Bus tests require dbus-daemon");
         // Establish ownership before any fallible setup so a failed test also
         // kills and reaps its daemon.
         let mut bus = Self {
            child,
            address: String::new(),
            directory,
         };
         BufReader::new(bus.child.stdout.take().unwrap())
            .read_line(&mut bus.address)
            .unwrap();
         bus.address = bus.address.trim().to_owned();
         assert!(!bus.address.is_empty(), "private D-Bus failed to start");
         bus
      }

      async fn connection(&self) -> Connection {
         // Use the production timeout configuration on this private bus too.
         let connection = bus_connection(ConnectionBuilder::address(self.address.as_str()))
            .await
            .unwrap();
         assert_eq!(
            connection.method_timeout(),
            Some(Duration::from_millis(500))
         );
         connection
      }
   }

   impl Drop for PrivateBus {
      fn drop(&mut self) {
         let _ = self.child.kill();
         let _ = self.child.wait();
      }
   }

   struct ServiceState {
      connectivity: AtomicU32,
      global_state: AtomicU32,
      device_type: AtomicU32,
      metered: AtomicU32,
      registration: AtomicU32,
      stall: AtomicBool,
      unreadable_metered: AtomicBool,
   }

   impl Default for ServiceState {
      fn default() -> Self {
         Self {
            connectivity: AtomicU32::new(NM_CONNECTIVITY_FULL),
            global_state: AtomicU32::new(NM_STATE_CONNECTED_GLOBAL),
            device_type: AtomicU32::new(NM_DEVICE_TYPE_WIFI),
            metered: AtomicU32::new(NM_METERED_NO),
            registration: AtomicU32::new(MM_MODEM_3GPP_REGISTRATION_STATE_HOME),
            stall: AtomicBool::new(false),
            unreadable_metered: AtomicBool::new(false),
         }
      }
   }

   struct Manager(Arc<ServiceState>);

   impl Manager {
      async fn delay_if_stalled(&self) {
         if self.0.stall.load(Ordering::SeqCst) {
            // Eventually reply so a missing method timeout fails the assertion
            // instead of leaving the test suite waiting forever.
            zruntime::SharedRuntime::current()
               .unwrap()
               .sleep(Duration::from_secs(2))
               .await;
         }
      }
   }

   #[zbus::interface(name = "org.freedesktop.NetworkManager")]
   impl Manager {
      #[zbus(property)]
      async fn connectivity(&self) -> u32 {
         self.delay_if_stalled().await;
         self.0.connectivity.load(Ordering::SeqCst)
      }

      #[zbus(property)]
      fn state(&self) -> u32 {
         self.0.global_state.load(Ordering::SeqCst)
      }

      #[zbus(property)]
      fn primary_connection(&self) -> OwnedObjectPath {
         object_path(ACTIVE_PATH)
      }

      #[zbus(property)]
      async fn devices(&self) -> Vec<OwnedObjectPath> {
         self.delay_if_stalled().await;
         vec![object_path(DEVICE_PATH), object_path(BAD_DEVICE_PATH)]
      }
   }

   struct WrongManager;

   #[zbus::interface(name = "org.freedesktop.NetworkManager")]
   impl WrongManager {
      #[zbus(property)]
      fn connectivity(&self) -> String {
         "invalid property signature".to_owned()
      }

      #[zbus(property)]
      fn devices(&self) -> String {
         "invalid property signature".to_owned()
      }
   }

   struct ActiveConnection;

   #[zbus::interface(name = "org.freedesktop.NetworkManager.Connection.Active")]
   impl ActiveConnection {
      #[zbus(property)]
      fn devices(&self) -> Vec<OwnedObjectPath> {
         vec![object_path(DEVICE_PATH)]
      }
   }

   struct Device(Arc<ServiceState>);

   #[zbus::interface(name = "org.freedesktop.NetworkManager.Device")]
   impl Device {
      #[zbus(property)]
      fn device_type(&self) -> u32 {
         self.0.device_type.load(Ordering::SeqCst)
      }

      #[zbus(property)]
      fn metered(&self) -> zbus::fdo::Result<u32> {
         if self.0.unreadable_metered.load(Ordering::SeqCst) {
            Err(zbus::fdo::Error::Failed("metering unavailable".to_owned()))
         } else {
            Ok(self.0.metered.load(Ordering::SeqCst))
         }
      }

      #[zbus(property)]
      fn udi(&self) -> String {
         MODEM_PATH.to_owned()
      }
   }

   struct WrongDevice;

   #[zbus::interface(name = "org.freedesktop.NetworkManager.Device")]
   impl WrongDevice {
      #[zbus(property)]
      fn device_type(&self) -> String {
         "invalid property signature".to_owned()
      }
   }

   struct Modem(Arc<ServiceState>);

   #[zbus::interface(name = "org.freedesktop.ModemManager1.Modem.Modem3gpp")]
   impl Modem {
      #[zbus(property)]
      fn registration_state(&self) -> u32 {
         self.0.registration.load(Ordering::SeqCst)
      }
   }

   const IPV4_ROUTE: &str = "Iface Destination Gateway Flags\neth0 00000000 0100000A 0003\n";
   const IPV6_ROUTE: &str = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 \
       fe800000000000000000000000000001 00000400 00000000 00000000 00000003 eth0\n";

   struct FallbackFixture {
      _directory: TempDir,
      ipv4_routes: PathBuf,
      ipv6_routes: PathBuf,
      sys_class_net: PathBuf,
   }

   impl FallbackFixture {
      fn new() -> Self {
         let directory = TempDir::new();
         let ipv4_routes = directory.path().join("route");
         let ipv6_routes = directory.path().join("ipv6_route");
         let sys_class_net = directory.path().join("net");
         fs::create_dir_all(sys_class_net.join("eth0")).unwrap();
         fs::write(sys_class_net.join("eth0/type"), "1\n").unwrap();
         fs::write(&ipv4_routes, IPV4_ROUTE).unwrap();
         fs::write(&ipv6_routes, "").unwrap();
         Self {
            _directory: directory,
            ipv4_routes,
            ipv6_routes,
            sys_class_net,
         }
      }

      fn paths(&self) -> FallbackPaths<'_> {
         FallbackPaths {
            ipv4_routes: &self.ipv4_routes,
            ipv6_routes: &self.ipv6_routes,
            sys_class_net: &self.sys_class_net,
         }
      }
   }

   fn assert_passive_fallback(status: &ConnectionStatus, connected: bool) {
      let expected = if connected {
         ConnectionStatus {
            connected: true,
            metered: None,
            constrained: None,
            connection_type: ConnectionType::Ethernet,
         }
      } else {
         ConnectionStatus::disconnected()
      };
      assert_eq!(*status, expected);
   }

   fn object_path(path: &str) -> OwnedObjectPath {
      OwnedObjectPath::try_from(path).unwrap()
   }

   async fn network_manager(bus: &PrivateBus, state: &Arc<ServiceState>) -> Connection {
      ConnectionBuilder::address(bus.address.as_str())
         .name(NETWORK_MANAGER_SERVICE)
         .serve_at(NETWORK_MANAGER_PATH, Manager(Arc::clone(state)))
         .serve_at(ACTIVE_PATH, ActiveConnection)
         .serve_at(DEVICE_PATH, Device(Arc::clone(state)))
         .serve_at(BAD_DEVICE_PATH, WrongDevice)
         .build()
         .await
         .unwrap()
   }

   #[test]
   fn reads_network_manager_status_and_partial_supported_transports() {
      let bus = PrivateBus::new();
      let fallback = FallbackFixture::new();
      let fallback_paths = fallback.paths();
      zbus::block_on(async {
         let state = Arc::new(ServiceState::default());
         let _service = network_manager(&bus, &state).await;
         let connection = bus.connection().await;
         let status = connection_status_from_connection(Ok(connection.clone()), &fallback_paths)
            .await
            .unwrap();
         assert_eq!(
            status,
            ConnectionStatus {
               connected: true,
               metered: Some(false),
               constrained: Some(false),
               connection_type: ConnectionType::Wifi,
            }
         );
         assert_eq!(
            supported_connection_types_from_connection(Ok(connection), &fallback_paths)
               .await
               .unwrap(),
            vec![ConnectionType::Wifi]
         );
      });
   }

   #[test]
   fn reads_cellular_roaming_and_preserves_unknown_policy_states() {
      let bus = PrivateBus::new();
      zbus::block_on(async {
         let state = Arc::new(ServiceState::default());
         state
            .device_type
            .store(NM_DEVICE_TYPE_MODEM, Ordering::SeqCst);
         let _manager = network_manager(&bus, &state).await;
         let connection = bus.connection().await;
         // A missing ModemManager must leave roaming unknown, even if unmetered.
         let status = network_manager_connection_status(&connection)
            .await
            .unwrap();
         assert_eq!(status.connection_type, ConnectionType::Cellular);
         assert_eq!(status.constrained, None);
         let _modem = ConnectionBuilder::address(bus.address.as_str())
            .name(MODEM_MANAGER_SERVICE)
            .serve_at(MODEM_PATH, Modem(Arc::clone(&state)))
            .build()
            .await
            .unwrap();
         for (registration, expected) in [
            (MM_MODEM_3GPP_REGISTRATION_STATE_HOME, Some(false)),
            (MM_MODEM_3GPP_REGISTRATION_STATE_ROAMING, Some(true)),
            (MM_MODEM_3GPP_REGISTRATION_STATE_UNKNOWN, None),
            (99, None),
         ] {
            state.registration.store(registration, Ordering::SeqCst);
            assert_eq!(
               network_manager_connection_status(&connection)
                  .await
                  .unwrap()
                  .constrained,
               expected
            );
         }
         state.unreadable_metered.store(true, Ordering::SeqCst);
         let status = network_manager_connection_status(&connection)
            .await
            .unwrap();
         assert_eq!(status.metered, None);
         assert_eq!(status.constrained, None);
      });
   }

   #[test]
   fn rereads_properties_and_resolves_unknown_connectivity_from_global_state() {
      let bus = PrivateBus::new();
      zbus::block_on(async {
         let state = Arc::new(ServiceState::default());
         let _manager = network_manager(&bus, &state).await;
         let connection = bus.connection().await;
         let proxy = dbus_proxy(
            &connection,
            NETWORK_MANAGER_SERVICE,
            NETWORK_MANAGER_PATH,
            NETWORK_MANAGER_INTERFACE,
         )
         .await
         .unwrap();
         for connectivity in [
            NM_CONNECTIVITY_FULL,
            NM_CONNECTIVITY_PORTAL,
            NM_CONNECTIVITY_NONE,
            NM_CONNECTIVITY_FULL,
         ] {
            state.connectivity.store(connectivity, Ordering::SeqCst);
            // No PropertiesChanged signal is emitted: this must read the bus.
            assert_eq!(
               proxy.get_property::<u32>("Connectivity").await.unwrap(),
               connectivity
            );
            let status = network_manager_connection_status(&connection)
               .await
               .unwrap();
            assert_eq!(status.connected, connectivity != NM_CONNECTIVITY_NONE);
            if status.connected {
               assert_eq!(
                  status.constrained,
                  Some(connectivity == NM_CONNECTIVITY_PORTAL)
               );
            }
         }
         for metered in [NM_METERED_YES, NM_METERED_NO, NM_METERED_UNKNOWN] {
            state.metered.store(metered, Ordering::SeqCst);
            let status = network_manager_connection_status(&connection)
               .await
               .unwrap();
            let expected = metered_status(metered);
            assert_eq!(status.metered, expected);
            assert_eq!(status.constrained, expected);
         }
         state.metered.store(NM_METERED_NO, Ordering::SeqCst);
         state
            .connectivity
            .store(NM_CONNECTIVITY_UNKNOWN, Ordering::SeqCst);
         assert!(
            network_manager_connection_status(&connection)
               .await
               .unwrap()
               .connected
         );
         state
            .global_state
            .store(NM_STATE_DISCONNECTED, Ordering::SeqCst);
         assert_eq!(
            network_manager_connection_status(&connection)
               .await
               .unwrap(),
            ConnectionStatus::disconnected()
         );
      });
   }

   #[rstest]
   #[case::ipv4(IPV4_ROUTE, "", true)]
   #[case::ipv6("", IPV6_ROUTE, true)]
   #[case::offline("", "", false)]
   fn missing_services_and_connection_failures_use_passive_fallbacks(
      #[case] ipv4_routes: &str,
      #[case] ipv6_routes: &str,
      #[case] connected: bool,
   ) {
      let bus = PrivateBus::new();
      let fallback = FallbackFixture::new();
      fs::write(&fallback.ipv4_routes, ipv4_routes).unwrap();
      fs::write(&fallback.ipv6_routes, ipv6_routes).unwrap();
      let fallback_paths = fallback.paths();
      zbus::block_on(async {
         let connection = bus.connection().await;
         assert!(
            !service_has_owner(&connection, NETWORK_MANAGER_SERVICE)
               .await
               .unwrap()
         );
         for connection in [
            Ok(connection),
            Err(zbus::Error::Failure("connection unavailable".to_owned())),
         ] {
            let status = connection_status_from_connection(connection.clone(), &fallback_paths)
               .await
               .unwrap();
            assert_passive_fallback(&status, connected);
            assert_eq!(
               supported_connection_types_from_connection(connection, &fallback_paths)
                  .await
                  .map_err(|e| e.to_string()),
               Ok(vec![ConnectionType::Ethernet])
            );
         }
      });
   }

   #[test]
   fn invalid_property_signatures_propagate_to_fallbacks() {
      let bus = PrivateBus::new();
      let fallback = FallbackFixture::new();
      let fallback_paths = fallback.paths();
      zbus::block_on(async {
         let _service = ConnectionBuilder::address(bus.address.as_str())
            .name(NETWORK_MANAGER_SERVICE)
            .serve_at(NETWORK_MANAGER_PATH, WrongManager)
            .build()
            .await
            .unwrap();
         let connection = bus.connection().await;
         assert!(matches!(
            network_manager_connection_status(&connection).await,
            Err(zbus::Error::SignatureMismatch(..))
         ));
         assert!(
            network_manager_supported_connection_types(&connection)
               .await
               .is_err()
         );
         let status = connection_status_from_connection(Ok(connection.clone()), &fallback_paths)
            .await
            .unwrap();
         assert_passive_fallback(&status, true);
         assert_eq!(
            supported_connection_types_from_connection(Ok(connection), &fallback_paths)
               .await
               .map_err(|e| e.to_string()),
            Ok(vec![ConnectionType::Ethernet])
         );
      });
   }

   #[test]
   fn stalled_property_reads_time_out_and_use_the_passive_fallback() {
      let bus = PrivateBus::new();
      let fallback = FallbackFixture::new();
      let fallback_paths = fallback.paths();
      zbus::block_on(async {
         let state = Arc::new(ServiceState::default());
         state.stall.store(true, Ordering::SeqCst);
         let _service = network_manager(&bus, &state).await;
         let connection = bus.connection().await;
         let started = Instant::now();
         assert!(matches!(
            network_manager_connection_status(&connection).await,
            Err(zbus::Error::InputOutput(error)) if error.kind() == std::io::ErrorKind::TimedOut
         ));
         assert!(started.elapsed() >= DBUS_METHOD_TIMEOUT);
         assert!(started.elapsed() < Duration::from_secs(5));
         let started = Instant::now();
         let status = connection_status_from_connection(Ok(connection), &fallback_paths)
            .await
            .unwrap();
         assert_passive_fallback(&status, true);
         assert!(started.elapsed() < Duration::from_secs(5));
         let connection = bus.connection().await;
         let started = Instant::now();
         assert_eq!(
            supported_connection_types_from_connection(Ok(connection), &fallback_paths)
               .await
               .map_err(|e| e.to_string()),
            Ok(vec![ConnectionType::Ethernet])
         );
         assert!(started.elapsed() >= DBUS_METHOD_TIMEOUT);
         assert!(started.elapsed() < Duration::from_secs(5));
      });
   }

   #[test]
   fn repeated_and_concurrent_queries_work_on_independent_calling_threads() {
      let bus = PrivateBus::new();
      let fallback = FallbackFixture::new();
      let fallback_paths = fallback.paths();
      let state = Arc::new(ServiceState::default());
      let _service = zbus::block_on(network_manager(&bus, &state));
      std::thread::scope(|scope| {
         let handles: Vec<_> = (0..4)
            .map(|_| {
               scope.spawn(|| {
                  for _ in 0..3 {
                     zbus::block_on(async {
                        let connection = bus.connection().await;
                        let status = connection_status_from_connection(
                           Ok(connection.clone()),
                           &fallback_paths,
                        )
                        .await
                        .unwrap();
                        assert!(status.connected);
                        assert_eq!(status.connection_type, ConnectionType::Wifi);
                        assert_eq!(
                           supported_connection_types_from_connection(
                              Ok(connection),
                              &fallback_paths
                           )
                           .await
                           .unwrap(),
                           vec![ConnectionType::Wifi]
                        );
                     });
                  }
               })
            })
            .collect();
         for handle in handles {
            handle.join().unwrap();
         }
      });
   }

   #[test]
   fn removes_private_bus_socket_directory_on_drop() {
      let directory = {
         let bus = PrivateBus::new();
         let directory = bus.directory.path().to_owned();
         assert!(
            fs::read_dir(&directory).unwrap().any(|entry| entry
               .unwrap()
               .file_type()
               .unwrap()
               .is_socket())
         );
         directory
      };
      assert!(!directory.exists());
   }
}
