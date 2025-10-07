import { archCheck } from './utils/arch-check';
import { SIDECAR_HOST } from './utils/consts';
import { colorize, consola } from './utils/logger';
import { Resolve } from './utils/resolve';

// force download
const FORCE = process.argv.includes('--force');
console.log(FORCE);
console.log(FORCE);

// cross platform build using
const ARCH = process.argv.includes('--arch')
  ? process.argv[process.argv.indexOf('--arch') + 1]
  : undefined;

// cross platform build support
if (!SIDECAR_HOST) {
  consola.fatal(colorize`{red.bold SIDECAR_HOST} not found`);

  process.exit(1);
} else {
  consola.debug(colorize`sidecar-host {yellow ${SIDECAR_HOST}}`);
}

const platform = process.platform;

const arch = (ARCH || process.arch) as NodeJS.Architecture | 'armel';

console.log(platform);

archCheck(platform, arch);

const resolve = new Resolve({
  platform,
  arch,
  sidecarHost: SIDECAR_HOST!,
  // todo: should delete in the future
  force: FORCE,
});
