import assert from 'node:assert/strict';
import test from 'node:test';
import {
  resolveCrumbParent,
  type ResolvedCrumb,
} from '../src/components/router/route-crumbs';

const makeCrumb = (
  fullPath: string,
  title: string,
  options: Partial<ResolvedCrumb['crumb']> = {},
  params: Record<string, string> = {},
): ResolvedCrumb => ({
  fullPath,
  params,
  crumb: { title: () => title, ...options },
});

test('uses the closest earlier crumb and keeps a navigation hub visible', () => {
  const crumbs = [
    makeCrumb('/main/settings', 'Settings', { hub: true }),
    makeCrumb('/main/settings/clash/network/', 'Ports & Network'),
  ];
  assert.deepEqual(resolveCrumbParent(crumbs), {
    to: '/main/settings',
    params: {},
    hub: true,
    title: 'Settings',
  });
});

test('legacy composite settings pages return directly to the settings hub', () => {
  const crumbs = [
    makeCrumb('/main/settings', 'Settings', { hub: true }),
    makeCrumb('/main/settings/chimera', 'Chimera'),
  ];
  assert.deepEqual(resolveCrumbParent(crumbs), {
    to: '/main/settings',
    params: {},
    hub: true,
    title: 'Settings',
  });
});

test('direct settings children skip legacy composite parent routes', () => {
  const crumbs = [
    makeCrumb('/main/settings', 'Settings', { hub: true }),
    makeCrumb('/main/settings/chimera', 'Chimera'),
    makeCrumb('/main/settings/chimera/tray', 'Tray', {
      parent: () => ({ to: '/main/settings' }),
    }),
  ];
  assert.deepEqual(resolveCrumbParent(crumbs), {
    to: '/main/settings',
    params: {},
    hub: true,
    title: 'Settings',
  });
});

test('resolves semantic parent and preserves dynamic route parameters', () => {
  const crumbs = [
    makeCrumb(
      '/main/profiles/$type',
      'Profiles',
      { hub: true },
      { type: 'remote' },
    ),
    makeCrumb(
      '/main/profiles/$type/detail/$uid',
      'Detail',
      {
        parent: () => ({ to: '/main/profiles/$type' }),
      },
      { type: 'remote', uid: 'profile-7' },
    ),
  ];
  assert.deepEqual(resolveCrumbParent(crumbs), {
    to: '/main/profiles/$type',
    params: { type: 'remote' },
    hub: true,
    title: 'Profiles',
  });
});

test('returns undefined when no earlier crumb or parent override exists', () => {
  assert.equal(resolveCrumbParent([]), undefined);
  assert.equal(
    resolveCrumbParent([
      makeCrumb('/main/settings', 'Settings', { hub: true }),
    ]),
    undefined,
  );
});
