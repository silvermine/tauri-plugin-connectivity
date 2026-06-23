import Foundation
import Network
import Tauri

private enum ConnectionTypePayload: String, Encodable {
   case wifi
   case ethernet
   case cellular
   case unknown
}

private struct ConnectionStatusPayload: Encodable {
   let connected: Bool
   let metered: Bool
   let constrained: Bool
   let connectionType: ConnectionTypePayload
}

class ConnectivityPlugin: Plugin {
   private let monitor = NWPathMonitor()
   private let monitorQueue = DispatchQueue(label: "tauri.plugin.connectivity.path")
   private let stateQueue = DispatchQueue(label: "tauri.plugin.connectivity.state")
   private var latestPath: NWPath?
   private let firstPathSemaphore = DispatchSemaphore(value: 0)
   private var hasSignalledFirstPath = false

   // Upper bound on how long the first connectionStatus() call waits for the
   // initial NWPathMonitor update before falling back to monitor.currentPath.
   private static let firstPathTimeout: DispatchTimeInterval = .milliseconds(200)

   override init() {
      super.init()
      monitor.pathUpdateHandler = { [weak self] path in
         guard let self else { return }
         self.stateQueue.async {
            self.latestPath = path
            if !self.hasSignalledFirstPath {
               self.hasSignalledFirstPath = true
               self.firstPathSemaphore.signal()
            }
         }
      }
      monitor.start(queue: monitorQueue)
   }

   deinit {
      monitor.cancel()
   }

   @objc public func connectionStatus(_ invoke: Invoke) throws {
      // The first pathUpdateHandler callback is delivered asynchronously after
      // start(), so on an early call latestPath may still be nil. Briefly wait
      // for that first update rather than immediately falling back to
      // `monitor.currentPath`, which may report `.requiresConnection` in that
      // window and under-report connectivity. The wait is bounded so the
      // calling thread never blocks indefinitely.
      if stateQueue.sync(execute: { latestPath }) == nil {
         _ = firstPathSemaphore.wait(timeout: .now() + Self.firstPathTimeout)
      }
      let path = stateQueue.sync { latestPath } ?? monitor.currentPath
      let connectionType = Self.resolveConnectionType(path)

      invoke.resolve(ConnectionStatusPayload(
         connected: path.status == .satisfied,
         metered: path.isExpensive,
         constrained: path.isConstrained,
         connectionType: connectionType
      ))
   }

   private static func resolveConnectionType(_ path: NWPath) -> ConnectionTypePayload {
      if path.usesInterfaceType(.wifi) {
         return .wifi
      } else if path.usesInterfaceType(.wiredEthernet) {
         return .ethernet
      } else if path.usesInterfaceType(.cellular) {
         return .cellular
      }
      return .unknown
   }
}

@_cdecl("init_plugin_connectivity")
func initPlugin() -> Plugin {
   return ConnectivityPlugin()
}
