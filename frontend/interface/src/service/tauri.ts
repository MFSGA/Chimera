import { invoke } from '@tauri-apps/api/core';
import { type EnvInfo } from '../ipc/bindings';
import { InspectUpdater } from './types';

export interface IPSBResponse {
  organization: string;
  longitude: number;
  timezone: string;
  isp: string;
  offset: number;
  asn: number;
  asn_organization: string;
  country: string;
  ip: string;
  latitude: number;
  continent_code: string;
  country_code: string;
}

export const isAppImage = async () => {
  return await invoke<boolean>('is_appimage');
};

export const openThat = async (path: string) => {
  return await invoke<void>('open_that', { path });
};

export const collectEnvs = async () => {
  return await invoke<EnvInfo>('collect_envs');
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

export const restartSidecar = async () => {
  return await invoke<void>('restart_sidecar');
};

export const isPortable = async () => {
  return await invoke<boolean>('is_portable');
};

export const getCoreStatus = async () => {
  return await invoke<
    ['Running' | { Stopped: string | null }, number, 'normal' | 'service']
  >('get_core_status');
};

export const urlDelayTest = async (url: string, expectedStatus: number) => {
  return await invoke<number | null>('url_delay_test', {
    url,
    expectedStatus,
  });
};

export const getIpsbASN = async () => {
  return await invoke<IPSBResponse>('get_ipsb_asn');
};
