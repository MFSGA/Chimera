/**
 * DOM regression checks adapted from pinned ref's
 * frontend/ui/tests/scroll-area.browser.test.tsx.
 *
 * Happy DOM provides events and nodes, not actual CSS layout or WebDriver.
 * Tests exercise the real ScrollArea hooks and React update lifecycle.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { Window } from 'happy-dom';
import {
  act,
  createElement as h,
  useLayoutEffect,
  type ReactNode,
} from 'react';
import { createRoot } from 'react-dom/client';
import { createServer } from 'vite';

interface Flags {
  isScrolling: boolean;
  isTop: boolean;
  isBottom: boolean;
  isPastHeader: boolean;
  scrollDirection: 'up' | 'down' | 'left' | 'right' | 'none';
}

interface ScrollModule {
  ScrollArea: React.ComponentType<{
    children?: ReactNode;
    scrollbars?: 'vertical' | 'horizontal' | 'both';
  }>;
  AppContentScrollArea: React.ComponentType<{
    children?: ReactNode;
    scrollbars?: 'vertical' | 'horizontal' | 'both';
  }>;
  useScrollArea: () => Flags;
  useScrollAreaViewport: () => {
    viewportRef: { current: HTMLDivElement | null };
  };
  useScrollAreaViewportElement: () => HTMLDivElement | null;
}

async function withScrollFixture(
  run: (ctx: {
    rootElement: HTMLElement;
    module: ScrollModule;
    window: Window;
  }) => Promise<void>,
) {
  const window = new Window({ url: 'http://localhost/' });
  const globals = [
    'window',
    'document',
    'navigator',
    'self',
    'Node',
    'Element',
    'HTMLElement',
    'Event',
    'CustomEvent',
    'MouseEvent',
    'PointerEvent',
    'KeyboardEvent',
    'FocusEvent',
    'MutationObserver',
    'ResizeObserver',
    'IntersectionObserver',
    'getComputedStyle',
    'requestAnimationFrame',
    'cancelAnimationFrame',
    'IS_REACT_ACT_ENVIRONMENT',
  ] as const;
  const previous = new Map(
    globals.map((key) => [
      key,
      Object.getOwnPropertyDescriptor(globalThis, key),
    ]),
  );
  const assign = (key: string, value: unknown) =>
    Object.defineProperty(globalThis, key, {
      configurable: true,
      writable: true,
      value,
    });

  assign('window', window);
  assign('document', window.document);
  assign('navigator', window.navigator);
  assign('self', window);
  assign('Node', window.Node);
  assign('Element', window.Element);
  assign('HTMLElement', window.HTMLElement);
  assign('Event', window.Event);
  assign('CustomEvent', window.CustomEvent);
  assign('MouseEvent', window.MouseEvent);
  assign('PointerEvent', window.PointerEvent);
  assign('KeyboardEvent', window.KeyboardEvent);
  assign('FocusEvent', window.FocusEvent);
  assign('MutationObserver', window.MutationObserver);
  assign('getComputedStyle', window.getComputedStyle.bind(window));
  assign('requestAnimationFrame', window.requestAnimationFrame.bind(window));
  assign('cancelAnimationFrame', window.cancelAnimationFrame.bind(window));
  assign('IS_REACT_ACT_ENVIRONMENT', true);

  class ResizeObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  class IntersectionObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  assign('ResizeObserver', ResizeObserverStub);
  assign('IntersectionObserver', IntersectionObserverStub);

  const rootElement = window.document.createElement('div');
  window.document.body.append(rootElement);

  const vite = await createServer({
    configFile: false,
    root: fileURLToPath(new URL('../', import.meta.url)),
    server: { middlewareMode: true, ws: false },
    esbuild: { jsx: 'automatic' },
    appType: 'custom',
    logLevel: 'error',
  });

  try {
    const module = (await vite.ssrLoadModule(
      '/src/scroll-area.tsx',
    )) as ScrollModule;
    await run({
      module,
      rootElement: rootElement as unknown as HTMLElement,
      window,
    });
  } finally {
    await vite.close();
    rootElement.remove();
    for (const key of globals) {
      const descriptor = previous.get(key);
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
    await window.happyDOM.abort();
  }
}

function emitScroll(viewport: HTMLElement, window: Window, scrollTop: number) {
  viewport.scrollTop = scrollTop;
  viewport.dispatchEvent(
    new window.Event('scroll', { bubbles: true }) as unknown as Event,
  );
}

function provideScrollDimensions(viewport: HTMLElement) {
  Object.defineProperty(viewport, 'scrollHeight', {
    configurable: true,
    value: 2000,
  });
  Object.defineProperty(viewport, 'clientHeight', {
    configurable: true,
    value: 400,
  });
}

for (const name of ['ScrollArea', 'AppContentScrollArea'] as const) {
  test(`${name} tracks header threshold and scroll flags without per-pixel context updates`, async () => {
    await withScrollFixture(async ({ module, rootElement, window }) => {
      const states: Flags[] = [];
      let renders = 0;
      function Consumer() {
        states.push(module.useScrollArea());
        renders += 1;
        return h('div', null, 'content');
      }
      const root = createRoot(rootElement);
      try {
        await act(async () => root.render(h(module[name], null, h(Consumer))));
        const viewport = rootElement.querySelector<HTMLElement>(
          '[data-slot="scroll-area-viewport"]',
        );
        assert.ok(viewport);
        provideScrollDimensions(viewport);
        assert.equal(states.at(-1)?.isPastHeader, false);
        assert.equal(states.at(-1)?.isTop, true);
        assert.equal(states.at(-1)?.scrollDirection, 'none');

        await act(async () => emitScroll(viewport, window, 40));
        assert.equal(
          states.at(-1)?.isPastHeader,
          false,
          'threshold is strictly above 40px',
        );
        assert.equal(states.at(-1)?.isTop, false);
        assert.equal(states.at(-1)?.scrollDirection, 'down');

        await act(async () => emitScroll(viewport, window, 41));
        assert.equal(states.at(-1)?.isPastHeader, true);

        const afterThreshold = renders;
        await act(async () => {
          for (let top = 42; top <= 600; top += 1) {
            emitScroll(viewport, window, top);
          }
        });
        assert.equal(
          renders,
          afterThreshold,
          'changing scrollTop alone should not force consumers to rerender',
        );
        assert.equal(states.at(-1)?.isPastHeader, true);
        assert.equal(states.at(-1)?.isScrolling, true);

        await act(async () => emitScroll(viewport, window, 1600));
        assert.equal(states.at(-1)?.isBottom, true);

        await act(async () => emitScroll(viewport, window, 0));
        assert.equal(states.at(-1)?.isTop, true);
        assert.equal(states.at(-1)?.isPastHeader, false);
        assert.equal(states.at(-1)?.scrollDirection, 'up');

        await act(async () => {
          await new Promise<void>((resolve) => setTimeout(resolve, 80));
        });
        assert.equal(
          states.at(-1)?.isScrolling,
          false,
          'scroll idle timer completes',
        );
      } finally {
        await act(async () => root.unmount());
      }
    });
  });
}

for (const name of ['ScrollArea', 'AppContentScrollArea'] as const) {
  test(`${name} attaches viewport for layout effects without rerendering ref consumers`, async () => {
    await withScrollFixture(async ({ module, rootElement }) => {
      const seen: Array<HTMLDivElement | null> = [];
      const renders = { reference: 0, flags: 0 };

      function ElementConsumer() {
        const element = module.useScrollAreaViewportElement();
        useLayoutEffect(() => {
          seen.push(element);
        }, [element]);
        return null;
      }
      function RefConsumer() {
        const { viewportRef } = module.useScrollAreaViewport();
        assert.ok(viewportRef);
        renders.reference += 1;
        return null;
      }
      function FlagsConsumer() {
        module.useScrollArea();
        renders.flags += 1;
        return null;
      }
      const root = createRoot(rootElement);
      try {
        await act(async () =>
          root.render(
            h(
              module[name],
              null,
              h(ElementConsumer),
              h(RefConsumer),
              h(FlagsConsumer),
            ),
          ),
        );
        const viewport = rootElement.querySelector<HTMLElement>(
          '[data-slot="scroll-area-viewport"]',
        );
        assert.ok(viewport);
        assert.deepEqual(seen, [null, viewport]);
        assert.deepEqual(renders, { reference: 1, flags: 1 });
      } finally {
        await act(async () => root.unmount());
      }
    });
  });
}
