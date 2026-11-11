import { mutationOptions, queryOptions } from '@tanstack/react-query';
import { commands } from './bindings.js';
import {
  CLASH_CONFIG_QUERY_KEY,
  CLASH_PROXIES_PROVIDER_QUERY_KEY,
  CLASH_PROXIES_QUERY_KEY,
  CLASH_RULES_PROVIDER_QUERY_KEY,
  CLASH_RULES_QUERY_KEY,
  CLASH_VERSION_QUERY_KEY,
  NATIVE_PROXIES_QUERY_KEY,
  PROFILES_QUERY_KEY,
  TRANSPARENT_PROXY_STATUS_QUERY_KEY,
} from './consts.js';

/**
 * Application query and mutation descriptors, parallel to ref's
 * frontend/query/src/query-bindings.ts. Chimera Specta generates commands
 * only, so keep typed query metadata outside generated bindings.
 */
export const queries = {
  getTransparentProxyStatus: () =>
    queryOptions({
      queryKey: [TRANSPARENT_PROXY_STATUS_QUERY_KEY],
      queryFn: () => commands.getTransparentProxyStatus(),
    }),
  getProfiles: (...args: Parameters<typeof commands.getProfiles>) =>
    queryOptions({
      queryKey: [PROFILES_QUERY_KEY, ...args],
      queryFn: () => commands.getProfiles(...args),
    }),
  readProfileFile: (...args: Parameters<typeof commands.readProfileFile>) =>
    queryOptions({
      queryKey: ['readProfileFile', ...args],
      queryFn: () => commands.readProfileFile(...args),
    }),
  inspectRuntime: () =>
    queryOptions({
      queryKey: ['inspectRuntime'],
      queryFn: () => commands.inspectRuntime(),
    }),
  inspectRuntimeNode: (
    ...args: Parameters<typeof commands.inspectRuntimeNode>
  ) =>
    queryOptions({
      queryKey: ['inspectRuntimeNode', ...args],
      queryFn: () => commands.inspectRuntimeNode(...args),
    }),
  clashApiGetConfigs: (
    ...args: Parameters<typeof commands.clashApiGetConfigs>
  ) =>
    queryOptions({
      queryKey: [CLASH_CONFIG_QUERY_KEY, ...args],
      queryFn: () => commands.clashApiGetConfigs(...args),
    }),
  getProxies: (...args: Parameters<typeof commands.getProxies>) =>
    queryOptions({
      queryKey: [CLASH_PROXIES_QUERY_KEY, ...args],
      queryFn: () => commands.getProxies(...args),
    }),
  /** Native nodes/groups graph; keep its cache separate from legacy DTOs. */
  getNativeProxies: (...args: Parameters<typeof commands.getNativeProxies>) =>
    queryOptions({
      queryKey: [NATIVE_PROXIES_QUERY_KEY, ...args],
      queryFn: () => commands.getNativeProxies(...args),
    }),
  clashApiGetProxyDelay: (
    ...args: Parameters<typeof commands.clashApiGetProxyDelay>
  ) =>
    queryOptions({
      queryKey: ['clashApiGetProxyDelay', ...args],
      queryFn: () => commands.clashApiGetProxyDelay(...args),
    }),
  clashApiGetGroupDelay: (
    ...args: Parameters<typeof commands.clashApiGetGroupDelay>
  ) =>
    queryOptions({
      queryKey: ['clashApiGetGroupDelay', ...args],
      queryFn: () => commands.clashApiGetGroupDelay(...args),
    }),
  clashApiGetRules: (...args: Parameters<typeof commands.clashApiGetRules>) =>
    queryOptions({
      queryKey: [CLASH_RULES_QUERY_KEY, ...args],
      queryFn: () => commands.clashApiGetRules(...args),
    }),
  clashApiGetVersion: (
    ...args: Parameters<typeof commands.clashApiGetVersion>
  ) =>
    queryOptions({
      queryKey: [CLASH_VERSION_QUERY_KEY, ...args],
      queryFn: () => commands.clashApiGetVersion(...args),
    }),
  clashApiGetProvidersRules: (
    ...args: Parameters<typeof commands.clashApiGetProvidersRules>
  ) =>
    queryOptions({
      queryKey: [CLASH_RULES_PROVIDER_QUERY_KEY, ...args],
      queryFn: () => commands.clashApiGetProvidersRules(...args),
    }),
  clashApiGetProvidersProxies: (
    ...args: Parameters<typeof commands.clashApiGetProvidersProxies>
  ) =>
    queryOptions({
      queryKey: [CLASH_PROXIES_PROVIDER_QUERY_KEY, ...args],
      queryFn: () => commands.clashApiGetProvidersProxies(...args),
    }),
};

