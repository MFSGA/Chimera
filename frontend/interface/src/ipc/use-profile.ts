import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { unwrapResult } from '../utils/index.js';
import {
  commands,
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
import { RROFILES_QUERY_KEY } from './consts.js';
import { remoteProfileDefinitionOf } from './profile-definition.js';

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
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });

  const query = useQuery({
    queryKey: [RROFILES_QUERY_KEY],
    queryFn: async () => {
      const result = unwrapResult(await commands.getProfiles());
      if (!result) return undefined;
      const items = result.items ?? [];
      if (options?.without_helper_fn) return { ...result, items };
      return {
        ...result,
        items: items.map((item) => ({
          ...item,
          view: async () => unwrapResult(await commands.viewProfile(item.uid)),
          update: (option?: RemoteProfileOptionsPatch_Deserialize | null) =>
            update.mutateAsync({ uid: item.uid, option: option ?? null }),
          drop: () => drop.mutateAsync(item.uid),
        })),
      };
    },
  });

  const create = useMutation({
    mutationFn: async (params: CreateParams) => {
      if (params.type === 'url') {
        if (params.data.mode) {
          return unwrapResult(
            await commands.importProfileWithMode(
              params.data.url,
              params.data.name ?? null,
              params.data.option ?? null,
              params.data.mode,
            ),
          );
        }
        return unwrapResult(
          await commands.importProfile(
            params.data.url,
            params.data.name ?? null,
            params.data.option ?? null,
          ),
        );
      }
      return unwrapResult(
        await commands.createProfile(params.data.request, params.data.fileData),
      );
    },
    onSuccess: invalidate,
  });

  const update = useMutation({
    mutationFn: async ({
      uid,
      option,
    }: {
      uid: ProfileId;
      option?: RemoteProfileOptionsPatch_Deserialize | null;
    }) => unwrapResult(await commands.updateProfile(uid, option ?? null)),
    onSuccess: invalidate,
  });

  const patchMetadata = useMutation({
    mutationFn: async ({
      uid,
      patch,
    }: {
      uid: ProfileId;
      patch: ProfileMetadataPatch_Deserialize;
    }) => unwrapResult(await commands.patchProfileMetadata(uid, patch)),
    onSuccess: invalidate,
  });

  const patchRemoteOptions = useMutation({
    mutationFn: async ({
      uid,
      patch,
    }: {
      uid: ProfileId;
      patch: RemoteProfileOptionsPatch_Deserialize;
    }) => unwrapResult(await commands.patchRemoteProfileOptions(uid, patch)),
    onSuccess: invalidate,
  });

  const replaceDefinition = useMutation({
    mutationFn: async ({
      uid,
      definition,
    }: {
      uid: ProfileId;
      definition: ProfileDefinition_Deserialize;
    }) =>
      unwrapResult(await commands.replaceProfileDefinition(uid, definition)),
    onSuccess: invalidate,
  });

  const activate = useMutation({
    mutationFn: async (uid: ProfileId | null) =>
      unwrapResult(await commands.activateProfile(uid)),
    onSuccess: invalidate,
  });

  const setValidFields = useMutation({
    mutationFn: async (fields: string[]) =>
      unwrapResult(await commands.setProfileValidFields(fields)),
    onSuccess: invalidate,
  });

  const setGlobalTransforms = useMutation({
    mutationFn: async (ids: ProfileId[]) =>
      unwrapResult(await commands.setGlobalTransforms(ids)),
    onSuccess: invalidate,
  });

  const sort = useMutation({
    mutationFn: async (uids: ProfileId[]) =>
      unwrapResult(await commands.reorderProfilesByList(uids)),
    onSuccess: invalidate,
  });

  const drop = useMutation({
    mutationFn: async (uid: ProfileId) =>
      unwrapResult(await commands.deleteProfile(uid)),
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
