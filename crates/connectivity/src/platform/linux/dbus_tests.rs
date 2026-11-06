use super::tests::TempDir;
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
         registration: AtomicU32::new(1),
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
      for (registration, expected) in [(1, Some(false)), (5, Some(true)), (4, None), (99, None)] {
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
      state.connectivity.store(0, Ordering::SeqCst);
      assert!(
         network_manager_connection_status(&connection)
            .await
            .unwrap()
            .connected
      );
      state.global_state.store(20, Ordering::SeqCst);
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
                     let status =
                        connection_status_from_connection(Ok(connection.clone()), &fallback_paths)
                           .await
                           .unwrap();
                     assert!(status.connected);
                     assert_eq!(status.connection_type, ConnectionType::Wifi);
                     assert_eq!(
                        supported_connection_types_from_connection(Ok(connection), &fallback_paths)
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
