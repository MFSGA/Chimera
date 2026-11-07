import { useQuery } from '@tanstack/react-query';
import { queries } from './query-bindings.js';
import { unwrapQueryOptions } from './query-options.js';

export const useClashRules = () => {
  const rules = queries.clashApiGetRules();
  return useQuery(unwrapQueryOptions(rules, rules.queryFn!));
};
