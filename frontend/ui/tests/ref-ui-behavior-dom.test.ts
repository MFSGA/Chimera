/**
 * DOM contract tests for the ref UI migration. Happy DOM supplies a controlled
 * DOM and mocked browser observers/animations; these tests are NOT WebDriver
 * or visual E2E proof of browser layout, pointer input or keyboard focus.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { Window } from 'happy-dom';
import { act, createElement as h } from 'react';
import { createServer } from 'vite';

type AnimationRecord = {
  frames: Keyframe[];
  options: KeyframeAnimationOptions;
  playState: 'running' | 'paused' | 'idle';
  play: () => void;
  pause: () => void;
  cancel: () => void;
};

async function withFixture(
  run: (ctx: {
    window: Window;
    root: HTMLElement;
    animations: AnimationRecord[];
    visibility: {
      observed: Element[];
      emit: (isIntersecting: boolean) => void;
      disconnected: boolean;
    }[];
    getMeasurements: () => number;
    setContainerWidth: (width: number) => void;
    resize: { emit: () => void; disconnected: boolean }[];
    load: (path: string) => Promise<Record<string, unknown>>;
  }) => Promise<void>,
) {
  const window = new Window({ url: 'http://localhost/' });
  const document = window.document;
  const keys = [
    'window',
    'document',
    'navigator',
    'self',
    'HTMLElement',
    'Element',
    'Node',
    'Event',
    'CustomEvent',
    'MouseEvent',
    'PointerEvent',
    'KeyboardEvent',
    'FocusEvent',
    'MutationObserver',
    'getComputedStyle',
    'ResizeObserver',
    'IntersectionObserver',
    'requestAnimationFrame',
    'cancelAnimationFrame',
    'IS_REACT_ACT_ENVIRONMENT',
  ] as const;
  const before = new Map(
    keys.map((key) => [key, Object.getOwnPropertyDescriptor(globalThis, key)]),
  );

  const assign = (key: string, value: unknown) =>
    Object.defineProperty(globalThis, key, {
      configurable: true,
      writable: true,
      value,
    });

  assign('window', window);
  assign('document', document);
  assign('self', window);
  assign('navigator', window.navigator);
  assign('HTMLElement', window.HTMLElement);
  assign('Element', window.Element);
  assign('Node', window.Node);
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

  const animations: AnimationRecord[] = [];
  const visibility: {
    observed: Element[];
    emit: (isIntersecting: boolean) => void;
    disconnected: boolean;
  }[] = [];
  const resize: { emit: () => void; disconnected: boolean }[] = [];
  class ResizeObserverStub {
    readonly state: (typeof resize)[number];
    constructor(private readonly notify: ResizeObserverCallback) {
      this.state = {
        disconnected: false,
        emit: () => this.notify([], this as unknown as ResizeObserver),
      };
      resize.push(this.state);
    }
    observe() {}
    unobserve() {}
    disconnect() {
      this.state.disconnected = true;
    }
  }
  class IntersectionObserverStub {
    readonly state: (typeof visibility)[number];
    constructor(private readonly callback: IntersectionObserverCallback) {
      this.state = {
        observed: [],
        disconnected: false,
        emit: (isIntersecting) => {
          this.callback(
            [{ isIntersecting } as IntersectionObserverEntry],
            this as unknown as IntersectionObserver,
          );
        },
      };
      visibility.push(this.state);
    }
    observe(target: Element) {
      this.state.observed.push(target);
    }
    unobserve(target: Element) {
      this.state.observed = this.state.observed.filter(
        (item) => item !== target,
      );
    }
    disconnect() {
      this.state.disconnected = true;
    }
  }
  assign('ResizeObserver', ResizeObserverStub);
  assign('IntersectionObserver', IntersectionObserverStub);
  Object.defineProperty(window, 'ResizeObserver', {
    configurable: true,
    value: ResizeObserverStub,
  });
  Object.defineProperty(window, 'IntersectionObserver', {
    configurable: true,
    value: IntersectionObserverStub,
  });

  let measurementReads = 0;
  let containerWidth = 100;
  Object.defineProperty(window.HTMLElement.prototype, 'offsetWidth', {
    configurable: true,
    get() {
      return this.getAttribute('data-slot') === 'text-marquee'
        ? containerWidth
        : 0;
    },
  });
  Object.defineProperty(window.HTMLElement.prototype, 'scrollWidth', {
    configurable: true,
    get() {
      measurementReads++;
      return (this.textContent?.length ?? 0) * 8;
    },
  });
  Object.defineProperty(window.Element.prototype, 'animate', {
    configurable: true,
    value(frames: Keyframe[], options: KeyframeAnimationOptions) {
      const record: AnimationRecord = {
        frames,
        options,
        playState: 'running',
        play() {
          this.playState = 'running';
        },
        pause() {
          this.playState = 'paused';
        },
        cancel() {
          this.playState = 'idle';
        },
      };
      animations.push(record);
      return record;
    },
  });

  const container = document.createElement('div');
  document.body.append(container);
  const vite = await createServer({
    configFile: false,
    root: fileURLToPath(new URL('../', import.meta.url)),
    plugins: [
      {
        name: 'ref-ui-test-icons',
        enforce: 'pre',
        resolveId(id) {
          if (id.startsWith('~icons/')) return '\0ref-ui-test-icon:' + id;
        },
        load(id) {
          if (id.startsWith('\0ref-ui-test-icon:')) {
            return 'import { createElement } from "react"; export default function Icon() { return createElement("svg", { "aria-hidden": true }); }';
          }
        },
      },
    ],
    oxc: { jsx: { runtime: 'automatic' } },
    server: { middlewareMode: true, ws: false },
    appType: 'custom',
    logLevel: 'error',
  });

  try {
    await run({
      window,
      root: container as unknown as HTMLElement,
      animations,
      visibility,
      getMeasurements: () => measurementReads,
      setContainerWidth: (width) => {
        containerWidth = width;
      },
      resize,
      load: async (path) =>
        vite.ssrLoadModule('/src/' + path) as Promise<Record<string, unknown>>,
    });
  } finally {
    await vite.close();
    container.remove();
    for (const key of keys) {
      const descriptor = before.get(key);
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
    await window.happyDOM.abort();
  }
}

test('marquee uses compositor animation, a fixed copy gap and observer lifecycle', async () => {
  await withFixture(
    async ({
      root,
      animations,
      visibility,
      getMeasurements,
      setContainerWidth,
      resize,
      load,
    }) => {
      const { default: TextMarquee } = await load('text-marquee.tsx');
      const { createRoot } = await import('react-dom/client');
      const reactRoot = createRoot(root);
      const content = () =>
        root.querySelector<HTMLElement>('[data-slot="text-marquee-content"]');
      const longText =
        'long scrolling marquee text that exceeds the container width';

      try {
        await act(async () => {
          reactRoot.render(
            h(
              TextMarquee as React.ComponentType<{
                children?: string;
                gap?: number;
                pauseDuration?: number;
                speed?: number;
              }>,
              {
                gap: 32,
                pauseDuration: 0.5,
                speed: 100,
              },
              longText,
            ),
          );
        });
        assert.equal(
          animations.length,
          1,
          'overflow should create one WAAPI animation',
        );
        assert.equal(animations[0].options.iterations, Infinity);
        assert.equal(animations[0].options.easing, 'linear');
        assert.equal(
          content()?.querySelectorAll('[data-slot="text-marquee-content-item"]')
            .length,
          2,
        );
        const copy = content()?.querySelector<HTMLElement>('[data-index="1"]');
        assert.ok(copy, 'second copy exists');
        assert.ok(
          copy.classList.contains('absolute'),
          'second copy stays out of flow',
        );
        assert.equal(copy.style.left, longText.length * 8 + 32 + 'px');
        assert.deepEqual(
          animations[0].frames.at(-1)?.transform,
          'translateX(-' + (longText.length * 8 + 32) + 'px)',
        );
        assert.equal(visibility.length, 1);
        assert.equal(
          visibility[0].observed[0]?.getAttribute('data-slot'),
          'text-marquee',
          'observe container, not translated content',
        );

        visibility[0].emit(false);
        assert.equal(animations[0].playState, 'paused');
        visibility[0].emit(true);
        assert.equal(animations[0].playState, 'running');

        const reads = getMeasurements();
        await act(async () => {
          reactRoot.render(
            h(
              TextMarquee as React.ComponentType<{
                children?: string;
                gap?: number;
                pauseDuration?: number;
                speed?: number;
              }>,
              {
                gap: 32,
                pauseDuration: 0.5,
                speed: 100,
              },
              longText,
            ),
          );
        });
        assert.equal(
          getMeasurements(),
          reads,
          'unchanged text should not cause width remeasurement',
        );

        setContainerWidth(2000);
        await act(async () => resize[0].emit());
        assert.match(content()?.className ?? '', /truncate/);
        assert.equal(
          animations[0].playState,
          'idle',
          'resizing to fit cancels animation',
        );
        assert.equal(visibility[0].disconnected, true);

        setContainerWidth(100);
        await act(async () => resize[0].emit());
        assert.equal(
          animations.length,
          2,
          'resizing back to overflow restarts animation',
        );

        await act(async () => reactRoot.unmount());
        assert.equal(resize[0].disconnected, true);
        assert.equal(animations.at(-1)?.playState, 'idle');
        assert.equal(visibility.at(-1)?.disconnected, true);
      } finally {
        // React unmount is idempotent only when this test has not unmounted yet.
        if (root.firstChild) await act(async () => reactRoot.unmount());
      }
    },
  );
});

test('fitting marquee renders one truncated copy without animation', async () => {
  await withFixture(async ({ root, animations, load }) => {
    const { default: TextMarquee } = await load('text-marquee.tsx');
    const { createRoot } = await import('react-dom/client');
    const reactRoot = createRoot(root);
    try {
      await act(async () => {
        reactRoot.render(
          h(
            TextMarquee as React.ComponentType<{ children: string }>,
            null,
            'short',
          ),
        );
      });
      assert.equal(animations.length, 0);
      assert.equal(
        root.querySelectorAll('[data-slot="text-marquee-content-item"]').length,
        0,
      );
      assert.match(
        root.querySelector('[data-slot="text-marquee-content"]')?.className ??
          '',
        /truncate/,
      );
    } finally {
      await act(async () => reactRoot.unmount());
    }
  });
});

test('dropdown exposes the historical main E2E locator on the reference menu surface', async () => {
  await withFixture(async ({ root, load }) => {
    const {
      DropdownMenu,
      DropdownMenuTrigger,
      DropdownMenuContent,
      DropdownMenuItem,
    } = await load('dropdown-menu.tsx');
    const { createRoot } = await import('react-dom/client');
    const reactRoot = createRoot(root);
    try {
      await act(async () => {
        reactRoot.render(
          h(
            DropdownMenu as React.ComponentType<{
              defaultOpen: boolean;
              children?: React.ReactNode;
            }>,
            { defaultOpen: true },
            h(
              DropdownMenuTrigger as React.ComponentType<{ children: string }>,
              null,
              'Open',
            ),
            h(
              DropdownMenuContent as React.ComponentType<{
                children: React.ReactNode;
              }>,
              null,
              h(
                DropdownMenuItem as React.ComponentType<{ children: string }>,
                null,
                'Reset',
              ),
            ),
          ),
        );
      });
      assert.match(root.textContent ?? '', /Open/);
      const menu = document.querySelector<HTMLElement>(
        '[data-slot="main-dropdown-menu-motion-content"]',
      );
      assert.ok(menu, 'MenuSurface must retain the desktop E2E locator');
      assert.ok(menu.className.includes('rounded'));
      assert.match(menu.textContent ?? '', /Reset/);

      // Named consumers (theme/language selectors) need their own locator;
      // the shared legacy main locator only applies when no slot is passed.
      await act(async () => {
        reactRoot.render(
          h(
            DropdownMenu as React.ComponentType<{
              defaultOpen: boolean;
              children?: React.ReactNode;
            }>,
            { defaultOpen: true },
            h(
              DropdownMenuTrigger as React.ComponentType<{ children: string }>,
              null,
              'Open',
            ),
            h(
              DropdownMenuContent as React.ComponentType<{
                children: React.ReactNode;
                'data-slot'?: string;
              }>,
              { 'data-slot': 'theme-mode-selector-menu' },
              h(
                DropdownMenuItem as React.ComponentType<{ children: string }>,
                null,
                'Dark',
              ),
            ),
          ),
        );
      });
      const namedMenu = document.querySelector<HTMLElement>(
        '[data-slot="theme-mode-selector-menu"]',
      );
      assert.ok(namedMenu, 'Caller data-slot must survive Radix asChild');
      assert.equal(
        document.querySelector(
          '[data-slot="main-dropdown-menu-motion-content"]',
        ),
        null,
        'The default locator must not override a named selector',
      );
      assert.match(namedMenu.textContent ?? '', /Dark/);
    } finally {
      await act(async () => reactRoot.unmount());
    }
  });
});
