type Platform =
  | 'aix'
  | 'android'
  | 'darwin'
  | 'freebsd'
  | 'haiku'
  | 'linux'
  | 'openbsd'
  | 'sunos'
  | 'win32'
  | 'cygwin'
  | 'netbsd';

/**
 * Defines in vite.config.ts.
 */
declare const WIN_PORTABLE: boolean;
declare const OS_PLATFORM: Platform;
declare const IS_NIGHTLY: boolean;
