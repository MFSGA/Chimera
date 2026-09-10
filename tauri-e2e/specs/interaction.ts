import type { ChainablePromiseElement } from 'webdriverio';

export async function focusElement(
  element: ChainablePromiseElement,
  timeout = 15_000,
) {
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
