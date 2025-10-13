/** tauri-specta globals **/

import { invoke as TAURI_INVOKE } from '@tauri-apps/api/core';

export const commands = {
  async importProfile(
    url: string,
    option: RemoteProfileOptionsBuilder | null,
  ): Promise<Result<null, string>> {
    try {
      return {
        status: 'ok',
        data: await TAURI_INVOKE('import_profile', { url, option }),
      };
    } catch (e) {
      if (e instanceof Error) throw e;
      else return { status: 'error', error: e as any };
    }
  },
};

/**
 * Builder for [`RemoteProfileOptions`](struct.RemoteProfileOptions.html).
 *
 */
export type RemoteProfileOptionsBuilder = {
  /**
   * see issue #13
   */
  user_agent: string | null;
  /**
   * for `remote` profile
   * use system proxy
   */
  with_proxy: boolean | null;
  /**
   * use self proxy
   */
  self_proxy: boolean | null;
  /**
   * subscription update interval
   */
  update_interval: number | null;
};

export type Result<T, E> =
  | { status: 'ok'; data: T }
  | { status: 'error'; error: E };
