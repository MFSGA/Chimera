import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';

// Git for Windows may check out the vendored Rust source with CRLF. Normalize
// line endings before extracting the raw JS string so the contract runs on
// both Windows and Unix runners.
const executorSource = readFileSync(
  path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    '../backend/tauri/vendor/tauri-plugin-wdio-webdriver/src/platform/executor.rs',
  ),
  'utf8',
).replace(
  new RegExp(String.fromCharCode(13) + String.fromCharCode(10), 'g'),
  String.fromCharCode(10),
);

function embeddedWheelScript(deltaX: number, deltaY: number) {
  const method = executorSource.indexOf('async fn dispatch_scroll_event(');
  assert.ok(method >= 0, 'The embedded driver wheel executor is missing');
  const start = executorSource.indexOf('r"(function() {{', method);
  const end = executorSource.indexOf('"\n        );', start);
  assert.ok(start > method && end > start, 'The wheel script format changed');
  return executorSource
    .slice(start + 2, end)
    .replaceAll('{{', '{')
    .replaceAll('}}', '}')
    .replaceAll('{x}', '25')
    .replaceAll('{y}', '35')
    .replaceAll('{delta_x}', String(deltaX))
    .replaceAll('{delta_y}', String(deltaY));
}

class ScrollElement {
  parentElement: ScrollElement | null = null;
  scrollTop = 0;
  scrollLeft = 0;
  scrollHeight = 100;
  clientHeight = 100;
  scrollWidth = 100;
  clientWidth = 100;
  cancelWheel = false;
  style = {
    overflowX: 'visible',
    overflowY: 'visible',
    overscrollBehaviorX: 'auto',
    overscrollBehaviorY: 'auto',
  };

  dispatchEvent(event: { preventDefault: () => void }) {
    if (this.cancelWheel) event.preventDefault();
    return !this.cancelWheel;
  }

  scrollBy({ left = 0, top = 0 }: { left?: number; top?: number }) {
    this.scrollLeft = Math.max(
      0,
      Math.min(this.scrollWidth - this.clientWidth, this.scrollLeft + left),
    );
    this.scrollTop = Math.max(
      0,
      Math.min(this.scrollHeight - this.clientHeight, this.scrollTop + top),
    );
  }
}

function runWheel({ cancel = false, contain = false, deltaY = 360 } = {}) {
  const leaf = new ScrollElement();
  const viewport = new ScrollElement();
  leaf.parentElement = viewport;
  leaf.cancelWheel = cancel;
  viewport.scrollHeight = 1954;
  viewport.clientHeight = 541;
  viewport.style.overflowY = 'scroll';
  viewport.style.overscrollBehaviorY = contain ? 'contain' : 'auto';
  const windowScrolls: number[] = [];
  class WheelEvent {
    defaultPrevented = false;
    constructor(_type: string, _options: object) {}
    preventDefault() {
      this.defaultPrevented = true;
    }
  }
  const context = {
    document: { elementFromPoint: () => leaf, body: new ScrollElement() },
    HTMLElement: ScrollElement,
    WheelEvent,
    getComputedStyle: (element: ScrollElement) => element.style,
    window: { scrollBy: ({ top }: { top: number }) => windowScrolls.push(top) },
  };
  assert.equal(runInNewContext(embeddedWheelScript(0, deltaY), context), true);
  return { viewport, windowScrolls };
}

test('embedded wheel scrolls the nearest nested ScrollArea rather than the window', () => {
  const { viewport, windowScrolls } = runWheel();
  assert.equal(viewport.scrollTop, 360);
  assert.deepEqual(windowScrolls, []);
});

test('embedded wheel honors preventDefault instead of changing any scroll state', () => {
  const { viewport, windowScrolls } = runWheel({ cancel: true });
  assert.equal(viewport.scrollTop, 0);
  assert.deepEqual(windowScrolls, []);
});

test('embedded wheel respects overscroll containment at the nested viewport', () => {
  const { viewport, windowScrolls } = runWheel({ contain: true, deltaY: 2000 });
  assert.equal(viewport.scrollTop, 1954 - 541);
  assert.deepEqual(windowScrolls, []);
});

test('embedded wheel delegates remaining distance to the window when not contained', () => {
  const { viewport, windowScrolls } = runWheel({ deltaY: 2000 });
  assert.equal(viewport.scrollTop, 1954 - 541);
  assert.deepEqual(windowScrolls, [2000 - (1954 - 541)]);
});
