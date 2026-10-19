import type { ClashCore_Serialize } from '@chimera/interface';
import { atom } from 'jotai';

export const coreTypeAtom = atom<ClashCore_Serialize>('mihomo');
