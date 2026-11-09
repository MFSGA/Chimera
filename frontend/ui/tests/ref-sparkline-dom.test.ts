/**
 * DOM lifecycle regressions adapted from ref's Sparkline browser contract.
 * Happy DOM provides React/DOM lifecycle; ResizeObserver and WAAPI are stubbed.
 * Real compositor timing and CSS layout still require an actual browser run.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { Window } from 'happy-dom';
import { act, createElement as h, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { createServer } from 'vite';

type ResizeRecord = {
  target: Element | null;
  disconnected: boolean;
  emit: (width: number, height: number) => void;
};

type AnimationRecord = {
  name: 'slide' | 'stretch';
  frames: Keyframe[];
  options: KeyframeAnimationOptions;
  cancelled: boolean;
  playState: AnimationPlayState;
  onfinish: (() => void) | null;
  finish: () => void;
};

const series = (start: number) =>
  Array.from({ length: 10 }, (_, index) => ((start + index) * 37) % 100);

async function withSparklineFixture(
  run: (context: {
    rootElement: HTMLElement;
    Sparkline: React.ComponentType<{
      data: number[];
      animationDuration?: number;
      className?: string;
    }>;
    resize: ResizeRecord[];
    animations: AnimationRecord[];
    layoutReads: () => number;
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
    'SVGElement',
    'Event',
    'MutationObserver',
    'ResizeObserver',
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
  assign('SVGElement', window.SVGElement);
  assign('Event', window.Event);
  assign('MutationObserver', window.MutationObserver);
  assign('getComputedStyle', window.getComputedStyle.bind(window));
  assign('requestAnimationFrame', window.requestAnimationFrame.bind(window));
  assign('cancelAnimationFrame', window.cancelAnimationFrame.bind(window));
  assign('IS_REACT_ACT_ENVIRONMENT', true);

  const resize: ResizeRecord[] = [];
  class ResizeObserverStub {
    readonly record: ResizeRecord;
    constructor(private readonly notify: ResizeObserverCallback) {
      this.record = {
        target: null,
        disconnected: false,
        emit: (width, height) => {
          if (!this.record.target) {
            throw new Error('Sparkline root was not observed');
          }
          this.notify(
            [
              {
                target: this.record.target,
                contentRect: { width, height } as DOMRectReadOnly,
              } as ResizeObserverEntry,
            ],
            this as unknown as ResizeObserver,
          );
        },
      };
      resize.push(this.record);
    }
    observe(target: Element) {
      this.record.target = target;
    }
    unobserve() {}
    disconnect() {
      this.record.disconnected = true;
    }
  }
  assign('ResizeObserver', ResizeObserverStub);
  Object.defineProperty(window, 'ResizeObserver', {
    configurable: true,
    value: ResizeObserverStub,
  });

  const animations: AnimationRecord[] = [];
  Object.defineProperty(window.Element.prototype, 'animate', {
    configurable: true,
    value(
      this: Element,
      frames: Keyframe[],
      options: KeyframeAnimationOptions,
    ) {
      const name = (this as HTMLElement).className.includes('origin-bottom')
        ? 'stretch'
        : 'slide';
      const record: AnimationRecord = {
        name,
        frames,
        options,
        cancelled: false,
        playState: 'running',
        onfinish: null,
        finish() {
          this.playState = 'finished';
          this.onfinish?.();
        },
      };
      animations.push(record);
      return {
        effect: { getComputedTiming: () => ({ progress: 0.5 }) },
        cancel() {
          record.cancelled = true;
          record.playState = 'idle';
        },
        get playState() {
          return record.playState;
        },
        set onfinish(callback: (() => void) | null) {
          record.onfinish = callback;
        },
        get onfinish() {
          return record.onfinish;
        },
      } as unknown as Animation;
    },
  });

  let layoutReads = 0;
  Object.defineProperty(window.Element.prototype, 'getBoundingClientRect', {
    configurable: true,
    value() {
      layoutReads++;
      return {
        x: 0,
        y: 0,
        top: 0,
        left: 0,
        right: 300,
        bottom: 100,
        width: 300,
        height: 100,
        toJSON: () => ({}),
      } as DOMRect;
    },
  });

  const rootElement = window.document.createElement('div');
  window.document.body.append(rootElement);
  const vite = await createServer({
    configFile: false,
    root: fileURLToPath(new URL('../', import.meta.url)),
    oxc: { jsx: { runtime: 'automatic' } },
    server: { middlewareMode: true, ws: false },
    appType: 'custom',
    logLevel: 'error',
  });

  try {
    const module = (await vite.ssrLoadModule('/src/sparkline.tsx')) as {
      Sparkline: React.ComponentType<{
        data: number[];
        animationDuration?: number;
        className?: string;
      }>;
    };
    await run({
      rootElement: rootElement as unknown as HTMLElement,
      Sparkline: module.Sparkline,
      resize,
      animations,
      layoutReads: () => layoutReads,
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

test('Sparkline uses ResizeObserver for resize and skips layout reads on equal data', async () => {
  await withSparklineFixture(
    async ({ rootElement, Sparkline, resize, layoutReads }) => {
      const controls = { rerender: () => {} };
      const Owner = () => {
        const [, setTick] = useState(0);
        controls.rerender = () => setTick((tick) => tick + 1);
        return h(Sparkline, { data: series(0), animationDuration: 0.4 });
      };
      const root = createRoot(rootElement);
      try {
        await act(async () => root.render(h(Owner)));
        assert.equal(resize.length, 1);
        await act(async () => resize[0].emit(300, 100));
        const line = rootElement.querySelector('path.line');
        assert.ok(line?.getAttribute('d'));
        const beforeResize = line.getAttribute('d');

        await act(async () => controls.rerender());
        assert.equal(layoutReads(), 0);
        assert.equal(line.getAttribute('d'), beforeResize);

        await act(async () => resize[0].emit(600, 100));
        assert.notEqual(line.getAttribute('d'), beforeResize);
      } finally {
        await act(async () => root.unmount());
      }
      assert.equal(resize[0].disconnected, true);
    },
  );
});

test('Sparkline continues an in-flight sample scroll with WAAPI and cleans up', async () => {
  await withSparklineFixture(
    async ({ rootElement, Sparkline, resize, animations }) => {
      const controls = { push: () => {} };
      const Owner = () => {
        const [start, setStart] = useState(0);
        controls.push = () => setStart((value) => value + 1);
        return h(Sparkline, { data: series(start), animationDuration: 0.4 });
      };
      const root = createRoot(rootElement);
      try {
        await act(async () => root.render(h(Owner)));
        await act(async () => resize[0].emit(300, 100));
        assert.ok(rootElement.querySelector('path.line')?.getAttribute('d'));
        await act(async () => controls.push());
        const first = animations.find(
          (animation) => animation.name === 'slide',
        );
        assert.ok(first);
        assert.deepEqual(first.options, {
          duration: 400,
          easing: 'linear',
          fill: 'forwards',
        });
        await act(async () => controls.push());
        assert.equal(first.cancelled, true);
        const slides = animations.filter(
          (animation) => animation.name === 'slide',
        );
        assert.equal(slides.length, 2);
        assert.match(String(slides[1].frames[0].transform), /translateX\(-16/);
        slides[1].finish();
        assert.equal(slides[1].cancelled, true);
        assert.ok(rootElement.querySelector('path.line')?.getAttribute('d'));
      } finally {
        await act(async () => root.unmount());
      }
      assert.equal(resize[0].disconnected, true);
      assert.ok(animations.every((animation) => animation.cancelled));
    },
  );
});

test('Sparkline avoids a vertical stretch animation for the first nonzero peak', async () => {
  await withSparklineFixture(
    async ({ rootElement, Sparkline, resize, animations }) => {
      const controls = { peak: () => {} };
      const Owner = () => {
        const [data, setData] = useState<number[]>(Array(10).fill(0));
        controls.peak = () =>
          setData((previous) => [...previous.slice(1), 10_000]);
        return h(Sparkline, { data, animationDuration: 0.4 });
      };
      const root = createRoot(rootElement);
      try {
        await act(async () => root.render(h(Owner)));
        await act(async () => resize[0].emit(300, 100));
        await act(async () => controls.peak());
        assert.equal(
          animations.filter((animation) => animation.name === 'slide').length,
          1,
        );
        assert.equal(
          animations.filter((animation) => animation.name === 'stretch').length,
          0,
        );
      } finally {
        await act(async () => root.unmount());
      }
      assert.equal(resize[0].disconnected, true);
      assert.ok(animations.every((animation) => animation.cancelled));
    },
  );
});
