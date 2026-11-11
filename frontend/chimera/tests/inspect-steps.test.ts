import assert from 'node:assert/strict';
import { test } from 'node:test';
import type {
  RuntimeInspectionNode,
  SnapshotDiffHunk,
} from '@chimera/interface';
import {
  buildSteps,
  filterSteps,
  finalNodeId,
  matchField,
  resolveStep,
} from '../src/pages/(main)/main/profiles/inspect/_modules/inspect-steps';
import {
  cycleIndex,
  diffLines,
  fieldLocator,
  findLines,
} from '../src/pages/(main)/main/profiles/inspect/_modules/text-find';

const nodes: RuntimeInspectionNode[] = [
  {
    id: 0,
    tag: { kind: 'bare_root' },
    next: [1],
    has_logs: false,
    changed_fields: null,
  },
  {
    id: 1,
    tag: {
      kind: 'builtin_step',
      data: { selected_profile_id: null, step: 'whitelist_field_filter' },
    },
    next: [2],
    has_logs: true,
    changed_fields: ['dns', 'mmdb'],
  },
  {
    id: 2,
    tag: {
      kind: 'builtin_step',
      data: { selected_profile_id: null, step: 'finalizing' },
    },
    next: [],
    has_logs: false,
    changed_fields: [],
  },
];

test('inspection chooses the finalizing step, not the independent last leaf', () => {
  const extra: RuntimeInspectionNode = {
    id: 3,
    tag: { kind: 'bare_root' },
    next: [],
    has_logs: false,
    changed_fields: null,
  };
  assert.equal(finalNodeId([...nodes, extra]), 2);
  assert.equal(finalNodeId([nodes[0], extra]), 3);
  assert.equal(finalNodeId([]), undefined);
});

test('ref inspect model preserves source, filter and final steps', () => {
  const steps = buildSteps(nodes, finalNodeId(nodes)!);
  assert.equal(steps.length, 3);
  assert.deepEqual(
    steps.map((item) => item.stage),
    ['source', 'chimera', 'result'],
  );
  assert.equal(steps.at(-1)?.key, 'final');
  assert.equal(steps[1].changedCount, 2);
  assert.equal(matchField(steps[1], 'dns.nameserver'), true);
  assert.equal(matchField(steps[1], 'mmdb'), true);
  assert.equal(matchField(steps[1], 'geosite'), false);
  assert.equal(resolveStep(steps, 'not-a-step')?.key, 'final');
  assert.deepEqual(
    filterSteps(steps, { onlyChanged: true }).map((item) => item.key),
    steps.map((item) => item.key),
  );
});

test('field locator points at the nested YAML mapping and rejects absent fields', () => {
  const lines = [
    'dns:',
    '  enabled: true',
    '  nameserver:',
    '    - 1.1.1.1',
    'mmdb: Country.mmdb',
    'mode: rule',
  ];
  const locate = fieldLocator(lines);
  assert.deepEqual(locate('dns.nameserver'), { start: 2, end: 3 });
  assert.deepEqual(locate('mmdb'), { start: 4, end: 4 });
  assert.equal(locate('dns.fake-field'), null);
  assert.equal(locate('geosite'), null);
  assert.deepEqual(findLines(lines, 'MMDB'), [4]);
  assert.equal(cycleIndex(0, 3, -1), 2);
});

test('diff flattening retains additions and deletions for field search', () => {
  const hunks: SnapshotDiffHunk[] = [
    {
      old_start: 1,
      old_lines: 1,
      new_start: 1,
      new_lines: 1,
      lines: ['-mmdb: Country.mmdb', '+mode: rule'],
    },
  ];
  assert.deepEqual(diffLines(hunks), ['mmdb: Country.mmdb', 'mode: rule']);
});
