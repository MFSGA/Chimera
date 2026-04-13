import * as d3 from 'd3';
import { animate } from 'framer-motion';
import { ComponentPropsWithoutRef, useEffect, useRef } from 'react';
import { cn } from '../../../utils';

const STABLE_CV_THRESHOLD = 0.15;
const STABLE_TOP_FACTOR = 2 / 3;
const ACTIVE_TOP_FACTOR = 0.35;

export const Sparkline = ({
  data,
  animationDuration = 1,
  className,
  ...props
}: ComponentPropsWithoutRef<'svg'> & {
  data: number[];
  animationDuration?: number;
  visible?: boolean;
}) => {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const gRef = useRef<SVGGElement | null>(null);
  const prevDataRef = useRef<number[] | null>(null);
  const leftGuardRef = useRef<number | null>(null);
  const animRef = useRef<ReturnType<typeof animate> | null>(null);

  useEffect(() => {
    if (!svgRef.current || !gRef.current) {
      return;
    }

    const g = d3.select(gRef.current);
    const { width, height } = svgRef.current.getBoundingClientRect();
    if (!width || !height) {
      return;
    }

    const makePaths = (
      points: number[],
      xRange: [number, number],
      yMax: number,
    ) => {
      const mean = d3.mean(points) ?? 0;
      const std = d3.deviation(points) ?? 0;
      const cv = mean > 0 ? std / mean : 0;
      const topFactor =
        yMax === 0
          ? 1
          : cv < STABLE_CV_THRESHOLD
            ? STABLE_TOP_FACTOR
            : ACTIVE_TOP_FACTOR;

      const x = d3
        .scaleLinear()
        .domain([0, points.length - 1])
        .range(xRange);
      const y = d3
        .scaleLinear()
        .domain([0, yMax])
        .range([height, height * topFactor]);

      const lineGen = d3
        .line<number>()
        .x((_, i) => x(i))
        .y((d) => y(d))
        .curve(d3.curveCatmullRom.alpha(0.5));
      const areaGen = d3
        .area<number>()
        .x((_, i) => x(i))
        .y0(height)
        .y1((d) => y(d))
        .curve(d3.curveCatmullRom.alpha(0.5));

      return {
        line: lineGen(points) ?? '',
        area: areaGen(points) ?? '',
      };
    };

    const buildPaths = (
      points: number[],
      xRange: [number, number],
      yMax: number,
      step: number,
      leftGuard?: number,
    ) => {
      const n = points.length;

      if (n === 0) {
        return { line: '', area: '' };
      }

      if (n === 1) {
        return makePaths(points, xRange, yMax);
      }

      const lGuard = leftGuard ?? 2 * points[0] - points[1];
      const rGuard = 2 * points[n - 1] - points[n - 2];

      return makePaths(
        [lGuard, ...points, rGuard],
        [xRange[0] - step, xRange[1] + step],
        yMax,
      );
    };

    const prevData = prevDataRef.current;
    prevDataRef.current = [...data];

    animRef.current?.stop();
    animRef.current = null;

    if (data.length < 2) {
      g.selectAll('*').remove();
      g.attr('transform', 'translate(0,0)');
      leftGuardRef.current = null;
      return;
    }

    if (!prevData || prevData.length !== data.length) {
      const yMax = Math.max(d3.max(data) ?? 0, 1);
      const step = width / (data.length - 1);
      const { line, area } = buildPaths(
        data,
        [0, width],
        yMax,
        step,
        leftGuardRef.current ?? undefined,
      );

      g.selectAll('*').remove();
      g.attr('transform', 'translate(0,0)');
      g.append('path').attr('class', 'area fill-primary/10').attr('d', area);
      g.append('path')
        .attr('class', 'line stroke-primary')
        .attr('fill', 'none')
        .attr('stroke-width', 2)
        .attr('d', line);
      return;
    }

    const stepWidth = width / (data.length - 1);
    const extPoints = [...prevData, data[data.length - 1]];
    const fromYMax = Math.max(d3.max(extPoints) ?? 0, 1);
    const toYMax = Math.max(d3.max(data) ?? 0, 1);
    const yMaxChanges = Math.abs(fromYMax - toYMax) > 1;
    const leftGuard = leftGuardRef.current ?? undefined;

    const { line: initLine, area: initArea } = buildPaths(
      extPoints,
      [0, width + stepWidth],
      fromYMax,
      stepWidth,
      leftGuard,
    );

    g.attr('transform', 'translate(0,0)');
    g.select('.area').attr('d', initArea);
    g.select('.line').attr('d', initLine);

    let cancelled = false;

    const anim = animate(0, 1, {
      duration: animationDuration,
      ease: 'linear',
      onUpdate(t) {
        g.attr('transform', `translate(${-stepWidth * t},0)`);

        if (yMaxChanges) {
          const easedT = d3.easeCubicInOut(t);
          const currentYMax = fromYMax + (toYMax - fromYMax) * easedT;
          const { line, area } = buildPaths(
            extPoints,
            [0, width + stepWidth],
            currentYMax,
            stepWidth,
            leftGuard,
          );

          g.select('.area').attr('d', area);
          g.select('.line').attr('d', line);
        }
      },
      onComplete() {
        if (cancelled) {
          return;
        }

        leftGuardRef.current = prevData[0];

        const { line, area } = buildPaths(
          data,
          [0, width],
          toYMax,
          stepWidth,
          prevData[0],
        );

        g.attr('transform', 'translate(0,0)');
        g.select('.area').attr('d', area);
        g.select('.line').attr('d', line);
      },
    });

    animRef.current = anim;

    return () => {
      cancelled = true;
      anim.stop();
    };
  }, [data, animationDuration]);

  return (
    <svg
      ref={svgRef}
      data-slot="sparkline"
      className={cn('size-full overflow-hidden', className)}
      {...props}
    >
      <g ref={gRef} />
    </svg>
  );
};

export default Sparkline;
