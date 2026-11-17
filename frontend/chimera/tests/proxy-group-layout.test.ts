import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { ProxyGroupLayout } from '@chimera/interface';
import {
  locateGroup,
  moveGroupByInLayout,
  moveGroupFolderByInLayout,
  moveGroupInLayout,
  removeGroupFolderFromLayout,
  renameGroupFolderInLayout,
  reorderGroupFoldersInLayout,
} from '../src/pages/(main)/main/proxies/_modules/group-layout';
import {
  folderDragId,
  folderDragIdsOf,
  folderHeadingId,
  folderOfDragId,
  groupDragId,
  groupOfDragId,
  resolveGroupDrop,
  sectionId,
  sectionItemsOf,
} from '../src/pages/(main)/main/proxies/_modules/resolve-group-drop';

const layout: ProxyGroupLayout = {
  ungrouped: ['Proxy', 'Auto'],
  folders: [
    { id: 'f1', name: 'Streaming', members: ['Netflix', 'Disney+'] },
    { id: 'f2', name: 'Regions', members: ['JP'] },
  ],
};

test('group moved into a folder leaves its previous place', () => {
  assert.deepEqual(moveGroupInLayout(layout, 'Proxy', 'f2', 0), {
    ungrouped: ['Auto'],
    folders: [
      layout.folders[0],
      { ...layout.folders[1], members: ['Proxy', 'JP'] },
    ],
  });
  assert.deepEqual(
    moveGroupInLayout(layout, 'Netflix', 'f2').folders[0].members,
    ['Disney+'],
  );
  assert.deepEqual(moveGroupInLayout(layout, 'Netflix', null, 1).ungrouped, [
    'Proxy',
    'Netflix',
    'Auto',
  ]);
});

test('reordering stays in section and unknown targets do nothing', () => {
  assert.equal(moveGroupInLayout(layout, 'Auto', 'missing'), layout);
  assert.deepEqual(moveGroupInLayout(layout, 'Auto', null, 0).ungrouped, [
    'Auto',
    'Proxy',
  ]);
  assert.deepEqual(
    moveGroupByInLayout(layout, 'Netflix', 1).folders[0].members,
    ['Disney+', 'Netflix'],
  );
  assert.deepEqual(moveGroupByInLayout(layout, 'Auto', -1).ungrouped, [
    'Auto',
    'Proxy',
  ]);
  assert.deepEqual(moveGroupByInLayout(layout, 'Proxy', -1), layout);
  assert.deepEqual(moveGroupByInLayout(layout, 'Missing', 1), layout);
});

test('folder rename, removal, and reorder do not mutate prior layout', () => {
  const before = structuredClone(layout);
  assert.equal(
    renameGroupFolderInLayout(layout, 'f2', 'Asia').folders[1].name,
    'Asia',
  );
  assert.deepEqual(removeGroupFolderFromLayout(layout, 'f1').ungrouped, [
    'Proxy',
    'Auto',
    'Netflix',
    'Disney+',
  ]);
  assert.equal(removeGroupFolderFromLayout(layout, 'missing'), layout);
  assert.deepEqual(
    moveGroupFolderByInLayout(layout, 'f1', 1).folders.map((f) => f.id),
    ['f2', 'f1'],
  );
  assert.deepEqual(
    reorderGroupFoldersInLayout(layout, ['f2', 'ghost']).folders.map(
      (f) => f.id,
    ),
    ['f2', 'f1'],
  );
  assert.deepEqual(layout, before);
});

test('group location and drag prefixes distinguish folder names from group names', () => {
  assert.deepEqual(locateGroup(layout, 'Auto'), {
    folder: null,
    index: 1,
    count: 2,
  });
  assert.equal(locateGroup(layout, 'Missing'), undefined);
  assert.notEqual(groupDragId('folder:f1'), folderDragId('f1'));
  assert.equal(groupOfDragId(groupDragId('folder:f1')), 'folder:f1');
  assert.equal(groupOfDragId(folderDragId('f1')), undefined);
  assert.equal(folderOfDragId(folderDragId('f1')), 'f1');
  assert.deepEqual(folderDragIdsOf(layout), [
    folderDragId('f1'),
    folderDragId('f2'),
  ]);
});

test('drop on a collapsed folder heading appends to its group', () => {
  const items = sectionItemsOf(layout);
  assert.deepEqual(
    resolveGroupDrop({ name: 'Auto', targetId: folderHeadingId('f1'), items }),
    { destination: 'f1' },
  );
});

test('drop preview resolves exact insertion index, including ungrouped and empty folders', () => {
  const items = sectionItemsOf(layout);
  const preview = {
    ...items,
    [sectionId('f1')]: [
      groupDragId('Netflix'),
      groupDragId('Auto'),
      groupDragId('Disney+'),
    ],
    [sectionId(null)]: [groupDragId('Proxy')],
  };
  assert.deepEqual(
    resolveGroupDrop({
      name: 'Auto',
      targetId: groupDragId('Disney+'),
      items: preview,
    }),
    { destination: 'f1', index: 1 },
  );
  assert.deepEqual(
    resolveGroupDrop({ name: 'Proxy', targetId: sectionId(null), items }),
    { destination: null, index: 0 },
  );
  assert.deepEqual(
    resolveGroupDrop({
      name: 'Auto',
      targetId: sectionId('f2'),
      items: {
        ...items,
        [sectionId('f2')]: [groupDragId('JP'), groupDragId('Auto')],
        [sectionId(null)]: [groupDragId('Proxy')],
      },
    }),
    { destination: 'f2', index: 1 },
  );
});

test('cancel or absent preview cannot write a drag destination', () => {
  const items = sectionItemsOf(layout);
  assert.equal(
    resolveGroupDrop({ name: 'Auto', targetId: undefined, items }),
    null,
  );
  assert.equal(
    resolveGroupDrop({
      name: 'Auto',
      targetId: groupDragId('Proxy'),
      items: null,
    }),
    null,
  );
  assert.equal(
    resolveGroupDrop({
      name: 'Unknown',
      targetId: groupDragId('Proxy'),
      items,
    }),
    null,
  );
});
