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

  async getVergeConfig(): Promise<Result<IVerge, string>> {
    try {
      return { status: 'ok', data: await TAURI_INVOKE('get_verge_config') };
    } catch (e) {
      if (e instanceof Error) throw e;
      else return { status: 'error', error: e as any };
    }
  },

  async patchVergeConfig(payload: IVerge): Promise<Result<null, string>> {
    try {
      return {
        status: 'ok',
        data: await TAURI_INVOKE('patch_verge_config', { payload }),
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

/**
 * ### `verge.yaml` schema
 */
export type IVerge = {
  /**
   * app listening port for app singleton
   */
  app_singleton_port: number | null;

  language: string | null;
  /**
   * `light` or `dark` or `system`
   */
  theme_mode: string | null;
  /**
   * enable traffic graph default is true
   */
  traffic_graph: boolean | null;
  /**
   * show memory info (only for Clash Meta)
   */
  enable_memory_usage: boolean | null;
  /**
   * global ui framer motion effects
   */
  lighten_animation_effects: boolean | null;
  /**
   * clash tun mode
   */
  enable_tun_mode: boolean | null;
  /**
   * windows service mode
   */
  enable_service_mode?: boolean | null;
  /**
   * can the app auto startup
   */
  enable_auto_launch: boolean | null;
  /**
   * not show the window on launch
   */
  enable_silent_start: boolean | null;
  /**
   * set system proxy
   */
  enable_system_proxy: boolean | null;
  /**
   * enable proxy guard
   */
  enable_proxy_guard: boolean | null;
  /**
   * set system proxy bypass
   */
  system_proxy_bypass: string | null;
  /**
   * proxy guard interval
   */
  proxy_guard_interval: number | null;
  /**
   * theme setting
   */
  theme_color: string | null;
  /**
   * web ui list
   */
  web_ui_list: string[] | null;
  /**
   * hotkey map
   * format: {func},{key}
   */
  hotkeys: string[] | null;
  /**
   * 切换代理时自动关闭连接 (已弃用)
   * @deprecated use `break_when_proxy_change` instead
   */
  auto_close_connection: boolean | null;

  /**
   * 切换配置时中断连接
   * true: 中断所有连接
   * false: 不中断连接
   */
  break_when_profile_change: boolean | null;
  /**
   * 切换模式时中断连接
   * true: 中断所有连接
   * false: 不中断连接
   */
  break_when_mode_change: boolean | null;
  /**
   * 默认的延迟测试连接
   */
  default_latency_test: string | null;
  /**
   * 支持关闭字段过滤，避免meta的新字段都被过滤掉，默认为真
   */
  enable_clash_fields: boolean | null;
  /**
   * 是否使用内部的脚本支持，默认为真
   */
  enable_builtin_enhanced: boolean | null;
  /**
   * proxy 页面布局 列数
   */
  proxy_layout_column: number | null;
  /**
   * 日志清理
   * 分钟数； 0 为不清理
   * @deprecated use `max_log_files` instead
   */
  auto_log_clean: number | null;
  /**
   * 日记轮转时间，单位：天
   */
  max_log_files: number | null;
  /**
   * window size and position
   * @deprecated use `window_size_state` instead
   */
  window_size_position?: number[] | null;

  /**
   * 是否启用随机端口
   */
  enable_random_port: boolean | null;
  /**
   * verge mixed port 用于覆盖 clash 的 mixed port
   */
  verge_mixed_port: number | null;
  /**
   * Check update when app launch
   */
  enable_auto_check_update: boolean | null;
  always_on_top: boolean | null;

  /**
   * PAC URL for automatic proxy configuration
   * This field is used to set PAC proxy without exposing it to the frontend UI
   */
  pac_url?: string | null;
  /**
   * enable tray text display on Linux systems
   * When enabled, shows proxy and TUN mode status as text next to the tray icon
   * When disabled, only shows status via icon changes (prevents text display issues on Wayland)
   */
  enable_tray_text: boolean | null;
};
