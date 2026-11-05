import fs from 'node:fs';
import path from 'node:path';

/**
 * Seed the legacy input before the E2E app starts. The migration creates all
 * typed config files together, so writing application.yaml alone would make
 * startup reject the incomplete migration state.
 *
 * This only touches the isolated E2E runtime directory, never the application
 * default or a user installation. Existing typed state is owned by the caller.
 */
export function seedE2eDisabledAutoUpdate(configDirectory: string): void {
  if (fs.existsSync(path.join(configDirectory, 'application.yaml'))) {
    return;
  }

  const legacyPath = path.join(configDirectory, 'chimera-config.yaml');
  const existing = fs.existsSync(legacyPath)
    ? fs.readFileSync(legacyPath, 'utf8')
    : '';
  const key = /^[ \t]*enable_auto_check_update[ \t]*:[^\r\n]*$/m;
  const disabled = 'enable_auto_check_update: false';
  const next = key.test(existing)
    ? existing.replace(key, disabled)
    : `${existing}${existing.length > 0 && !existing.endsWith('\n') ? '\n' : ''}${disabled}\n`;

  if (next !== existing) {
    fs.writeFileSync(legacyPath, next, 'utf8');
  }
}
