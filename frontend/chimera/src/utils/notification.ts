import {
  MessageDialogOptions,
  message as tauriMessage,
} from '@tauri-apps/plugin-dialog';

export const message = async (
  value: string,
  options?: string | MessageDialogOptions | undefined,
) => {
  if (typeof options === 'object') {
    await tauriMessage(value, {
      ...options,
      title: options.title
        ? `Clash Chimera - ${options.title}`
        : 'Clash Chimera',
    });
  } else {
    await tauriMessage(value, options);
  }
};
