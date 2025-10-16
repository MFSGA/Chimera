/** tauri-specta globals **/

import { invoke as TAURI_INVOKE } from '@tauri-apps/api/core';

export const commands = {
  async getGreet(greetmsg: string): Promise<Result<string, string>> {
    try {
      return {
        status: 'ok',
        data: await TAURI_INVOKE('greet', { name: greetmsg }),
      };
    } catch (e) {
      if (e instanceof Error) throw e;
      else return { status: 'error', error: e as any };
    }
  },

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

  async getProfiles(): Promise<Result<Profiles, string>> {
    try {
      return { status: 'ok', data: await TAURI_INVOKE('get_profiles') };
    } catch (e) {
      if (e instanceof Error) throw e;
      else return { status: 'error', error: e as any };
    }
  },

  async viewProfile(uid: string): Promise<Result<null, string>> {
    try {
      return {
        status: 'ok',
        data: await TAURI_INVOKE('view_profile', { uid }),
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

// todo: change the def to types folder
export type Profile =
  | ({ type: 'remote' } & RemoteProfile)
  | ({ type: 'local' } & LocalProfile);
// todo | ({ type: 'merge' } & MergeProfile)
// | ({ type: 'script' } & ScriptProfile)

export type RemoteProfile = {
  /**
   * Profile ID
   */
  uid: string;
  /**
   * profile name
   */
  name: string;
  /**
   * profile holds the file
   */
  file: string;
  /**
   * profile description
   */
  desc: string | null;
  /**
   * update time
   */
  updated: number;
} & {
  /**
   * subscription url
   */
  url: string;
  /**
   * subscription user info
   */
  extra?: SubscriptionInfo;
  /**
   * remote profile options
   */
  option?: RemoteProfileOptions;
  /**
   * process chain
   */
  chain?: string[];
};

export type LocalProfile = {
  /**
   * Profile ID
   */
  uid: string;
  /**
   * profile name
   */
  name: string;
  /**
   * profile holds the file
   */
  file: string;
  /**
   * profile description
   */
  desc: string | null;
  /**
   * update time
   */
  updated: number;
} & {
  /**
   * file symlinks
   */
  symlinks?: string | null;
  /**
   * process chain
   */
  chain?: string[];
};

export type SubscriptionInfo = {
  upload: number;
  download: number;
  total: number;
  expire: number;
};

export type RemoteProfileOptions = {
  /**
   * see issue #13
   */
  user_agent?: string | null;
  /**
   * for `remote` profile
   * use system proxy
   */
  with_proxy?: boolean | null;
  /**
   * use self proxy
   */
  self_proxy?: boolean | null;
  /**
   * subscription update interval
   */
  update_interval: number;
};

/**
 * Define the `profiles.yaml` schema
 */
export type Profiles = {
  /**
   * same as PrfConfig.current
   */
  current?: string[];
  /**
   * same as PrfConfig.chain
   */
  chain?: string[];
  /**
   * record valid fields for clash
   */
  valid?: string[];
  /**
   * profile list
   */
  items?: Profile[];
};
