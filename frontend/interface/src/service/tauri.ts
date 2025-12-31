import { invoke } from '@tauri-apps/api/core';
import { InspectUpdater } from './types';

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
