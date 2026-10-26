import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import type { TopologyEdge, TopologyNode } from '@chimera/interface';
import {
  layoutSankey,
  NODE_GAP,
  sankeyMinHeight,
} from '../src/pages/(main)/main/topology/flow/_modules/sankey-layout';

// Unit contract: topology values determine shared node and band scales. Missing
// endpoints and zero-value edges stay out of the drawing; zero-value nodes keep
// a visible minimum height. These assertions fail if the layout breaks any of
// those observable geometry rules.
const usage = (download: number) => ({
  bytes: { upload: 0, download },
  connections: 1,
});

const node = (id: string, layer: number, download: number): TopologyNode => ({
  id,
  layer,
  key: id,
  usage: usage(download),
});

const edge = (
  source: string,
  target: string,
  download: number,
): TopologyEdge => ({
  source,
  target,
  usage: usage(download),
});

const nodes = [
  node('a', 0, 60),
  node('b', 0, 40),
  node('x', 1, 70),
  node('y', 1, 30),
];
const edges = [
  edge('a', 'x', 50),
  edge('a', 'y', 10),
  edge('b', 'x', 20),
  edge('b', 'y', 20),
];

const layout = () =>
  layoutSankey({
    nodes,
    edges,
    layerCount: 2,
    metric: 'bytes',
    width: 408,
    height: 210,
  });

describe('reference Sankey topology layout', () => {
  it('stacks nodes by value and spans the available width', () => {
    const placed = new Map(layout().nodes.map((item) => [item.node.id, item]));

    assert.deepEqual(placed.get('a'), {
      node: nodes[0],
      x: 0,
      y: 0,
      height: 0.6 * (210 - NODE_GAP),
    });
    assert.equal(placed.get('x')?.x, 400);
    assert.ok(
      Math.abs(placed.get('b')!.y - (placed.get('a')!.height + NODE_GAP)) <
        0.001,
    );
  });

  it('draws each band on the same scale as its usage value', () => {
    const result = layout();
    const band = result.bands.find((item) => item.id === 'a->x');

    assert.equal(result.bands.length, 4);
    assert.ok(band);
    assert.ok(Math.abs(band.thickness - 0.5 * (210 - NODE_GAP)) < 0.001);
    assert.match(band.path, /^M8 /);
    assert.match(band.centerline, /^M8 /);
  });

  it('omits edges without two nodes or positive usage', () => {
    const result = layoutSankey({
      nodes,
      edges: [...edges, edge('a', 'missing', 5), edge('b', 'x', 0)],
      layerCount: 2,
      metric: 'bytes',
      width: 408,
      height: 210,
    });

    assert.equal(result.bands.length, 4);
    assert.ok(!result.bands.some((band) => band.id === 'a->missing'));
  });

  it('keeps zero-usage nodes visible with the minimum height', () => {
    const result = layoutSankey({
      nodes: [node('a', 0, 0), node('x', 1, 0)],
      edges: [],
      layerCount: 2,
      metric: 'bytes',
      width: 100,
      height: 100,
    });

    assert.ok(result.nodes.every((item) => item.height > 0));
  });

  it('uses the tallest column when calculating the minimum canvas height', () => {
    assert.equal(sankeyMinHeight([]), 0);
    assert.equal(sankeyMinHeight(nodes), 2 * sankeyMinHeight([nodes[0]]));
  });
});
