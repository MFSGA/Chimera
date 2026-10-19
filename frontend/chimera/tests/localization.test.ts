import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import { formatRelativeTime } from '../src/utils/date';
import { normalizeLanguage, toI18nLanguage } from '../src/utils/language';

const english = JSON.parse(
  readFileSync(new URL('../messages/en.json', import.meta.url), 'utf8'),
) as Record<string, string>;
const japanese = JSON.parse(
  readFileSync(new URL('../messages/ja.json', import.meta.url), 'utf8'),
) as Record<string, string>;

function placeholders(message: string) {
  return [...message.matchAll(/\{([\w]+)\}/g)].map((match) => match[1]).sort();
}

describe('Japanese locale support', () => {
  it('provides every English message with the same interpolation fields', () => {
    assert.deepEqual(Object.keys(japanese).sort(), Object.keys(english).sort());
    assert.equal(japanese.language, '日本語');

    for (const key of Object.keys(english)) {
      if (key === '$schema' || key === 'language') continue;
      assert.deepEqual(
        placeholders(japanese[key]),
        placeholders(english[key]),
        `Placeholder mismatch in ${key}`,
      );
    }
  });

  it('normalizes the legacy Japanese locale alias', () => {
    assert.equal(normalizeLanguage('ja'), 'ja');
    assert.equal(normalizeLanguage('ja-JP'), 'ja');
    assert.equal(normalizeLanguage('JA-JP'), 'ja');
    assert.equal(normalizeLanguage('en-US'), 'en');
    assert.equal(normalizeLanguage('fr-FR'), null);
  });

  it('maps UI locales to canonical backend language values', () => {
    const mappings = [
      ['en', 'en'],
      ['ja', 'ja'],
      ['ru', 'ru'],
      ['zh-cn', 'zh-cn'],
      ['zh-tw', 'zh-tw'],
    ] as const;

    for (const [locale, language] of mappings) {
      assert.equal(toI18nLanguage(locale), language);
    }
  });

  it('formats relative time with the Japanese Day.js locale', () => {
    const now = new Date('2026-01-01T12:03:00.000Z');
    const threeMinutesAgo = new Date(now.getTime() - 3 * 60_000);

    assert.match(formatRelativeTime(threeMinutesAgo, now, 'ja'), /3分前/);
  });
});
