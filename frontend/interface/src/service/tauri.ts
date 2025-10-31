import { invoke } from '@tauri-apps/api/core';

export const isAppImage = async () => {
  return await invoke<boolean>('is_appimage');
};

export const openThat = async (path: string) => {
  return await invoke<void>('open_that', { path });
};
