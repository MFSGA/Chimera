import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { runInNewContext } from 'node:vm';

// Extract the real JS embedded in Rust, not a reimplementation of the driver.
// Git for Windows may have converted the vendored source to CRLF.
const source = readFileSync(
  path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    '../backend/tauri/vendor/tauri-plugin-wdio-webdriver/src/platform/executor.rs',
  ),
  'utf8',
).replace(
  new RegExp(String.fromCharCode(13) + String.fromCharCode(10), 'g'),
  String.fromCharCode(10),
);
const method = source.indexOf('async fn dispatch_pointer_event(');
assert.ok(method >= 0, 'Embedded pointer executor is missing');

function rawRustScript(arm: string) {
  const armStart = source.indexOf(arm, method);
  assert.ok(armStart > method, 'Pointer arm missing: ' + arm);
  const start = source.indexOf('r#"', armStart) + 3;
  const end = source.indexOf('"#', start);
  assert.ok(start > armStart && end > start, 'Pointer arm format changed');
  return source
    .slice(start, end)
    .replaceAll('{{', '{')
    .replaceAll('}}', '}')
    .replaceAll('{x}', '35')
    .replaceAll('{y}', '42')
    .replaceAll('{button}', '0')
    .replaceAll('{buttons}', '0');
}

const moveScript = rawRustScript('PointerEventType::Move => format!(');
const buttonScript = rawRustScript(
  'PointerEventType::Down | PointerEventType::Up => {',
);

class FakeEvent {
  readonly isTrusted = false;
  readonly type: string;
  readonly pointerType: string | undefined;
  readonly relatedTarget: FakeElement | null;

  constructor(type: string, init: Record<string, unknown> = {}) {
    this.type = type;
    this.pointerType = init.pointerType as string | undefined;
    this.relatedTarget =
      (init.relatedTarget as FakeElement | undefined) ?? null;
  }
}

class FakeElement {
  isConnected = true;
  parentElement: FakeElement | null = null;
  events: FakeEvent[] = [];

  contains(element: FakeElement | null): boolean {
    for (let current = element; current; current = current.parentElement) {
      if (current === this) return true;
    }
    return false;
  }

  dispatchEvent(event: FakeEvent) {
    this.events.push(event);
    return true;
  }
}

function createHarness() {
  const first = new FakeElement();
  const second = new FakeElement();
  let selected = first;
  const browserWindow = {} as { __chimeraE2eHoveredElement?: FakeElement };
  const context = {
    document: {
      body: new FakeElement(),
      elementFromPoint: () => selected,
    },
    window: browserWindow,
    PointerEvent: FakeEvent,
    MouseEvent: FakeEvent,
  };
  const runScript = (code: string) =>
    runInNewContext(
      '(function(){ var el = document.elementFromPoint(35, 42); ' +
        'if (!el) el = document.body; ' +
        code +
        ' return true; })()',
      context,
    );
  return {
    first,
    second,
    browserWindow,
    select: (target: FakeElement) => {
      selected = target;
    },
    move: () => runScript(moveScript),
    press: (name: 'pointerdown' | 'pointerup') =>
      runScript(buttonScript.replaceAll('{pointer_name}', name)),
  };
}

test('embedded mouse move delivers a PointerEvent to Radix and a legacy mousemove', () => {
  const harness = createHarness();
  assert.equal(harness.move(), true);
  assert.deepEqual(
    harness.first.events.map((event) => event.type),
    ['pointerover', 'pointerenter', 'mouseover', 'pointermove'],
  );
  assert.equal(harness.first.events[3]?.pointerType, 'mouse');
  assert.equal(harness.first.events[3]?.isTrusted, false);
  assert.equal(harness.browserWindow.__chimeraE2eHoveredElement, harness.first);
});

test('embedded pointer transitions emit an exit and new pointerover, with related targets', () => {
  const harness = createHarness();
  harness.move();
  harness.select(harness.second);
  harness.move();
  assert.deepEqual(
    harness.first.events.map((event) => event.type),
    [
      'pointerover',
      'pointerenter',
      'mouseover',
      'pointermove',
      'pointerout',
      'pointerleave',
      'mouseout',
    ],
  );
  assert.deepEqual(
    harness.second.events.map((event) => event.type),
    ['pointerover', 'pointerenter', 'mouseover', 'pointermove'],
  );
  assert.equal(harness.first.events[4]?.relatedTarget, harness.second);
  assert.equal(harness.second.events[0]?.relatedTarget, harness.first);
});

test('moving between siblings leaves neither their shared ancestor nor the document', () => {
  const harness = createHarness();
  const sharedParent = new FakeElement();
  harness.first.parentElement = sharedParent;
  harness.second.parentElement = sharedParent;

  harness.move();
  harness.select(harness.second);
  harness.move();

  assert.ok(
    !sharedParent.events.some((event) => event.type === 'pointerleave'),
    'A pointer moving between siblings must not leave the common parent.',
  );
  assert.equal(
    harness.first.events.filter((event) => event.type === 'pointerleave')
      .length,
    1,
  );
  assert.equal(
    harness.second.events.filter((event) => event.type === 'pointerenter')
      .length,
    1,
  );
});

test('leaving a nested Radix trigger emits pointerleave on its ancestor', () => {
  const harness = createHarness();
  const tooltipTrigger = new FakeElement();
  harness.first.parentElement = tooltipTrigger;

  harness.move();
  harness.select(harness.second);
  harness.move();

  const exit = tooltipTrigger.events.find(
    (event) => event.type === 'pointerleave',
  );
  assert.ok(
    exit,
    'The trigger must receive its native pointerleave lifecycle.',
  );
  assert.equal(exit.relatedTarget, harness.second);
  assert.equal(exit.pointerType, 'mouse');
});

test('repeated moves on the same target do not emit duplicate pointerover', () => {
  const harness = createHarness();
  harness.move();
  harness.move();
  assert.deepEqual(
    harness.first.events.map((event) => event.type),
    ['pointerover', 'pointerenter', 'mouseover', 'pointermove', 'pointermove'],
  );
});

test('replaced route DOM targets do not receive stale pointerout events', () => {
  const harness = createHarness();
  harness.move();
  harness.first.isConnected = false;
  harness.select(harness.second);
  harness.move();
  assert.equal(harness.first.events.length, 4);
  assert.equal(harness.second.events[0]?.type, 'pointerover');
  assert.equal(harness.second.events[0]?.relatedTarget, null);
});

test('embedded pointer down/up reach PointerEvent handlers before mouse compatibility events', () => {
  const harness = createHarness();
  harness.press('pointerdown');
  harness.press('pointerup');
  assert.deepEqual(
    harness.first.events.map((event) => event.type),
    ['pointerdown', 'pointerup'],
  );
  assert.equal(harness.first.events[0]?.pointerType, 'mouse');
});
