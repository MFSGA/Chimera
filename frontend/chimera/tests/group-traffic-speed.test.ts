import assert from 'node:assert/strict';
import test from 'node:test';
import { latestGroupTrafficSpeed } from '../src/pages/(main)/main/proxies/_modules/group-traffic-speed';

const snapshot = (
  connections: Array<{
    chains: string[];
    downloadSpeed: number;
    uploadSpeed: number;
  }>,
) => ({ connections });

test('uses only the latest connection sample for a group', () => {
  const history = [
    snapshot([{ chains: ['group'], downloadSpeed: 999, uploadSpeed: 999 }]),
    snapshot([{ chains: ['group'], downloadSpeed: 10, uploadSpeed: 20 }]),
  ];
  assert.deepEqual(latestGroupTrafficSpeed(history, 'group'), {
    download: 10,
    upload: 20,
  });
  assert.deepEqual(latestGroupTrafficSpeed(history, 'other'), {
    download: 0,
    upload: 0,
  });
});

test('sums all connections containing a group, including nested chains', () => {
  const samples = [
    snapshot([
      { chains: ['group', 'nested'], downloadSpeed: 3, uploadSpeed: 4 },
      { chains: ['group'], downloadSpeed: 5, uploadSpeed: 6 },
      { chains: ['other'], downloadSpeed: 100, uploadSpeed: 200 },
      { chains: ['group', 'group'], downloadSpeed: 1, uploadSpeed: 2 },
    ]),
  ];
  assert.deepEqual(latestGroupTrafficSpeed(samples, 'group'), {
    download: 9,
    upload: 12,
  });
  assert.deepEqual(latestGroupTrafficSpeed(samples, 'nested'), {
    download: 3,
    upload: 4,
  });
});

test('returns zero with no samples or no active connections', () => {
  assert.deepEqual(latestGroupTrafficSpeed([], 'group'), {
    download: 0,
    upload: 0,
  });
  assert.deepEqual(latestGroupTrafficSpeed([snapshot([])], 'group'), {
    download: 0,
    upload: 0,
  });
});
