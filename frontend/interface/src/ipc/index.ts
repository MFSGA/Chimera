import { commands } from './bindings';

export * from './use-profile';
export * from './consts';

export { commands } from './bindings';
export type * from './bindings';
export * from './use-settings';
export * from './use-clash-config';
export * from './use-clash-connections';
export * from './use-clash-rules';
/** 7 */
export * from './use-proxy-mode';
/** 8 */
export * from './use-clash-proxies';
/** 9 */
export * from './use-clash-cores';
/** 10 */
export * from './use-clash-version';
export * from './use-clash-logs';
/** 11 */
export * from './use-profile-content';
/** 12 */
export * from './use-runtime-profile';

export * from './use-system-service';
export * from './use-system-proxy';

// manually added
export const openUWPTool = commands.invokeUwpTool;
