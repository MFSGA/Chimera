import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import type { ProfileLayout } from '@chimera/interface';
import {
  isSameLayout,
  moveProfileInLayout,
  removeFolderFromLayout,
  renameFolderInLayout,
  reorderFoldersInLayout,
} from '../src/pages/(main)/main/profiles/_modules/profile-layout';

const layout: ProfileLayout = {
  folders: [
    { id: 'f1', name: 'Work', members: ['a', 'b'] },
    { id: 'f2', name: 'Home', members: ['c'] },
  ],
  ungrouped: ['d', 'e'],
};

describe('moveProfileInLayout', () => {
  it('moves a profile to the end of another folder', () => {
    assert.deepEqual(moveProfileInLayout(layout, 'a', 'f2'), {
      folders: [
        { id: 'f1', name: 'Work', members: ['b'] },
        { id: 'f2', name: 'Home', members: ['c', 'a'] },
      ],
      ungrouped: ['d', 'e'],
    });
  });

  it('inserts at a given index', () => {
    assert.deepEqual(
      moveProfileInLayout(layout, 'd', 'f1', 1).folders[0].members,
      ['a', 'd', 'b'],
    );
  });

  it('reorders inside the same folder', () => {
    assert.deepEqual(
      moveProfileInLayout(layout, 'a', 'f1', 1).folders[0].members,
      ['b', 'a'],
    );
  });

  it('moves a profile out of its folder', () => {
    const next = moveProfileInLayout(layout, 'b', null);
    assert.deepEqual(next.folders[0].members, ['a']);
    assert.deepEqual(next.ungrouped, ['d', 'e', 'b']);
  });

  it('leaves the layout unchanged for an unknown folder', () => {
    assert.strictEqual(moveProfileInLayout(layout, 'a', 'missing'), layout);
  });

  it('never mutates the original layout', () => {
    const before = structuredClone(layout);
    moveProfileInLayout(layout, 'a', 'f2');
    assert.deepEqual(layout, before);
  });

  it('detects semantic no-op moves', () => {
    assert.equal(
      isSameLayout(layout, moveProfileInLayout(layout, 'b', 'f1')),
      true,
    );
    assert.equal(
      isSameLayout(layout, moveProfileInLayout(layout, 'e', null)),
      true,
    );
  });

  it('does not silently insert an unknown profile into a valid layout', () => {
    const next = moveProfileInLayout(layout, 'not-loaded', 'f2');
    assert.strictEqual(next, layout);
    assert.strictEqual(moveProfileInLayout(layout, 'not-loaded', null), layout);
  });
});

describe('folder edits', () => {
  it('renames a folder', () => {
    assert.equal(
      renameFolderInLayout(layout, 'f2', 'Family').folders[1].name,
      'Family',
    );
  });

  it('removes a folder and appends its members to ungrouped', () => {
    assert.deepEqual(removeFolderFromLayout(layout, 'f1'), {
      folders: [{ id: 'f2', name: 'Home', members: ['c'] }],
      ungrouped: ['d', 'e', 'a', 'b'],
    });
  });

  it('ignores unknown folders', () => {
    assert.strictEqual(removeFolderFromLayout(layout, 'missing'), layout);
  });

  it('keeps unlisted folders after the explicitly ordered folders', () => {
    assert.deepEqual(
      reorderFoldersInLayout(layout, ['f2']).folders.map(({ id }) => id),
      ['f2', 'f1'],
    );
    assert.deepEqual(
      reorderFoldersInLayout(layout, ['f2', 'f1', 'x']).folders.map(
        ({ id }) => id,
      ),
      ['f2', 'f1'],
    );
  });
});
