import { invoke } from '@tauri-apps/api/core';

/**
 * Describes the physical or logical transport used to connect to the network.
 *
 * When multiple interfaces are active simultaneously (e.g. WiFi + Cellular),
 * this represents the preferred/primary transport as determined by the OS.
 */
export type ConnectionType = 'wifi' | 'ethernet' | 'cellular' | 'unknown';

/**
 * Information about the current network connection.
 *
 * Combines reachability, cost/constraint flags, and the physical connection type
 * to give callers enough context to make network policy decisions.
 */
export interface ConnectionStatus {

   /** Whether the device has an active internet connection. */
   connected: boolean;

   /**
    * Whether data usage is billed or limited (e.g. mobile data plans, capped
    * hotspots).
    *
    * Platform mapping:
    * - **Windows:** `NetworkCostType` is `Unknown`, `Fixed`, or `Variable`
    * - **iOS:** `NWPath.isExpensive`
    * - **Android:** absence of `NET_CAPABILITY_NOT_METERED`
    */
   metered: boolean;

   /**
    * Whether the connection is constrained — approaching or over its data limit,
    * roaming, or background data usage is restricted.
    *
    * Platform mapping:
    * - **Windows:** `ConstrainedInternetAccess`, `ApproachingDataLimit`,
    *   `OverDataLimit`, `Roaming`, or `BackgroundDataUsageRestricted`
    * - **iOS:** `NWPath.isConstrained` (Low Data Mode)
    * - **Android:** Data Saver / `RESTRICT_BACKGROUND_STATUS`
    */
   constrained: boolean;

   /**
    * The physical or logical transport used to connect to the network. When
    * `connected` is `false`, this will be `'unknown'`.
    */
   connectionType: ConnectionType;
}

/**
 * Returns the current network connection status.
 *
 * @returns A promise that resolves with the current {@link ConnectionStatus}.
 * @throws Rejects with a string error when the platform is unsupported or when
 * native status detection fails. Unsupported platforms use the message
 * `connection status detection is not supported on this platform`; backend
 * failures use `connection status detection failed: ...` or
 * `connection status detection failed with native error code <code>: ...`.
 */
export async function connectionStatus(): Promise<ConnectionStatus> {
   return invoke<ConnectionStatus>('plugin:connectivity|connection_status');
}
