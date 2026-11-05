/**
 * SSR contracts for ref-aligned search highlighting. These verify rendered
 * markup and the visible text, not browser selection or desktop interactions.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { createElement as h } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import HighlightText from '../src/highlight-text';

const render = (searchText: string, children: string, className?: string) =>
  renderToStaticMarkup(h(HighlightText, { searchText, className }, children));

test('HighlightText highlights every case-insensitive occurrence and keeps text order', () => {
  const markup = render('al', 'Alpha ALpha', 'custom-highlight');
  assert.match(markup, /^<span class="custom-highlight">/);
  assert.equal((markup.match(/<mark\b/g) ?? []).length, 2);
  assert.match(markup, /<mark[^>]*>Al<\/mark>/);
  assert.match(markup, /<mark[^>]*>AL<\/mark>/);
  assert.equal(markup.replace(/<[^>]+>/g, ''), 'Alpha ALpha');
});

test('HighlightText leaves content unmarked for whitespace-only and absent terms', () => {
  assert.equal(render('  ', 'Unchanged', 'no-mark'), '<span class="no-mark">Unchanged</span>');
  const markup = render('missing', 'Unchanged');
  assert.doesNotMatch(markup, /<mark\b/);
  assert.match(markup, /Unchanged/);
});

test('HighlightText treats punctuation literally and handles adjacent occurrences', () => {
  const literal = render('.*', 'literal .* and unrelated letters');
  assert.equal((literal.match(/<mark\b/g) ?? []).length, 1);
  assert.ok(literal.includes('>.*</mark>'));

  const adjacent = render('aa', 'aaaa');
  assert.equal((adjacent.match(/<mark\b/g) ?? []).length, 2);
  assert.equal(adjacent.replace(/<[^>]+>/g, ''), 'aaaa');
});
