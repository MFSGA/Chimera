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
