import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  migrateLegacyColumnSettings,
  migrateLegacyColumnSizing,
  resolveColumnOrder,
} from '../src/pages/(main)/main/connections/_modules/column-settings';
import { searchableText } from '../src/utils/searchable-text';

/**
 * Contract CONN-COLUMN-MIGRATION (unit): a settings record from a prior
 * version can contain removed or duplicate columns. The resolved order must
 * retain known ids once and append new columns without changing their order.
 * Deleting the known/id reconciliation fails these assertions.
 */
describe('reference connection column settings', () => {
  it('keeps known saved ids, removes duplicates, and appends new columns', () => {
    assert.deepEqual(
      resolveColumnOrder(
        ['Chains', 'Removed', 'Host', 'Chains'],
        ['Host', 'Downloaded', 'Chains', 'Rule'],
      ),
      ['Chains', 'Host', 'Downloaded', 'Rule'],
    );
  });

  it('uses the reference default order when no columns were saved', () => {
    assert.deepEqual(resolveColumnOrder([], ['Host', 'Chains']), [
      'Host',
      'Chains',
    ]);
  });

  it('migrates Legacy KV visibility and order without removed columns', () => {
    assert.deepEqual(
      migrateLegacyColumnSettings(
        [
          ['chains', false],
          ['host', true],
          ['destination_asn', false],
          ['chains', true],
        ],
        ['Host', 'Chains', 'Rule'],
      ),
      {
        order: ['Chains', 'Host', 'Rule'],
        visibility: { Chains: false, Host: true },
      },
    );
  });

  it('preserves a visible column when all previously supported columns were hidden', () => {
    assert.deepEqual(
      migrateLegacyColumnSettings(
        [
          ['host', false],
          ['chains', false],
        ],
        ['Host', 'Chains'],
      ),
      {
        order: ['Host', 'Chains'],
        visibility: { Host: true, Chains: false },
      },
    );
  });

  it('ignores malformed and absent Legacy preferences', () => {
    assert.equal(migrateLegacyColumnSettings(null, ['Host']), null);
    assert.equal(
      migrateLegacyColumnSettings([['removed', true]], ['Host']),
      null,
    );
  });

  it('migrates saved widths without overwriting newer Reference widths', () => {
    assert.deepEqual(
      migrateLegacyColumnSizing({ host: 220, chains: 370, Host: 280 }),
      { Host: 280, Chains: 370 },
    );
  });

  it('keeps the existing sizing object when no Legacy keys remain', () => {
    const sizing = { Host: 280, Chains: 370 };
    assert.equal(migrateLegacyColumnSizing(sizing), sizing);
  });
});

/**
 * Contract CONN-SEARCH-TEXT (unit): nested string fields remain searchable,
 * case-insensitively, without matching a term across separate field values.
 */
describe('reference connection search normalization', () => {
  it('collects string values from nested metadata and chains', () => {
    const text = searchableText({
      metadata: { host: 'Example.COM', process: 'ProxyClient' },
      chains: ['DIRECT', 'Fallback'],
    });
    assert.ok(text.includes('example.com'));
    assert.ok(text.includes('proxyclient'));
    assert.ok(text.includes('fallback'));
  });

  it('never matches across field boundaries', () => {
    const text = searchableText(['abc', 'def']);
    assert.equal(text.includes('bcde'), false);
    assert.equal(text.includes('abc'), true);
  });
});
