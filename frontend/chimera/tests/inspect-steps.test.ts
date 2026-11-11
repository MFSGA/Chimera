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
  stepStatus,
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
    max_log_level: null,
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
    max_log_level: 'warn',
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
    max_log_level: null,
    changed_fields: [],
  },
];

test('inspection chooses the finalizing step, not the independent last leaf', () => {
  const extra: RuntimeInspectionNode = {
    id: 3,
    tag: { kind: 'bare_root' },
    next: [],
    has_logs: false,
    max_log_level: null,
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
  assert.equal(steps[1].status, 'warn');
  assert.equal(matchField(steps[1], 'dns.nameserver'), true);
  assert.equal(matchField(steps[1], 'mmdb'), true);
  assert.equal(matchField(steps[1], 'geosite'), false);
  assert.equal(resolveStep(steps, 'not-a-step')?.key, 'final');
  assert.deepEqual(
    filterSteps(steps, { onlyChanged: true }).map((item) => item.key),
    steps.map((item) => item.key),
  );
});

test('guard and TUN ownership passes appear as distinct stable Inspect steps', () => {
  const kinds = [
    'keep_unmanaged',
    'managed_settings',
    'tun_keep_unmanaged',
    'tun_managed_settings',
  ] as const;
  const phases: RuntimeInspectionNode[] = kinds.map((step, index) => ({
    id: index + 4,
    tag: { kind: 'builtin_step', data: { selected_profile_id: null, step } },
    next: index === kinds.length - 1 ? [2] : [index + 5],
    has_logs: false,
    max_log_level: null,
    changed_fields: index % 2 === 0 ? [] : ['tun.auto-route'],
  }));
  const steps = buildSteps([nodes[0], nodes[1], ...phases, nodes[2]], 2);
  assert.deepEqual(
    steps.filter((item) => item.node.id >= 4).map((item) => item.key),
    kinds.map((step) => `builtin:${step}`),
  );
  assert.ok(
    steps
      .filter((item) => item.node.id >= 4)
      .every((item) => item.stage === 'chimera'),
  );
  assert.equal(steps.at(-1)?.key, 'final');
});

test('default and core field-filter stages remain inspectable before finalizing', () => {
  const finalFieldFilter: RuntimeInspectionNode = {
    id: 4,
    tag: {
      kind: 'builtin_step',
      data: { selected_profile_id: null, step: 'final_field_filter' },
    },
    next: [5],
    has_logs: false,
    max_log_level: null,
    changed_fields: ['mmdb'],
  };
  const defaults: RuntimeInspectionNode = {
    id: 5,
    tag: {
      kind: 'builtin_step',
      data: { selected_profile_id: null, step: 'defaults' },
    },
    next: [2],
    has_logs: false,
    max_log_level: null,
    changed_fields: ['dns', 'profile'],
  };
  const steps = buildSteps(
    [nodes[0], nodes[1], finalFieldFilter, defaults, nodes[2]],
    2,
  );
  assert.equal(
    steps.find((s) => s.node.id === 4)?.key,
    'builtin:final_field_filter',
  );
  assert.equal(steps.find((s) => s.node.id === 5)?.changedCount, 2);
  assert.equal(steps.find((s) => s.node.id === 5)?.stage, 'chimera');
  assert.equal(steps.at(-1)?.key, 'final');
});

test('core effective step is separate from final and errors outrank changes', () => {
  const effective: RuntimeInspectionNode = {
    id: 3,
    tag: {
      kind: 'builtin_step',
      data: { selected_profile_id: null, step: 'core_controller' },
    },
    next: [],
    has_logs: false,
    max_log_level: null,
    changed_fields: ['mmdb'],
  };
  const steps = buildSteps([...nodes, effective], 2);
  assert.equal(steps.at(-1)?.status, 'core');
  assert.equal(steps.at(-1)?.key, 'core');
  assert.equal(steps.at(-1)?.stage, 'result');
  assert.equal(stepStatus({ ...nodes[1], max_log_level: 'error' }, 2), 'error');
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
