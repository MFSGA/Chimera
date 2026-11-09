import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const messagesDirectory = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../frontend/chimera/messages',
);

export async function readAppMessageCatalog(): Promise<
  Record<string, unknown>
> {
  const locale = await browser.execute(() =>
    document.documentElement.lang.toLowerCase(),
  );
  const locales = [...new Set([locale, locale.split('-')[0]])];

  for (const candidate of locales) {
    const messagesPath = path.join(messagesDirectory, `${candidate}.json`);
    if (fs.existsSync(messagesPath)) {
      return JSON.parse(fs.readFileSync(messagesPath, 'utf8')) as Record<
        string,
        unknown
      >;
    }
  }

  throw new Error(`No message catalog was found for locale ${locale}.`);
}

export function xpathLiteral(value: string): string {
  if (!value.includes("'")) return `'${value}'`;
  return `concat(${value
    .split("'")
    .map((part) => `'${part}'`)
    .join(`, "'", `)})`;
}
