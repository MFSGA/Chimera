import path from 'path';

export const cwd = process.cwd();
export const TAURI_APP_DIR = path.join(cwd, 'backend/tauri');
export const TEMP_DIR = path.join(cwd, 'node_modules/.verge');

// used for git-info
export const TAURI_APP_TEMP_DIR = path.join(TAURI_APP_DIR, 'tmp');
export const GIT_SUMMARY_INFO_PATH = path.join(
  TAURI_APP_TEMP_DIR,
  'git-info.json',
);
