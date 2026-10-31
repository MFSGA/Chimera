import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import type { ProfileLayout } from '@chimera/interface';
import {
  collapsedSectionId,
  NAV_ALL_ID,
  resolveProfileDrop,
  sectionId,
  sectionItemsOf,
  sidebarFolderId,
  withoutProfile,
} from '../src/pages/(main)/main/profiles/_modules/resolve-profile-drop';

const layout: ProfileLayout = {
  folders: [
    { id: 'f1', name: 'Work', members: ['a', 'b'] },
    { id: 'f2', name: 'Home', members: [] },
  ],
  ungrouped: ['c'],
};

const items = sectionItemsOf(layout);

describe('sectionItemsOf', () => {
  it('keys folders and ungrouped profiles by section ID', () => {
    assert.deepEqual(items, {
      [sectionId('f1')]: ['a', 'b'],
      [sectionId('f2')]: [],
      [sectionId(null)]: ['c'],
    });
  });
});

describe('resolveProfileDrop', () => {
  it('appends to a sidebar folder', () => {
    assert.deepEqual(
      resolveProfileDrop({
        uid: 'c',
        targetId: sidebarFolderId('f1'),
        items,
        layout,
      }),
      { destination: 'f1' },
    );
  });

  it('appends to the ungrouped list from All profiles', () => {
    assert.deepEqual(
      resolveProfileDrop({ uid: 'a', targetId: NAV_ALL_ID, items, layout }),
      { destination: null },
    );
  });

  it('appends to a collapsed folder or ungrouped section', () => {
    assert.deepEqual(
      resolveProfileDrop({
        uid: 'c',
        targetId: collapsedSectionId('f2'),
        items,
        layout,
      }),
      { destination: 'f2' },
    );
    assert.deepEqual(
      resolveProfileDrop({
        uid: 'a',
        targetId: collapsedSectionId(null),
        items,
        layout,
      }),
      { destination: null },
    );
  });

  it('keeps the position of the drag preview for a card or section', () => {
    const preview = {
      [sectionId('f1')]: ['b'],
      [sectionId('f2')]: [],
      [sectionId(null)]: ['c', 'a'],
    };
    assert.deepEqual(
      resolveProfileDrop({ uid: 'a', targetId: 'c', items: preview, layout }),
      { destination: null, index: 1 },
    );
    assert.deepEqual(
      resolveProfileDrop({
        uid: 'a',
        targetId: sectionId(null),
        items: preview,
        layout,
      }),
      { destination: null, index: 1 },
    );
  });

  it('identifies the destination folder from a section', () => {
    assert.deepEqual(
      resolveProfileDrop({
        uid: 'b',
        targetId: 'a',
        items: {
          [sectionId('f1')]: ['b', 'a'],
          [sectionId('f2')]: [],
          [sectionId(null)]: ['c'],
        },
        layout,
      }),
      { destination: 'f1', index: 0 },
    );
  });

  it('returns null for an unknown drop or missing preview', () => {
    assert.equal(
      resolveProfileDrop({ uid: 'a', targetId: undefined, items, layout }),
      null,
    );
    assert.equal(
      resolveProfileDrop({ uid: 'x', targetId: 'a', items, layout }),
      null,
    );
    assert.equal(
      resolveProfileDrop({ uid: 'a', targetId: 'b', items: null, layout }),
      null,
    );
  });
});

describe('hidden active profile', () => {
  it('removes one UID from every section without changing the others', () => {
    assert.deepEqual(withoutProfile(sectionItemsOf(layout), 'a'), {
      [sectionId('f1')]: ['b'],
      [sectionId('f2')]: [],
      [sectionId(null)]: ['c'],
    });
  });

  it('resolves a drop relative to members hidden from the visible preview', () => {
    const shown = withoutProfile(sectionItemsOf(layout), 'a');
    assert.deepEqual(
      resolveProfileDrop({
        uid: 'c',
        targetId: 'b',
        items: {
          ...shown,
          [sectionId('f1')]: ['c', 'b'],
          [sectionId(null)]: [],
        },
        layout,
      }),
      { destination: 'f1', index: 1 },
    );
    assert.deepEqual(
      resolveProfileDrop({
        uid: 'c',
        targetId: 'b',
        items: {
          ...shown,
          [sectionId('f1')]: ['b', 'c'],
          [sectionId(null)]: [],
        },
        layout,
      }),
      { destination: 'f1', index: 2 },
    );
  });
});
