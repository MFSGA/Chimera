import type { VergeConfig } from '@chimera/interface';
import { atom } from 'jotai';

export const coreTypeAtom =
  atom<NonNullable<VergeConfig['clash_core']>>('mihomo');
