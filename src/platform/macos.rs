use std::ffi::c_void;
use std::sync::{Arc, OnceLock, RwLock};

use block2::{Block, RcBlock};
use dispatch2::{DispatchQueue, DispatchRetained};

use crate::error::{Error, Result};
use crate::types::{ConnectionStatus, ConnectionType};

const NW_PATH_STATUS_SATISFIED: i32 = 1;
const NW_INTERFACE_TYPE_WIFI: i32 = 1;
const NW_INTERFACE_TYPE_CELLULAR: i32 = 2;
const NW_INTERFACE_TYPE_WIRED: i32 = 3;

type NwPath = *mut c_void;
type NwPathMonitor = *mut c_void;

#[link(name = "Network", kind = "framework")]
unsafe extern "C" {
   fn nw_path_monitor_create() -> NwPathMonitor;
   fn nw_path_monitor_set_queue(monitor: NwPathMonitor, queue: &DispatchQueue);
   fn nw_path_monitor_set_update_handler(
      monitor: NwPathMonitor,
      update_handler: &Block<dyn Fn(NwPath)>,
   );
   fn nw_path_monitor_start(monitor: NwPathMonitor);
   fn nw_path_get_status(path: NwPath) -> i32;
   fn nw_path_is_expensive(path: NwPath) -> bool;
   fn nw_path_is_constrained(path: NwPath) -> bool;
   fn nw_path_uses_interface_type(path: NwPath, interface_type: i32) -> bool;
}

static MONITOR: OnceLock<Option<MacosConnectivityMonitor>> = OnceLock::new();

struct MacosConnectivityMonitor {
   _monitor: NwPathMonitor,
   _queue: DispatchRetained<DispatchQueue>,
   _handler: RcBlock<dyn Fn(NwPath)>,
   status: Arc<RwLock<Option<ConnectionStatus>>>,
}

unsafe impl Send for MacosConnectivityMonitor {}
unsafe impl Sync for MacosConnectivityMonitor {}

pub fn connection_status() -> Result<ConnectionStatus> {
   let monitor = MONITOR.get_or_init(create_monitor);

   monitor
      .as_ref()
      .map(MacosConnectivityMonitor::current_status)
      .ok_or_else(|| Error::DetectionFailed {
         message: String::from("failed to create macOS path monitor"),
         code: None,
      })
}

fn create_monitor() -> Option<MacosConnectivityMonitor> {
   let monitor = unsafe { nw_path_monitor_create() };
   if monitor.is_null() {
      return None;
   }

   let queue = DispatchQueue::new("tauri.plugin.connectivity.path", None);
   let status = Arc::new(RwLock::new(None));
   let handler_status = Arc::clone(&status);
   let handler = RcBlock::new(move |path: NwPath| {
      if let Ok(mut status) = handler_status.write() {
         *status = Some(read_status(path));
      }
   });

   unsafe {
      nw_path_monitor_set_queue(monitor, &queue);
      nw_path_monitor_set_update_handler(monitor, &handler);
      nw_path_monitor_start(monitor);
   }

   Some(MacosConnectivityMonitor {
      _monitor: monitor,
      _queue: queue,
      _handler: handler,
      status,
   })
}

impl MacosConnectivityMonitor {
   fn current_status(&self) -> ConnectionStatus {
      self
         .status
         .read()
         .map(|status| cached_status(status.clone()))
         .unwrap_or_else(|_| ConnectionStatus::disconnected())
   }
}

fn cached_status(status: Option<ConnectionStatus>) -> ConnectionStatus {
   status.unwrap_or_else(ConnectionStatus::disconnected)
}

fn resolve_connection_type(wifi: bool, wired: bool, cellular: bool) -> ConnectionType {
   if wifi {
      ConnectionType::Wifi
   } else if wired {
      ConnectionType::Ethernet
   } else if cellular {
      ConnectionType::Cellular
   } else {
      ConnectionType::Unknown
   }
}

/// Apple reports path availability, not whether a specific request will succeed.
///
/// We only treat `nw_path_status_satisfied` as `connected = true`, because that
/// is the state where the path is currently usable for sending and receiving
/// data.
///
/// We still treat `nw_path_status_satisfiable` as disconnected. Apple describes
/// that state as a path that is not currently available, even though a future
/// connection attempt may activate it.
///
/// References:
/// - `nw_path_status_satisfied`: <https://developer.apple.com/documentation/network/nw_path_status_satisfied>
/// - `nw_path_status_satisfiable`: <https://developer.apple.com/documentation/network/nw_path_status_satisfiable>
/// - `nw_path_status_t`: <https://developer.apple.com/documentation/network/nw_path_status_t>
fn is_connected_status(status: i32) -> bool {
   status == NW_PATH_STATUS_SATISFIED
}

fn read_status(path: NwPath) -> ConnectionStatus {
   let connected = is_connected_status(unsafe { nw_path_get_status(path) });
   if !connected {
      return ConnectionStatus::disconnected();
   }

   let wifi = unsafe { nw_path_uses_interface_type(path, NW_INTERFACE_TYPE_WIFI) };
   let wired = unsafe { nw_path_uses_interface_type(path, NW_INTERFACE_TYPE_WIRED) };
   let cellular = unsafe { nw_path_uses_interface_type(path, NW_INTERFACE_TYPE_CELLULAR) };

   ConnectionStatus {
      connected: true,
      metered: unsafe { nw_path_is_expensive(path) },
      constrained: unsafe { nw_path_is_constrained(path) },
      connection_type: resolve_connection_type(wifi, wired, cellular),
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn maps_interface_flags_by_priority() {
      assert_eq!(
         resolve_connection_type(true, true, true),
         ConnectionType::Wifi
      );
      assert_eq!(
         resolve_connection_type(false, true, true),
         ConnectionType::Ethernet
      );
      assert_eq!(
         resolve_connection_type(false, false, true),
         ConnectionType::Cellular
      );
      assert_eq!(
         resolve_connection_type(false, false, false),
         ConnectionType::Unknown
      );
   }

   #[test]
   fn disconnected_status_has_unknown_connection_type() {
      assert_eq!(
         ConnectionStatus::disconnected().connection_type,
         ConnectionType::Unknown
      );
   }

   #[test]
   fn missing_cached_status_defaults_to_disconnected() {
      assert_eq!(cached_status(None), ConnectionStatus::disconnected());
   }
}
