import {
  commands,
  type AppUpdatePhase,
  type EnvInfo,
  type IpsbResponse,
  type UpdaterSummary,
} from '../ipc/bindings';
import { unwrapResult } from '../utils';

export type IPSBResponse = IpsbResponse;

export const isAppImage = async () => {
  return unwrapResult(await commands.isAppimage());
};

export const openThat = async (path: string) => {
  unwrapResult(await commands.openThat(path));
};

export const collectEnvs = async (): Promise<EnvInfo> => {
  return unwrapResult(await commands.collectEnvs());
};

export const recordUpdatePhase = async (
  targetVersion: string,
  phase: AppUpdatePhase,
) => {
  unwrapResult(await commands.recordUpdatePhase(targetVersion, phase));
};

export const cleanupProcesses = async () => {
  unwrapResult(await commands.cleanupProcesses());
};

export const getServerPort = async () => {
  return unwrapResult(await commands.getServerPort());
};

export const inspectUpdater = async (
  updaterId: number,
): Promise<UpdaterSummary> => {
  return unwrapResult(await commands.inspectUpdater(updaterId));
};

export const getStorageItem = async (key: string) => {
  return unwrapResult(await commands.getStorageItem(key));
};

export const setStorageItem = async (key: string, value: string) => {
  unwrapResult(await commands.setStorageItem(key, value));
};

export const removeStorageItem = async (key: string) => {
  unwrapResult(await commands.removeStorageItem(key));
};

export const restartSidecar = async () => {
  unwrapResult(await commands.restartSidecar());
};

export const isPortable = async () => {
  return unwrapResult(await commands.isPortable());
};

export const getCoreStatus = async () => {
  return unwrapResult(await commands.getCoreStatus());
};

export const urlDelayTest = async (url: string, expectedStatus: number) => {
  return unwrapResult(await commands.urlDelayTest(url, expectedStatus));
};

export const getIpsbASN = async (): Promise<IpsbResponse> => {
  return unwrapResult(await commands.getIpsbAsn());
};
