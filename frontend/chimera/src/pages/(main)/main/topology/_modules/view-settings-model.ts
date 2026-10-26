/** The drawings the flow page can show its paths as. */
export const FLOW_GRAPHS = ['sankey', 'cards'] as const;

export type FlowGraph = (typeof FLOW_GRAPHS)[number];

/** How the flow and map pages are shown, the same on both. */
export type TrafficViewSettings<Ranking extends string = string> = {
  /** The rankings under the drawing, in the order supplied by the caller. */
  rankings: Ranking[];
  flowGraph: FlowGraph;
};

export const TRAFFIC_VIEW_KV_KEY = 'traffic-view';

/** Coerces whatever the KV store holds into the current settings shape. */
export function normalizeTrafficViewSettings<Ranking extends string>(
  value: unknown,
  rankingOrder: readonly Ranking[],
  defaults: TrafficViewSettings<Ranking>,
): TrafficViewSettings<Ranking> {
  const stored = (value ?? {}) as Partial<
    Record<keyof TrafficViewSettings<Ranking>, unknown>
  >;

  return {
    rankings: Array.isArray(stored.rankings)
      ? rankingOrder.filter((dimension) =>
          (stored.rankings as unknown[]).includes(dimension),
        )
      : defaults.rankings,
    flowGraph: FLOW_GRAPHS.includes(stored.flowGraph as FlowGraph)
      ? (stored.flowGraph as FlowGraph)
      : defaults.flowGraph,
  };
}
