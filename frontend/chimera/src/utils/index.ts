/* eslint-disable @typescript-eslint/no-explicit-any */
import { type EnvInfo } from '@chimera/interface';
import { includes, isArray, isObject, isString, some } from 'lodash-es';

export function formatError(err: unknown): string {
  return `Error: ${err instanceof Error ? err.message : String(err)}`;
}

export const containsSearchTerm = (obj: any, term: string): boolean => {
  if (!obj || !term) return false;

  if (isString(obj)) {
    return includes(obj.toLowerCase(), term.toLowerCase());
  }

  if (isObject(obj) || isArray(obj)) {
    return some(obj, (value: any) => containsSearchTerm(value, term));
  }

  return false;
};

export function formatEnvInfos(envs: EnvInfo) {
  let result = '----------- System -----------\n';
  result += `OS: ${envs.os}\n`;
  result += `Arch: ${envs.arch}\n`;
  result += `----------- Device -----------\n`;
  for (const cpu of envs.device.cpu) {
    result += `CPU: ${cpu}\n`;
  }
  result += `Memory: ${envs.device.memory}\n`;
  result += `----------- Core -----------\n`;
  for (const key in envs.core) {
    result += `${key}: \`${envs.core[key]}\`\n`;
  }
  result += `----------- Build Info -----------\n`;
  for (const k of Object.keys(envs.build_info)) {
    const key = k
      .split('_')
      .map((v: string) => v.charAt(0).toUpperCase() + v.slice(1))
      .join(' ');
    result += `${key}: ${envs.build_info[k as keyof EnvInfo['build_info']]}\n`;
  }

  return result;
}
