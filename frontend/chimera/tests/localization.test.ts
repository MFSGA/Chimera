import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';
import { formatRelativeTime } from '../src/utils/date';
import { normalizeLanguage, toI18nLanguage } from '../src/utils/language';

type Message =
  | string
  | Array<{
      declarations: string[];
      selectors: string[];
      match: Record<string, string>;
    }>;

const english = JSON.parse(
  readFileSync(new URL('../messages/en.json', import.meta.url), 'utf8'),
) as Record<string, Message>;
const japanese = JSON.parse(
  readFileSync(new URL('../messages/ja.json', import.meta.url), 'utf8'),
) as Record<string, Message>;

function placeholders(message: Message) {
  // Paraglide accepts both simple strings and plural/select message variants.
  // Compare the union of actual interpolation fields across every variant.
  const branches =
    typeof message === 'string'
      ? [message]
      : message.flatMap(({ match }) => Object.values(match));
  return [
    ...new Set(
      branches.flatMap((value) =>
        [...value.matchAll(/\{([\w]+)\}/g)].map((match) => match[1]),
      ),
    ),
  ].sort();
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

  it('supports plural/select translations without losing interpolation fields', () => {
    assert.deepEqual(placeholders(english.profile_folder_delete_description), [
      'count',
    ]);
    assert.deepEqual(placeholders(japanese.profile_folder_delete_description), [
      'count',
    ]);
    assert.deepEqual(
      placeholders([
        {
          declarations: ['input count'],
          selectors: ['count'],
          match: {
            'count=one': '{count} item from {name}',
            'count=other': '{count} items from {name}',
          },
        },
      ]),
      ['count', 'name'],
    );
  });

  it('checks placeholders in the other supported locales, including Russian plurals', () => {
    for (const locale of ['ru', 'zh-cn', 'zh-tw']) {
      const messages = JSON.parse(
        readFileSync(
          new URL(`../messages/${locale}.json`, import.meta.url),
          'utf8',
        ),
      ) as Record<string, Message>;
      assert.deepEqual(
        Object.keys(messages).sort(),
        Object.keys(english).sort(),
      );
      for (const key of Object.keys(english)) {
        if (key === '$schema' || key === 'language') continue;
        assert.deepEqual(
          placeholders(messages[key]),
          placeholders(english[key]),
          `Placeholder mismatch in ${locale}:${key}`,
        );
      }
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
