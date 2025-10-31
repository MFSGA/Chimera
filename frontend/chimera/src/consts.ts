/* eslint-disable */
// @ts-nocheck

import { getSystem } from '@chimera/ui';

export const OS = getSystem();

export const IS_NIGHTLY = window.__IS_NIGHTLY__ === true;
