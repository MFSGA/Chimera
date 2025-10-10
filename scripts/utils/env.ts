import path from 'path';

export const cwd = process.cwd();
export const TAURI_APP_DIR = path.join(cwd, 'backend/tauri');
export const TEMP_DIR = path.join(cwd, 'node_modules/.verge');
