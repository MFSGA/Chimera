import { invoke } from '@tauri-apps/api/core';
import { InspectUpdater } from './types';

export type SystemServiceStatus = 'running' | 'stopped' | 'not_installed';

export type SystemServiceStatusInfo = {
  name: string;
  version: string;
  status: SystemServiceStatus;
  server: unknown | null;
};

export const isAppImage = async () => {
  return await invoke<boolean>('is_appimage');
};

export const openThat = async (path: string) => {
  return await invoke<void>('open_that', { path });
};

export const cleanupProcesses = async () => {
  return await invoke<void>('cleanup_processes');
};

export const getServerPort = async () => {
  return await invoke<number>('get_server_port');
};

export const inspectUpdater = async (updaterId: number) => {
  return await invoke<InspectUpdater>('inspect_updater', { updaterId });
};

export const getStorageItem = async (key: string) => {
  return await invoke<string | null>('get_storage_item', { key });
};

export const setStorageItem = async (key: string, value: string) => {
  return await invoke<void>('set_storage_item', { key, value });
};

export const removeStorageItem = async (key: string) => {
  return await invoke<void>('remove_storage_item', { key });
};

export const statusService = async (): Promise<SystemServiceStatusInfo> => {
  try {
    return await invoke<SystemServiceStatusInfo>('status_service');
  } catch (e) {
    console.error(e);
    return {
      name: 'nyanpasu-service',
      version: '',
      status: 'not_installed',
      server: null,
    };
  }
};

export const installService = async () => {
  return await invoke<void>('install_service');
};

export const uninstallService = async () => {
  return await invoke<void>('uninstall_service');
};

export const startService = async () => {
  return await invoke<void>('start_service');
};

export const stopService = async () => {
  return await invoke<void>('stop_service');
};

export const restartService = async () => {
  return await invoke<void>('restart_service');
};

export const getServiceInstallPrompt = async () => {
  return await invoke<string>('get_service_install_prompt');
};

export const getCoreDir = async () => {
  return await invoke<string>('get_core_dir');
};

export const restartSidecar = async () => {
  return await invoke<void>('restart_sidecar');
};

export const isPortable = async () => {
  return await invoke<boolean>('is_portable');
};
