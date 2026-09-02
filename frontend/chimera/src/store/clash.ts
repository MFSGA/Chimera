import type { IVerge_Serialize } from '@chimera/interface';
import { atom } from 'jotai';

export const coreTypeAtom =
  atom<NonNullable<IVerge_Serialize['clash_core']>>('mihomo');
