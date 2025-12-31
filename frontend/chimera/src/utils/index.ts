/* eslint-disable @typescript-eslint/no-explicit-any */
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
