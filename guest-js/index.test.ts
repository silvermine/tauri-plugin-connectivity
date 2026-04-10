import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { connectionStatus, ConnectionStatus } from './index';

let lastCmd = '';

const CONNECTED_WIFI: ConnectionStatus = {
   connected: true,
   metered: false,
   constrained: false,
   connectionType: 'wifi',
};

const CONNECTED_CELLULAR: ConnectionStatus = {
   connected: true,
   metered: true,
   constrained: false,
   connectionType: 'cellular',
};

const CONNECTED_ETHERNET: ConnectionStatus = {
   connected: true,
   metered: false,
   constrained: false,
   connectionType: 'ethernet',
};

const DISCONNECTED: ConnectionStatus = {
   connected: false,
   metered: false,
   constrained: false,
   connectionType: 'unknown',
};

beforeEach(() => {
   mockIPC((cmd) => {
      lastCmd = cmd;

      if (cmd === 'plugin:connectivity|connection_status') {
         return CONNECTED_WIFI;
      }
      return undefined;
   });
});

afterEach(() => { return clearMocks(); });

describe('connectionStatus', () => {
   it('invokes the correct Tauri command', async () => {
      await connectionStatus();

      expect(lastCmd).toBe('plugin:connectivity|connection_status');
   });

   it('returns a wifi connection status', async () => {
      const status = await connectionStatus();

      expect(status.connected).toBe(true);
      expect(status.metered).toBe(false);
      expect(status.constrained).toBe(false);
      expect(status.connectionType).toBe('wifi');
   });

   it('returns a cellular connection status', async () => {
      mockIPC(() => { return CONNECTED_CELLULAR; });

      const status = await connectionStatus();

      expect(status.connected).toBe(true);
      expect(status.metered).toBe(true);
      expect(status.connectionType).toBe('cellular');
   });

   it('returns an ethernet connection status', async () => {
      mockIPC(() => { return CONNECTED_ETHERNET; });

      const status = await connectionStatus();

      expect(status.connected).toBe(true);
      expect(status.connectionType).toBe('ethernet');
   });

   it('returns a disconnected status', async () => {
      mockIPC(() => { return DISCONNECTED; });

      const status = await connectionStatus();

      expect(status.connected).toBe(false);
      expect(status.metered).toBe(false);
      expect(status.constrained).toBe(false);
      expect(status.connectionType).toBe('unknown');
   });

   it('returns constrained status when data is restricted', async () => {
      mockIPC(() => {
         return { ...CONNECTED_CELLULAR, constrained: true };
      });

      const status = await connectionStatus();

      expect(status.connected).toBe(true);
      expect(status.metered).toBe(true);
      expect(status.constrained).toBe(true);
   });

   it('handles errors thrown by the backend', async () => {
      mockIPC(() => { throw new Error('unsupported'); });

      await expect(connectionStatus()).rejects.toThrow('unsupported');
   });
});
