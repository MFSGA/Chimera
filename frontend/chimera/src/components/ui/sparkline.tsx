/**
 * Sparkline 迷你趋势图组件
 *
 * 迁移自 ref: `src/components/ui/sparkline.tsx`
 *
 * 职责：
 * - 使用 D3.js 渲染平滑的迷你趋势曲线（带面积填充）
 * - 支持数据追加动画：旧数据滑出左侧，新数据切入右侧
 * - 智能 y 轴缩放：稳定数据占上方 1/3，波动数据占满高度
 * - 使用 Catmull-Rom 样条曲线生成平滑线条
 * - 护点（guard points）技术消除端点切线抖动
 *
 * 动画机制：
 * 1. SVG translateX 实现水平滚动（线性缓动，恒定速度）
 * 2. D3 easeCubicInOut 实现 yMax 插值（非线性缓动，自然感）
 * 3. 二者解耦，互不影响
 * 4. 使用 motion animate 进行高性能动画驱动
 */

import { cn } from '@chimera/ui';
import * as d3 from 'd3';
import { cloneDeep } from 'lodash-es';
import { animate } from 'motion';
import { useEffect, useRef, type ComponentPropsWithoutRef } from 'react';

/**
 * 变异系数（标准差/均值）阈值
 * 低于此阈值认为序列"稳定"，图表只占 SVG 高度的下方 1/3
 */
const STABLE_CV_THRESHOLD = 0.15;

/**
 * 稳定序列时，图表占用 SVG 高度的比例
 * topFactor = 2/3 → 可用波段 = height - height * (2/3) = h/3
 */
const STABLE_TOP_FACTOR = 2 / 3;

/** 波动序列时，图表占用 SVG 高度的比例 */
const ACTIVE_TOP_FACTOR = 0.35;

export const Sparkline = ({
  data,
  animationDuration = 1,
  className,
  ...props
}: ComponentPropsWithoutRef<'svg'> & {
  data: number[];
  animationDuration?: number;
}) => {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const gRef = useRef<SVGGElement | null>(null);
  const prevDataRef = useRef<number[] | null>(null);
  // 最近一次滚动到左侧之外的点值，用于保持 x=0 处的曲线连续
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

    /**
     * 生成折线和面积路径字符串
     */
    const makePaths = (
      points: number[],
      xRange: [number, number],
      yMax: number,
    ) => {
      const mean = d3.mean(points) ?? 0;
      const std = d3.deviation(points) ?? 0;
      const cv = mean > 0 ? std / mean : 0;
      // 稳定序列只占底部 1/3，波动序列占满
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

      // Catmull-Rom 样条（alpha=0.5）生成平滑曲线
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

    /**
     * 构建带护点的路径
     *
     * 护点（guard points）技术：
     * 在数据序列两端各添加一个虚拟点（线性外推），使所有真实数据点成为样条曲线的
     * 内部节点，消除端点切向不连续性导致的边界抖动。
     * SVG overflow:hidden 自动裁剪护点区域。
     */
    const buildPaths = (
      points: number[],
      xRange: [number, number],
      yMax: number,
      step: number,
      leftGuard?: number,
    ) => {
      const n = points.length;

      // 空数据或单点数据直接返回
      if (n === 0) {
        return { line: '', area: '' };
      }

      if (n === 1) {
        return makePaths(points, xRange, yMax);
      }

      // 左侧护点：线性外推（2 * p0 - p1）
      // 右侧护点：线性外推（2 * pn-1 - pn-2）
      const lGuard = leftGuard ?? 2 * points[0] - points[1];
      const rGuard = 2 * points[n - 1] - points[n - 2];

      return makePaths(
        [lGuard, ...points, rGuard],
        [xRange[0] - step, xRange[1] + step],
        yMax,
      );
    };

    const prevData = prevDataRef.current;
    prevDataRef.current = cloneDeep(data);

    // 停止正在进行的动画
    animRef.current?.stop();
    animRef.current = null;

    // 短数据序列（<2 点）直接渲染，无动画
    if (data.length < 2) {
      g.selectAll('*').remove();
      g.attr('transform', 'translate(0,0)');
      leftGuardRef.current = null;
      return;
    }

    if (!prevData || prevData.length !== data.length) {
      // 初次渲染或数据长度变化：直接绘制，无动画
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

    // 数据追加动画：
    // N+1 个点 = 旧数据首点（即将滑出）+ 完整新数据集
    const stepWidth = width / (data.length - 1);
    const extPoints = [...prevData, data[data.length - 1]];
    const fromYMax = Math.max(d3.max(extPoints) ?? 0, 1);
    const toYMax = Math.max(d3.max(data) ?? 0, 1);
    const yMaxChanges = Math.abs(fromYMax - toYMax) > 1;

    const leftGuard = leftGuardRef.current ?? undefined;

    // 渲染动画初始状态（N+1 点路径）
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

    // 使用 motion animate 驱动动画
    const anim = animate(0, 1, {
      duration: animationDuration,
      ease: 'linear',
      onUpdate(t) {
        // X 轴：线性平移（恒定速度滑动）
        g.attr('transform', `translate(${-stepWidth * t},0)`);

        // Y 轴：使用缓动函数插值 yMax
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

        // 保存滑出的点作为下一个周期的左护点
        leftGuardRef.current = prevData[0];

        // 动画完成后切换到 N 点路径（无缝切换，因为 N+1 路径 t=1 和 N 路径重合）
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
