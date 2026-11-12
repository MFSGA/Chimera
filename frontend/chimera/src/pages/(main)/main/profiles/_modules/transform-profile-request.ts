import {
  ProfileTemplate,
  type NewProfileRequest_Deserialize,
  type ProfileDefinition_Deserialize,
} from '@chimera/interface';
import { ProfileType } from './consts';

export function transformRequest(
  type: ProfileType,
  name: string,
  desc: string | null,
): { request: NewProfileRequest_Deserialize; fileData: string } {
  const source = {
    type: 'local' as const,
    binding: { type: 'managed' as const, file: 'pending.yaml' },
  };

  let definition: ProfileDefinition_Deserialize;
  let fileData: string;

  if (type === ProfileType.Merge) {
    definition = { type: 'transform', transform: { type: 'overlay', source } };
    fileData = ProfileTemplate.merge;
  } else {
    const runtime = type === ProfileType.Lua ? 'lua' : 'javascript';
    definition = {
      type: 'transform',
      transform: { type: 'script', source, runtime },
    };
    fileData =
      runtime === 'lua'
        ? ProfileTemplate.luascript
        : ProfileTemplate.javascript;
  }

  return {
    request: { metadata: { name, desc }, definition },
    fileData,
  };
}
