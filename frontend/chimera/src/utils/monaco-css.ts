/**
 * Monaco CSS editor helpers for Chimera custom CSS.
 * Registers a completion item provider that suggests data-slot values.
 */

import type { Monaco } from '@monaco-editor/react';
import { DATA_SLOTS } from '@/generated/data-slots.gen';

let registered = false;

export function registerCssDataSlotCompletion(monacoInstance: Monaco): void {
  if (registered) return;
  registered = true;

  for (const lang of ['css', 'less']) {
    monacoInstance.languages.registerCompletionItemProvider(lang, {
      triggerCharacters: ['"', '=', '-', '['],
      provideCompletionItems(
        model: Monaco['editor']['ITextModel'],
        position: Monaco['Position'],
      ) {
        if (!isLikelySelectorPosition(model, position)) {
          return { suggestions: [] };
        }

        const textBefore = model
          .getLineContent(position.lineNumber)
          .substring(0, position.column - 1);
        const attrMatch = textBefore.match(/\[data-slot="?([^"\]]*)$/);
        const attrPrefix = attrMatch?.[1];
        const isAttrContext = attrPrefix !== undefined;

        let directPrefix: string | undefined;
        if (!isAttrContext) {
          const directMatch = textBefore.match(/(^|[\s>+~,])([a-zA-Z0-9_-]*)$/);
          directPrefix = directMatch?.[2];
        }

        const prefix = isAttrContext ? attrPrefix : directPrefix;
        if (prefix === undefined) return { suggestions: [] };

        const range = new monacoInstance.Range(
          position.lineNumber,
          position.column - prefix.length,
          position.lineNumber,
          position.column,
        );

        return {
          suggestions: DATA_SLOTS.filter((slot) => slot.startsWith(prefix)).map(
            (slot) => ({
              label: slot,
              kind: monacoInstance.languages.CompletionItemKind.Value,
              insertText: isAttrContext ? `${slot}"]` : slot,
              range,
              detail: 'data-slot',
              documentation: {
                value: `Selects: \`[data-slot="${slot}"]\``,
              },
            }),
          ),
        };
      },
    });
  }
}

function isLikelySelectorPosition(
  model: { getOffsetAt: (position: unknown) => number; getValue: () => string },
  position: unknown,
): boolean {
  const offset = model.getOffsetAt(position);
  const text = model.getValue();

  for (let index = offset - 1; index >= 0; index -= 1) {
    const character = text[index];
    if (character === '}') return true;
    if (character === '{') return false;
  }
  return true;
}
