import { SIDECAR_HOST } from './utils/consts';

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
  // todo
  // consola.fatal(colorize`{red.bold SIDECAR_HOST} not found`)
  console.log('de1');

  process.exit(1);
} else {
  console.log('de2');
  // consola.debug(colorize`sidecar-host {yellow ${SIDECAR_HOST}}`)
}
