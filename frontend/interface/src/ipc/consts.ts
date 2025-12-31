/**
 * Chimera profile query key, used to fetch profiles from query
 */
export const RROFILES_QUERY_KEY = 'profiles';

/**
 * CHIMERA setting query key, used by useSettings hook
 */
export const CHIMERA_SETTING_QUERY_KEY = 'settings';

/**
 * Clash connections query key, used by clash ws provider to mutate connections via clash connections ws api
 */
export const CLASH_CONNECTIONS_QUERY_KEY = 'clash-connections';

/**
 * Clash info query key, used by useClashInfo hook
 */
export const CLASH_INFO_QUERY_KEY = 'clash-info';

/**
 * Clash config query key, used by useClashConfig hook
 */
export const CLASH_CONFIG_QUERY_KEY = 'clash-config';

/**
 * Clash proxies query key, used by useClashProxies hook
 */
export const CLASH_PROXIES_QUERY_KEY = 'clash-proxies';

/**
 * Clash core query key, used by useClashCores hook
 */
export const CLASH_CORE_QUERY_KEY = 'clash-core';

/**
 * Clash version query key, used to fetch clash version from query
 */
export const CLASH_VERSION_QUERY_KEY = 'clash-version';

/**
 * Clash log query key, used by clash ws provider to mutate logs via clash logs ws api
 */
export const CLASH_LOGS_QUERY_KEY = 'clash-logs';

/**
 * Clash traffic query key, used by clash ws provider to mutate memory via clash traffic ws api
 */
export const CLASH_TRAAFFIC_QUERY_KEY = 'clash-traffic';

/**
 * Clash memory query key, used by clash ws provider to mutate memory via clash memory ws api
 */
export const CLASH_MEMORY_QUERY_KEY = 'clash-memory';

/**
 * Maximum connections history length, used by clash ws provider to limit connections history length
 */
export const MAX_CONNECTIONS_HISTORY = 32;

/**
 * Maximum memory history length, used by clash ws provider to limit memory history length
 */
export const MAX_MEMORY_HISTORY = 32;

/**
 * Maximum traffic history length, used by clash ws provider to limit traffic history length
 */
export const MAX_TRAFFIC_HISTORY = 32;

/**
 * Maximum logs history length, used by clash ws provider to limit logs history length
 */
export const MAX_LOGS_HISTORY = 1024;
