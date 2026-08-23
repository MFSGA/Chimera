import type {
  ProfileDefinition_Deserialize,
  ProfileResponse,
} from './bindings.js';

export function remoteProfileDefinitionOf(
  profile: ProfileResponse,
): ProfileDefinition_Deserialize | null {
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
}
