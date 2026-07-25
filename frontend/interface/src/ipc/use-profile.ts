import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import {
  commands,
  type ProfileBuilderRequest_Deserialize,
  type ProfileDefinition_Deserialize,
  type ProfileMetadataPatch_Deserialize,
  type ProfileResponse,
  type RemoteProfileOptionsBuilder,
  type RemoteProfileOptionsPatch_Deserialize,
} from './bindings';
import { RROFILES_QUERY_KEY } from './consts';

export type NormalizedProfile = ProfileResponse;

export type NormalizedProfileBuilder = ProfileBuilderRequest_Deserialize;

export type URLImportParams = Parameters<typeof commands.importProfile>;

export type ManualImportParams = Parameters<typeof commands.createProfile>;

export type CreateParams =
  | {
      type: 'url';
      data: {
        url: URLImportParams[0];
        option: URLImportParams[1];
      };
    }
  | {
      type: 'manual';
      data: {
        item: NormalizedProfileBuilder;
        fileData: string | null;
      };
    };

type ProfileHelperFn = {
  view: () => Promise<null | undefined>;
  update: (option: RemoteProfileOptionsBuilder) => Promise<null | undefined>;
  drop: () => Promise<null | undefined>;
};

export type ProfileQueryResult = NonNullable<
  ReturnType<typeof useProfile>['query']['data']
>;

export type ProfileQueryResultItem = NormalizedProfile &
  Partial<ProfileHelperFn>;

export const remoteProfileDefinitionOf = (
  profile: ProfileQueryResultItem,
): ProfileDefinition_Deserialize | null => {
  if (profile.type !== 'remote') return null;

  return {
    type: 'config',
    config: {
      type: 'file',
      transforms: [],
      source: {
        type: 'remote',
        file: profile.file,
        updated_at: profile.updated || null,
        url: profile.url,
        option: {
          user_agent: profile.option.user_agent ?? null,
          with_proxy: profile.option.with_proxy,
          self_proxy: profile.option.self_proxy,
          update_interval_minutes: profile.option.update_interval_minutes,
        },
        subscription: profile.extra,
      },
    },
  };
};

export const useProfile = (options?: { without_helper_fn?: boolean }) => {
  const queryClient = useQueryClient();

  function addHelperFn(
    item: ProfileResponse,
  ): NormalizedProfile & ProfileHelperFn {
    const uid = item.uid;
    return {
      ...item,
      view: async () => unwrapResult(await commands.viewProfile(uid)),
      update: async (option: RemoteProfileOptionsBuilder) =>
        await update.mutateAsync({ uid, option }),
      drop: async () => await drop.mutateAsync(uid),
    };
  }

  const query = useQuery({
    queryKey: [RROFILES_QUERY_KEY],
    queryFn: async () => {
      const result = unwrapResult(await commands.getProfiles());

      if (!result) {
        return undefined;
      }

      if (options?.without_helper_fn) {
        return result;
      }

      return {
        ...result,
        items: result.items.map((item) => addHelperFn(item)),
      };
    },
  });

  const create = useMutation({
    mutationFn: async ({ type, data }: CreateParams) => {
      if (type === 'url') {
        const { url, option } = data;
        return unwrapResult(await commands.importProfile(url, option));
      } else {
        const { item, fileData } = data;
        return unwrapResult(await commands.createProfile(item, fileData));
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });
    },
  });

  const update = useMutation({
    mutationFn: async ({
      uid,
      option,
    }: {
      uid: string;
      option: RemoteProfileOptionsBuilder | null;
    }) => {
      return unwrapResult(await commands.updateProfile(uid, option));
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });
    },
  });

  const patch = useMutation({
    mutationFn: async ({
      uid,
      profile,
    }: {
      uid: string;
      profile: NormalizedProfileBuilder;
    }) => {
      return unwrapResult(await commands.patchProfile(uid, profile));
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });
    },
  });

  const setValidFields = useMutation({
    mutationFn: async (fields: string[]) =>
      unwrapResult(await commands.setProfileValidFields(fields)),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });
    },
  });

  const activate = useMutation({
    mutationFn: async (uid: string | null) =>
      unwrapResult(await commands.activateProfile(uid)),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });
    },
  });

  const patchMetadata = useMutation({
    mutationFn: async ({
      uid,
      patch,
    }: {
      uid: string;
      patch: ProfileMetadataPatch_Deserialize;
    }) => unwrapResult(await commands.patchProfileMetadata(uid, patch)),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });
    },
  });

  const patchRemoteOptions = useMutation({
    mutationFn: async ({
      uid,
      patch,
    }: {
      uid: string;
      patch: RemoteProfileOptionsPatch_Deserialize;
    }) => unwrapResult(await commands.patchRemoteProfileOptions(uid, patch)),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });
    },
  });

  const replaceDefinition = useMutation({
    mutationFn: async ({
      uid,
      definition,
    }: {
      uid: string;
      definition: ProfileDefinition_Deserialize;
    }) =>
      unwrapResult(await commands.replaceProfileDefinition(uid, definition)),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });
    },
  });

  const drop = useMutation({
    mutationFn: async (uid: string) => {
      return unwrapResult(await commands.deleteProfile(uid));
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });
    },
  });

  const sort = useMutation({
    mutationFn: async (uids: string[]) =>
      unwrapResult(await commands.reorderProfilesByList(uids)),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: [RROFILES_QUERY_KEY] });
    },
  });

  return {
    query,
    create,
    update,
    patch,
    setValidFields,
    activate,
    patchMetadata,
    patchRemoteOptions,
    replaceDefinition,
    drop,
    sort,
  };
};
