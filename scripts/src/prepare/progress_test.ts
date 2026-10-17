import assert from 'node:assert/strict';
import {
  formatProgressSize,
  formatResolveInfo,
  formatSize,
  formatSpeed,
  ProgressRenderer,
} from './progress.ts';

Deno.test('prepare progress formats byte counts and resolved versions', () => {
  assert.equal(formatSize(1024), '1.0 KB');
  assert.equal(formatSpeed(2048), '2.0 KB/s');
  assert.equal(formatProgressSize(1024, 2048), '1.0 KB/2.0 KB');
  assert.deepEqual(
    formatResolveInfo({
      file: 'mihomo-alpha-aarch64-apple-darwin',
      version: 'alpha-7654321',
      size: 1024,
      speed: 2048,
      cached: false,
    }),
    {
      file: 'mihomo-alpha-aarch64-apple-darwin',
      version: 'alpha-7654321',
      size: '1.0 KB',
      speed: '2.0 KB/s',
      cached: false,
    },
  );
});

Deno.test(
  'non-terminal progress reports task transitions without redraw output',
  () => {
    const log: string[] = [];
    const output: string[] = [];
    const progress = new ProgressRenderer(
      ['mihomo-alpha'],
      {
        start: (...values) => log.push(`start:${values.join(' ')}`),
        info: (...values) => log.push(`info:${values.join(' ')}`),
        warn: (...values) => log.push(`warn:${values.join(' ')}`),
        success: (...values) => log.push(`success:${values.join(' ')}`),
      },
      false,
      (text) => output.push(text),
    );

    progress.start();
    progress.update('mihomo-alpha', 'Pulling', { version: 'alpha-1234567' });
    progress.update('mihomo-alpha', 'Pulling', { size: '1.0 MB' });
    progress.update('mihomo-alpha', 'Retrying', { note: '1/5' });
    progress.update('mihomo-alpha', 'Done', { cached: true });
    progress.finish();

    assert.deepEqual(log, [
      'start:start check and download resources...',
      'info:mihomo-alpha Pulling version=alpha-1234567',
      'warn:mihomo-alpha Retrying 1/5',
      'success:mihomo-alpha Done cached',
    ]);
    assert.deepEqual(output, []);
  },
);