export const mutations = {
  selectProxy: mutationOptions({
    mutationKey: ['selectProxy'],
    mutationFn: (input: Parameters<typeof commands.selectProxy>) =>
      commands.selectProxy(...input),
  }),
  clearProxyFixed: mutationOptions({
    mutationKey: ['clearProxyFixed'],
    mutationFn: (input: Parameters<typeof commands.clearProxyFixed>) =>
      commands.clearProxyFixed(...input),
  }),
  /** One native group operation on one bound core, even for pinned members. */
  testNativeProxyGroupDelay: mutationOptions({
    mutationKey: ['testNativeProxyGroupDelay'],
    mutationFn: (
      input: Parameters<typeof commands.testNativeProxyGroupDelay>,
    ) => commands.testNativeProxyGroupDelay(...input),
  }),
  patchRuntimeOverrides: mutationOptions({
    mutationKey: ['patchRuntimeOverrides'],
    mutationFn: (input: Parameters<typeof commands.patchRuntimeOverrides>) =>
      commands.patchRuntimeOverrides(...input),
  }),
  clashApiUpdateProvidersRules: mutationOptions({
    mutationKey: ['clashApiUpdateProvidersRules'],
    mutationFn: (
      input: Parameters<typeof commands.clashApiUpdateProvidersRules>,
    ) => commands.clashApiUpdateProvidersRules(...input),
  }),
  updateProxyProvider: mutationOptions({
    mutationKey: ['updateProxyProvider'],
    mutationFn: (input: Parameters<typeof commands.updateProxyProvider>) =>
      commands.updateProxyProvider(...input),
  }),
  healthcheckProxyProvider: mutationOptions({
    mutationKey: ['healthcheckProxyProvider'],
    mutationFn: (input: Parameters<typeof commands.healthcheckProxyProvider>) =>
      commands.healthcheckProxyProvider(...input),
  }),
  saveProfileFile: mutationOptions({
    mutationKey: ['saveProfileFile'],
    mutationFn: (input: Parameters<typeof commands.saveProfileFile>) =>
      commands.saveProfileFile(...input),
  }),
  importProfile: mutationOptions({
    mutationKey: ['importProfile'],
    mutationFn: (input: Parameters<typeof commands.importProfile>) =>
      commands.importProfile(...input),
  }),
  importProfileWithMode: mutationOptions({
    mutationKey: ['importProfileWithMode'],
    mutationFn: (input: Parameters<typeof commands.importProfileWithMode>) =>
      commands.importProfileWithMode(...input),
  }),
  createProfile: mutationOptions({
    mutationKey: ['createProfile'],
    mutationFn: (input: Parameters<typeof commands.createProfile>) =>
      commands.createProfile(...input),
  }),
  updateProfile: mutationOptions({
    mutationKey: ['updateProfile'],
    mutationFn: (input: Parameters<typeof commands.updateProfile>) =>
      commands.updateProfile(...input),
  }),
  patchProfileMetadata: mutationOptions({
    mutationKey: ['patchProfileMetadata'],
    mutationFn: (input: Parameters<typeof commands.patchProfileMetadata>) =>
      commands.patchProfileMetadata(...input),
  }),
  patchRemoteProfileOptions: mutationOptions({
    mutationKey: ['patchRemoteProfileOptions'],
    mutationFn: (
      input: Parameters<typeof commands.patchRemoteProfileOptions>,
    ) => commands.patchRemoteProfileOptions(...input),
  }),
  replaceProfileDefinition: mutationOptions({
    mutationKey: ['replaceProfileDefinition'],
    mutationFn: (input: Parameters<typeof commands.replaceProfileDefinition>) =>
      commands.replaceProfileDefinition(...input),
  }),
  viewProfile: mutationOptions({
    mutationKey: ['viewProfile'],
    mutationFn: (input: Parameters<typeof commands.viewProfile>) =>
      commands.viewProfile(...input),
  }),
  activateProfile: mutationOptions({
    mutationKey: ['activateProfile'],
    mutationFn: (input: Parameters<typeof commands.activateProfile>) =>
      commands.activateProfile(...input),
  }),
  setProfileValidFields: mutationOptions({
    mutationKey: ['setProfileValidFields'],
    mutationFn: (input: Parameters<typeof commands.setProfileValidFields>) =>
      commands.setProfileValidFields(...input),
  }),
  setGlobalTransforms: mutationOptions({
    mutationKey: ['setGlobalTransforms'],
    mutationFn: (input: Parameters<typeof commands.setGlobalTransforms>) =>
      commands.setGlobalTransforms(...input),
  }),
  setProfileLayout: mutationOptions({
    mutationKey: ['setProfileLayout'],
    mutationFn: (input: Parameters<typeof commands.setProfileLayout>) =>
      commands.setProfileLayout(...input),
  }),
  createProfileFolder: mutationOptions({
    mutationKey: ['createProfileFolder'],
    mutationFn: (input: Parameters<typeof commands.createProfileFolder>) =>
      commands.createProfileFolder(...input),
  }),
  reorderProfilesByList: mutationOptions({
    mutationKey: ['reorderProfilesByList'],
    mutationFn: (input: Parameters<typeof commands.reorderProfilesByList>) =>
      commands.reorderProfilesByList(...input),
  }),
  deleteProfile: mutationOptions({
    mutationKey: ['deleteProfile'],
    mutationFn: (input: Parameters<typeof commands.deleteProfile>) =>
      commands.deleteProfile(...input),
  }),
};
