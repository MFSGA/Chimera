import assert from 'node:assert/strict';
import test from 'node:test';
import type {
  ProfileItem_Serialize,
  ProfileLayout,
} from '../src/ipc/bindings.js';
import {
  applyProfileLayout,
  profileLayoutOf,
  profileSections,
} from '../src/ipc/profile-folders.js';

const config = (uid: string): ProfileItem_Serialize => ({
  type: 'config',
  uid,
  name: uid,
  config: {
    type: 'file',
    transforms: [],
    source: {
      type: 'local',
      binding: { type: 'managed', file: `${uid}.yaml` },
    },
  },
});
const transform = (uid: string): ProfileItem_Serialize => ({
  type: 'transform',
  uid,
  name: uid,
  transform: {
    type: 'script',
    runtime: 'javascript',
    source: {
      type: 'local',
      binding: { type: 'managed', file: `${uid}.js` },
    },
  },
});
const uids = (items: { uid: string }[]) => items.map((item) => item.uid);

test('profileSections groups only Configs in member order, with ungrouped in document order', () => {
  const source = {
    items: [config('a'), transform('t'), config('b'), config('c'), config('d')],
    folders: [
      { id: 'f1', name: 'Work', members: ['c', 'a'] },
      { id: 'f2', name: 'Empty', members: [] },
    ],
  };
  const sections = profileSections(source);
  assert.deepEqual(
    sections.folders.map((entry) => entry.folder.id),
    ['f1', 'f2'],
  );
  assert.deepEqual(uids(sections.folders[0].profiles), ['c', 'a']);
  assert.deepEqual(sections.folders[1].profiles, []);
  assert.deepEqual(uids(sections.ungrouped), ['b', 'd']);
});

test('profileSections treats a legacy document without folders as all ungrouped', () => {
  const sections = profileSections({ items: [config('a'), transform('t')] });
  assert.deepEqual(sections.folders, []);
  assert.deepEqual(uids(sections.ungrouped), ['a']);
});

test('profileSections ignores a transform and missing folder member rather than showing them', () => {
  const sections = profileSections({
    items: [config('a'), transform('t')],
    folders: [{ id: 'f1', name: 'Work', members: ['t', 'gone', 'a'] }],
  });
  assert.deepEqual(uids(sections.folders[0].profiles), ['a']);
  assert.deepEqual(sections.ungrouped, []);
});

test('profileLayoutOf calculates ungrouped Configs from committed document', () => {
  const source = {
    items: [config('a'), config('b'), config('c')],
    folders: [{ id: 'f1', name: 'Work', members: ['c'] }],
  };
  assert.deepEqual(profileLayoutOf(source), {
    folders: [{ id: 'f1', name: 'Work', members: ['c'] }],
    ungrouped: ['a', 'b'],
  });
});

test('applyProfileLayout orders folders then ungrouped then transforms without mutating original', () => {
  const source = {
    current: 'a',
    valid: [],
    items: [
      config('a'),
      transform('t1'),
      config('b'),
      config('c'),
      transform('t2'),
    ],
    folders: [],
  };
  const layout: ProfileLayout = {
    folders: [
      { id: 'f1', name: 'Work', members: ['c'] },
      { id: 'f2', name: 'Home', members: ['a'] },
    ],
    ungrouped: ['b'],
  };
  const next = applyProfileLayout(source, layout);
  assert.deepEqual(uids(next.items), ['c', 'a', 'b', 't1', 't2']);
  assert.deepEqual(next.folders, layout.folders);
  assert.equal(next.current, 'a');
  assert.deepEqual(uids(source.items), ['a', 't1', 'b', 'c', 't2']);
  assert.deepEqual(source.folders, []);
});

test('applyProfileLayout and profileLayoutOf agree on a valid layout', () => {
  const layout: ProfileLayout = {
    folders: [{ id: 'f1', name: 'Work', members: ['b', 'a'] }],
    ungrouped: ['c'],
  };
  const next = applyProfileLayout(
    { items: [config('a'), config('b'), config('c')], folders: [] },
    layout,
  );
  assert.deepEqual(profileLayoutOf(next), layout);
});
