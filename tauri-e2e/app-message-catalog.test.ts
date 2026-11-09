import assert from 'node:assert/strict';
import test from 'node:test';
import { xpathLiteral } from './specs/app-message-catalog.ts';

test('XPath literal handles localized labels without apostrophes', () => {
  assert.equal(xpathLiteral('移动到文件夹'), "'移动到文件夹'");
  assert.equal(xpathLiteral('Move to folder'), "'Move to folder'");
});

test('XPath literal escapes apostrophes without changing the label', () => {
  assert.equal(
    xpathLiteral("A user's profile"),
    `concat('A user', "'", 's profile')`,
  );
});
