import { cn } from '@chimera/utils';
import { motion, useAnimationControls } from 'framer-motion';
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from 'react';

const sleep = (ms: number) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms);
  });

export default function TextMarquee({
  children,
  className,
  speed = 30,
  gap = 32,
  pauseDuration = 1,
  fadeEdges = false,
  fadeWidth = 12,
}: {
  children: ReactNode;
  className?: string;
  speed?: number;
  gap?: number;
  pauseDuration?: number;
  fadeEdges?: boolean;
  fadeWidth?: number;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const textRef = useRef<HTMLDivElement>(null);
  const [shouldAnimate, setShouldAnimate] = useState(false);
  const [textWidth, setTextWidth] = useState(0);
  const controls = useAnimationControls();

  const checkOverflow = useCallback(() => {
    if (!containerRef.current || !textRef.current) {
      return;
    }

    const containerWidth = containerRef.current.offsetWidth;
    const contentWidth = textRef.current.scrollWidth;

    setTextWidth(contentWidth);
    setShouldAnimate(contentWidth > containerWidth);
  }, []);

  useEffect(() => {
    checkOverflow();

    const resizeObserver = new ResizeObserver(checkOverflow);

    if (containerRef.current) {
      resizeObserver.observe(containerRef.current);
    }

    return () => {
      resizeObserver.disconnect();
    };
  }, [checkOverflow, children]);

  useEffect(() => {
    if (!shouldAnimate) {
      controls.set({ x: 0 });
      return;
    }

    const totalDistance = textWidth + gap;
    const duration = totalDistance / speed;
    let cancelled = false;

    const runAnimationLoop = async () => {
      await sleep(pauseDuration * 1000);

      if (cancelled) {
        return;
      }

      await controls.start({
        x: -totalDistance,
        transition: {
          duration,
          ease: 'linear',
        },
      });

      if (cancelled) {
        return;
      }

      controls.set({ x: 0 });
      void runAnimationLoop();
    };

    void runAnimationLoop();

    return () => {
      cancelled = true;
      controls.stop();
    };
  }, [controls, gap, pauseDuration, shouldAnimate, speed, textWidth]);

  return (
    <div
      ref={containerRef}
      className={cn(
        'overflow-hidden',
        fadeEdges && [
          'mask-[linear-gradient(to_right,transparent_0,black_var(--marquee-fade-width),black_calc(100%-var(--marquee-fade-width)),transparent_100%)]',
          '[-webkit-mask-image:linear-gradient(to_right,transparent_0,black_var(--marquee-fade-width),black_calc(100%-var(--marquee-fade-width)),transparent_100%)]',
        ],
        className,
      )}
      data-slot="text-marquee"
      style={
        fadeEdges
          ? ({
              '--marquee-fade-width': `${fadeWidth}px`,
            } as CSSProperties)
          : {}
      }
    >
      {shouldAnimate ? (
        <motion.div
          className="flex whitespace-nowrap"
          animate={controls}
          data-slot="text-marquee-content"
        >
          <span
            ref={textRef}
            data-slot="text-marquee-content-item"
            data-index="0"
          >
            {children}
          </span>
          <span
            style={{ paddingLeft: gap }}
            data-slot="text-marquee-content-item"
            data-index="1"
          >
            {children}
          </span>
        </motion.div>
      ) : (
        <div
          ref={textRef}
          className="truncate"
          data-slot="text-marquee-content"
        >
          {children}
        </div>
      )}
    </div>
  );
}
