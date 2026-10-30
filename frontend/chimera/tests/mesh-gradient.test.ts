import assert from 'node:assert/strict';
import test from 'node:test';
import {
  drawMeshGradient,
  type MeshGradientColor,
} from '../src/pages/(main)/main/profiles/_modules/mesh-gradient';

const draw = (
  colors: MeshGradientColor[],
  width = 12,
  height = 4,
  time = 0,
) => {
  const pixels = new Uint8ClampedArray(width * height * 4);
  drawMeshGradient(pixels, width, height, time, {
    colors,
    distortion: 0.5,
    swirl: 0.1,
  });
  return pixels;
};

const pixelsOf = (data: Uint8ClampedArray) =>
  Array.from({ length: data.length / 4 }, (_, index) =>
    Array.from(data.subarray(index * 4, index * 4 + 4)),
  );

test('a single color fills the mesh buffer with the original opaque RGB values', () => {
  for (const pixel of pixelsOf(draw([[0.2, 0.4, 0.6]]))) {
    assert.deepEqual(pixel, [51, 102, 153, 255]);
  }
});

test('wide meshes blend two colors with an opaque alpha channel', () => {
  const pixels = pixelsOf(
    draw([
      [1, 0, 0],
      [0, 0, 1],
    ]),
  );
  for (const [r, g, b, a] of pixels) {
    assert.equal(g, 0);
    assert.ok(r + b >= 254 && r + b <= 256);
    assert.equal(a, 255);
  }
  assert.ok(Math.max(...pixels.map(([r]) => r)) > 128);
  assert.ok(Math.max(...pixels.map(([, , b]) => b)) > 128);
});

test('the mesh clock moves colors without changing its buffer bounds', () => {
  const colors: MeshGradientColor[] = [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
  ];
  const before = draw(colors, 8, 12, 0);
  const after = draw(colors, 8, 12, 10);
  assert.notDeepEqual(before, after);
  assert.equal(before.length, 8 * 12 * 4);
  for (const pixel of pixelsOf(after)) {
    assert.equal(pixel[3], 255);
  }
});
