import type { ClashRule } from '@chimera/interface';
import { atom } from 'jotai';
import type { RefObject } from 'react';

export const atomRulePage = atom<{
  data?: ClashRule[];
  scrollRef?: RefObject<HTMLElement>;
}>();
