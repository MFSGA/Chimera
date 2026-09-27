import type {
  ProfileDefinition_Deserialize,
  ProfileItem_Serialize,
} from './bindings.js';

export const remoteProfileDefinitionOf = (
  item: ProfileItem_Serialize,
): ProfileDefinition_Deserialize | null => {
  if (
    item.type !== 'config' ||
    item.config.type !== 'file' ||
    item.config.source.type !== 'remote'
  ) {
    return null;
  }
  return {
    type: 'config',
    config: {
      type: 'file',
      source: item.config.source,
      transforms: item.config.transforms ?? [],
    },
  };
};
