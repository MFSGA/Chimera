import type { ChainablePromiseElement } from 'webdriverio';
import { markE2eStep } from './step-diagnostics.js';

type ElementTarget = ChainablePromiseElement | WebdriverIO.Element;

export async function displayedElement(selector: string, timeout = 15_000) {
  let current: WebdriverIO.Element | undefined;
  let matched = 0;
  let displayed = 0;

  try {
    await browser.waitUntil(
      async () => {
        const elements = await $$(selector);
        const visible: WebdriverIO.Element[] = [];
        matched = await elements.length;

        for (let index = 0; index < matched; index += 1) {
          const element = await elements[index].getElement();
          if (await element.isDisplayed().catch(() => false)) {
            visible.push(element);
          }
        }

        displayed = visible.length;
        current = visible.length === 1 ? visible[0] : undefined;
        return current !== undefined;
      },
      {
        timeout,
        timeoutMsg: `Expected exactly one displayed element for ${selector}.`,
      },
    );
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(
      `${message} Last match count: ${matched}; displayed count: ${displayed}.`,
    );
  }

  if (!current) {
    throw new Error(`No current displayed element found for ${selector}.`);
  }

  return current;
}

export async function focusElement(element: ElementTarget, timeout = 15_000) {
  const target = await element.getElement();
  await target.waitForDisplayed({ timeout });
  await target.scrollIntoView({ block: 'center', inline: 'center' });

  const focused = await browser.execute((node) => {
    const element = node as HTMLElement;
    element.focus();
    return document.activeElement === element;
  }, target);

  if (!focused) {
    throw new Error('The target element could not receive focus.');
  }
}

export async function clickElement(element: ElementTarget, timeout = 15_000) {
  const target = await element.getElement();
  await target.waitForDisplayed({ timeout });
  markE2eStep('pointer', 'scroll target into view');
  await target.scrollIntoView({ block: 'center', inline: 'center' });

  try {
    // WDIO's element.click() has repeatedly returned only after ~60 seconds
    // on Windows WebView2, even when the visible switch did change. Use the
    // same W3C mouse action sequence already exercised by the language spec:
    // it is a real pointer move, press and release, not a DOM click shortcut.
    markE2eStep('pointer', 'perform WebDriver mouse down and up');
    await browser
      .action('pointer', { parameters: { pointerType: 'mouse' } })
      .move({ origin: target })
      .down()
      .pause(10)
      .up()
      .perform();
  } catch (error) {
    const diagnostics = await browser.execute((node) => {
      const element = node as HTMLElement;
      const rect = element.getBoundingClientRect();
      const centerX = rect.left + rect.width / 2;
      const centerY = rect.top + rect.height / 2;
      const hit = document.elementFromPoint(
        centerX,
        centerY,
      ) as HTMLElement | null;

      const describe = (item: HTMLElement | null) =>
        item
          ? {
              tag: item.tagName.toLowerCase(),
              dataSlot: item.dataset.slot ?? null,
              role: item.getAttribute('role'),
              className: item.className || null,
              disabled:
                item instanceof HTMLButtonElement ||
                item instanceof HTMLInputElement ||
                item instanceof HTMLSelectElement ||
                item instanceof HTMLTextAreaElement
                  ? item.disabled
                  : null,
              ariaDisabled: item.getAttribute('aria-disabled'),
              pointerEvents: getComputedStyle(item).pointerEvents,
            }
          : null;

      return {
        target: describe(element),
        rect: {
          x: rect.x,
          y: rect.y,
          width: rect.width,
          height: rect.height,
        },
        centerHit: describe(hit),
      };
    }, target);

    const message = error instanceof Error ? error.message : String(error);
    throw new Error(
      `WebDriver pointer action failed: ${message}\n${JSON.stringify(diagnostics, null, 2)}`,
    );
  }
}
