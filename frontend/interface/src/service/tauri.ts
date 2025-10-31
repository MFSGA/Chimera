import { invoke } from '@tauri-apps/api/core';

export const isAppImage = async () => {
  return await invoke<boolean>('is_appimage');
};
