import { mutationOptions, queryOptions } from '@tanstack/react-query';
import { commands } from './bindings.js';
import { PROFILES_QUERY_KEY } from './consts.js';

/**
 * Profile query and mutation descriptors corresponding to the generated
 * descriptors in ref. Chimera's Specta output currently generates commands
 * only, so keep this adapter outside the generated bindings file.
 */
export const queries = {
  getProfiles: (...args: Parameters<typeof commands.getProfiles>) =>
    queryOptions({
      queryKey: [PROFILES_QUERY_KEY, ...args],
      queryFn: () => commands.getProfiles(...args),
    }),
};

export const mutations = {
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
