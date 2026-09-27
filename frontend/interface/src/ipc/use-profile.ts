import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { unwrapResult } from '../utils/index.js';
import {
  type MutationOutcome,
  type NewProfileRequest_Deserialize,
  type ProfileDefinition_Deserialize,
  type ProfileId,
  type ProfileItem_Serialize,
  type ProfileMetadataPatch_Deserialize,
  type ProfileSource_Serialize,
  type RemoteProfileImportMode,
  type RemoteProfileOptionsPatch_Deserialize,
} from './bindings.js';
import { mutations, queries } from './profile-bindings.js';
import { remoteProfileDefinitionOf } from './profile-definition.js';
import { invokeMutation, invokeQuery } from './query-options.js';

export const isConfigItem = (
  item: ProfileItem_Serialize,
): item is Extract<ProfileItem_Serialize, { type: 'config' }> =>
  item.type === 'config';

export const isTransformItem = (
  item: ProfileItem_Serialize,
): item is Extract<ProfileItem_Serialize, { type: 'transform' }> =>
  item.type === 'transform';

export const isFileConfigItem = (
  item: ProfileItem_Serialize,
): item is Extract<ProfileItem_Serialize, { type: 'config' }> & {
  config: Extract<
    Extract<ProfileItem_Serialize, { type: 'config' }>['config'],
    { type: 'file' }
  >;
} => isConfigItem(item) && item.config.type === 'file';

export const getProfileSource = (
  item: ProfileItem_Serialize,
): ProfileSource_Serialize | undefined => {
  if (isFileConfigItem(item)) return item.config.source;
  if (isTransformItem(item)) return item.transform.source;
  return undefined;
};

export const getRemoteSource = (
  item: ProfileItem_Serialize,
): Extract<ProfileSource_Serialize, { type: 'remote' }> | undefined =>
  isConfigItem(item) &&
  item.config.type === 'file' &&
  item.config.source.type === 'remote'
    ? item.config.source
    : undefined;

export const isRemoteItem = (item: ProfileItem_Serialize): boolean =>
  getRemoteSource(item) !== undefined;

export const scopedTransformsOf = (
  item: ProfileItem_Serialize,
): ProfileId[] => {
  if (!isConfigItem(item)) return [];
  return item.config.transforms ?? [];
};

export { remoteProfileDefinitionOf };

export interface ProfileHelperFn {
  view: () => Promise<unknown>;
  update: (
    option?: RemoteProfileOptionsPatch_Deserialize | null,
  ) => Promise<MutationOutcome<null>>;
  drop: () => Promise<MutationOutcome<null>>;
}

export type ProfileQueryResultItem = ProfileItem_Serialize &
  Partial<ProfileHelperFn>;

export type ProfileQueryResult = NonNullable<
  ReturnType<typeof useProfile>['query']['data']
>;

export type CreateParams =
  | {
      type: 'url';
      data: {
        url: string;
        name?: string | null;
        option?: RemoteProfileOptionsPatch_Deserialize | null;
        mode?: RemoteProfileImportMode;
      };
    }
  | {
      type: 'manual';
      data: {
        request: NewProfileRequest_Deserialize;
        fileData: string | null;
      };
    };

export const useProfile = (options?: { without_helper_fn?: boolean }) => {
  const queryClient = useQueryClient();
  const profilesOptions = queries.getProfiles();
  const importProfile = mutations.importProfile;
  const importProfileWithMode = mutations.importProfileWithMode;
  const createProfile = mutations.createProfile;
  const updateProfile = mutations.updateProfile;
  const patchProfileMetadata = mutations.patchProfileMetadata;
  const patchRemoteProfileOptions = mutations.patchRemoteProfileOptions;
  const replaceProfileDefinition = mutations.replaceProfileDefinition;
  const viewProfile = mutations.viewProfile;
  const activateProfile = mutations.activateProfile;
  const setProfileValidFields = mutations.setProfileValidFields;
  const setGlobalTransformsMutation = mutations.setGlobalTransforms;
  const reorderProfilesByList = mutations.reorderProfilesByList;
  const deleteProfile = mutations.deleteProfile;
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: profilesOptions.queryKey });

  const query = useQuery({
    queryKey: profilesOptions.queryKey,
    queryFn: async () => {
      const result = unwrapResult(await invokeQuery(profilesOptions));
      if (!result) return undefined;
      const items = result.items ?? [];
      if (options?.without_helper_fn) return { ...result, items };
      return {
        ...result,
        items: items.map((item) => ({
          ...item,
          view: async () =>
            unwrapResult(await invokeMutation(viewProfile, [item.uid])),
          update: (option?: RemoteProfileOptionsPatch_Deserialize | null) =>
            update.mutateAsync({ uid: item.uid, option: option ?? null }),
          drop: () => drop.mutateAsync(item.uid),
        })),
      };
    },
  });

  const create = useMutation({
    mutationKey: createProfile.mutationKey,
    mutationFn: async (params: CreateParams) => {
      if (params.type === 'url') {
        if (params.data.mode) {
          return unwrapResult(
            await invokeMutation(importProfileWithMode, [
              params.data.url,
              params.data.name ?? null,
              params.data.option ?? null,
              params.data.mode,
            ]),
          );
        }
        return unwrapResult(
          await invokeMutation(importProfile, [
            params.data.url,
            params.data.name ?? null,
            params.data.option ?? null,
          ]),
        );
      }
      return unwrapResult(
        await invokeMutation(createProfile, [
          params.data.request,
          params.data.fileData,
        ]),
      );
    },
    onSuccess: invalidate,
  });

  const update = useMutation({
    mutationKey: updateProfile.mutationKey,
    mutationFn: async ({
      uid,
      option,
    }: {
      uid: ProfileId;
      option?: RemoteProfileOptionsPatch_Deserialize | null;
    }) =>
      unwrapResult(await invokeMutation(updateProfile, [uid, option ?? null])),
    onSuccess: invalidate,
  });

  const patchMetadata = useMutation({
    mutationKey: patchProfileMetadata.mutationKey,
    mutationFn: async ({
      uid,
      patch,
    }: {
      uid: ProfileId;
      patch: ProfileMetadataPatch_Deserialize;
    }) =>
      unwrapResult(await invokeMutation(patchProfileMetadata, [uid, patch])),
    onSuccess: invalidate,
  });

  const patchRemoteOptions = useMutation({
    mutationKey: patchRemoteProfileOptions.mutationKey,
    mutationFn: async ({
      uid,
      patch,
    }: {
      uid: ProfileId;
      patch: RemoteProfileOptionsPatch_Deserialize;
    }) =>
      unwrapResult(
        await invokeMutation(patchRemoteProfileOptions, [uid, patch]),
      ),
    onSuccess: invalidate,
  });

  const replaceDefinition = useMutation({
    mutationKey: replaceProfileDefinition.mutationKey,
    mutationFn: async ({
      uid,
      definition,
    }: {
      uid: ProfileId;
      definition: ProfileDefinition_Deserialize;
    }) =>
      unwrapResult(
        await invokeMutation(replaceProfileDefinition, [uid, definition]),
      ),
    onSuccess: invalidate,
  });

  const activate = useMutation({
    mutationKey: activateProfile.mutationKey,
    mutationFn: async (uid: ProfileId | null) =>
      unwrapResult(await invokeMutation(activateProfile, [uid])),
    onSuccess: invalidate,
  });

  const setValidFields = useMutation({
    mutationKey: setProfileValidFields.mutationKey,
    mutationFn: async (fields: string[]) =>
      unwrapResult(await invokeMutation(setProfileValidFields, [fields])),
    onSuccess: invalidate,
  });

  const setGlobalTransforms = useMutation({
    mutationKey: setGlobalTransformsMutation.mutationKey,
    mutationFn: async (ids: ProfileId[]) =>
      unwrapResult(await invokeMutation(setGlobalTransformsMutation, [ids])),
    onSuccess: invalidate,
  });

  const sort = useMutation({
    mutationKey: reorderProfilesByList.mutationKey,
    mutationFn: async (uids: ProfileId[]) =>
      unwrapResult(await invokeMutation(reorderProfilesByList, [uids])),
    onSuccess: invalidate,
  });

  const drop = useMutation({
    mutationKey: deleteProfile.mutationKey,
    mutationFn: async (uid: ProfileId) =>
      unwrapResult(await invokeMutation(deleteProfile, [uid])),
    onSuccess: invalidate,
  });

  return {
    query,
    create,
    update,
    patchMetadata,
    patchRemoteOptions,
    replaceDefinition,
    activate,
    setValidFields,
    setGlobalTransforms,
    sort,
    drop,
  };
};
