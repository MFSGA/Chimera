import { cn } from '@chimera/utils';
import { ScrollArea as ScrollAreaPrimitive } from 'radix-ui';
import {
  createContext,
  useContext,
  useRef,
  useState,
  type ComponentProps,
  type RefObject,
  type UIEvent,
} from 'react';

interface ScrollAreaContextValue {
  isScrolling: boolean;
  isTop: boolean;
  isBottom: boolean;
  scrollDirection: 'up' | 'down' | 'left' | 'right' | 'none';
  offset: {
    top: number;
    bottom: number;
    left: number;
    right: number;
  };
  viewportRef: RefObject<HTMLDivElement | null>;
}

const ScrollAreaContext = createContext<ScrollAreaContextValue | null>(null);

export function useScrollArea() {
  const context = useContext(ScrollAreaContext);

  if (!context) {
    throw new Error('useScrollArea must be used within a ScrollArea component');
  }

  return context;
}

function useScrollTracking(threshold = 50) {
  const [isScrolling, setIsScrolling] = useState(false);
  const [isTop, setIsTop] = useState(true);
  const [isBottom, setIsBottom] = useState(false);
  const [scrollDirection, setScrollDirection] = useState<
    'up' | 'down' | 'left' | 'right' | 'none'
  >('none');
  const [offset, setOffset] = useState({
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
  });
  const lastScrollTop = useRef(0);
  const lastScrollLeft = useRef(0);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleScroll = (event: UIEvent<HTMLDivElement>) => {
    const {
      scrollTop,
      scrollLeft,
      scrollWidth,
      clientWidth,
      scrollHeight,
      clientHeight,
    } = event.currentTarget;

    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
    }

    setIsScrolling(true);
    setIsTop(scrollTop === 0);
    setIsBottom(scrollHeight - scrollTop - clientHeight < threshold);
    setOffset({
      top: scrollTop,
      left: scrollLeft,
      right: scrollWidth - clientWidth,
      bottom: scrollHeight - clientHeight,
    });

    const deltaY = scrollTop - lastScrollTop.current;
    const deltaX = scrollLeft - lastScrollLeft.current;

    if (Math.abs(deltaY) > Math.abs(deltaX)) {
      if (deltaY > 0) setScrollDirection('down');
      if (deltaY < 0) setScrollDirection('up');
    } else if (Math.abs(deltaX) > Math.abs(deltaY)) {
      if (deltaX > 0) setScrollDirection('right');
      if (deltaX < 0) setScrollDirection('left');
    }

    lastScrollTop.current = scrollTop;
    lastScrollLeft.current = scrollLeft;
    timeoutRef.current = setTimeout(() => {
      setIsScrolling(false);
    }, threshold);
  };

  return {
    isTop,
    isBottom,
    scrollDirection,
    handleScroll,
    isScrolling,
    offset,
  };
}

function Viewport({
  className,
  children,
  ...props
}: ComponentProps<typeof ScrollAreaPrimitive.Viewport>) {
  return (
    <ScrollAreaPrimitive.Viewport
      data-slot="scroll-area-viewport"
      className={cn(
        'flex min-h-0 w-full flex-1 flex-col',
        'rounded-[inherit] transition-[color,box-shadow] outline-none',
        '[&>div]:block! [&>div]:min-h-0! [&>div]:flex-1!',
        className,
      )}
      {...props}
    >
      {children}
    </ScrollAreaPrimitive.Viewport>
  );
}

const Corner = ScrollAreaPrimitive.Corner;
const Root = ScrollAreaPrimitive.Root;

function ScrollBar({
  className,
  orientation = 'vertical',
  ...props
}: ComponentProps<typeof ScrollAreaPrimitive.Scrollbar>) {
  return (
    <ScrollAreaPrimitive.ScrollAreaScrollbar
      data-slot="scroll-area-scrollbar"
      orientation={orientation}
      className={cn(
        'z-50 flex touch-none p-px select-none',
        'transition-opacity duration-300 ease-out',
        'data-[state=hidden]:opacity-0 data-[state=visible]:opacity-100',
        orientation === 'vertical' &&
          'h-full w-3.5 border-l border-l-transparent px-0.75 py-1.5',
        orientation === 'horizontal' &&
          'h-3.5 flex-col border-t border-t-transparent px-1.5 py-0.75',
        className,
      )}
      {...props}
    >
      <ScrollAreaPrimitive.ScrollAreaThumb
        data-slot="scroll-area-thumb"
        className="bg-surface-variant relative flex-1 rounded-full"
      />
    </ScrollAreaPrimitive.ScrollAreaScrollbar>
  );
}

function ScrollAreaProvider({
  className,
  children,
  scrollbars,
  slot,
  ...props
}: ComponentProps<typeof ScrollAreaPrimitive.Root> & {
  scrollbars: 'vertical' | 'horizontal' | 'both';
  slot: string;
}) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const tracking = useScrollTracking();

  return (
    <ScrollAreaContext.Provider value={{ ...tracking, viewportRef }}>
      <Root
        data-slot={slot}
        type="scroll"
        scrollHideDelay={600}
        className={className}
        data-scrolling={String(tracking.isScrolling)}
        data-top={String(tracking.isTop)}
        data-bottom={String(tracking.isBottom)}
        data-scroll-direction={tracking.scrollDirection}
        {...props}
      >
        <Viewport ref={viewportRef} onScroll={tracking.handleScroll}>
          {children}
        </Viewport>
        {(scrollbars === 'vertical' || scrollbars === 'both') && (
          <ScrollBar orientation="vertical" />
        )}
        {(scrollbars === 'horizontal' || scrollbars === 'both') && (
          <ScrollBar orientation="horizontal" />
        )}
        <Corner />
      </Root>
    </ScrollAreaContext.Provider>
  );
}

export function ScrollArea({
  className,
  scrollbars = 'vertical',
  ...props
}: ComponentProps<typeof ScrollAreaPrimitive.Root> & {
  scrollbars?: 'vertical' | 'horizontal' | 'both';
}) {
  return (
    <ScrollAreaProvider
      className={cn('relative flex min-h-0 flex-col', className)}
      scrollbars={scrollbars}
      slot="scroll-area"
      {...props}
    />
  );
}

export function AppContentScrollArea({
  className,
  scrollbars = 'vertical',
  ...props
}: ComponentProps<typeof ScrollAreaPrimitive.Root> & {
  scrollbars?: 'vertical' | 'horizontal' | 'both';
}) {
  return (
    <ScrollAreaProvider
      className={cn(
        'relative flex min-h-0 flex-1 flex-col',
        'max-w-screen min-w-0',
        className,
      )}
      scrollbars={scrollbars}
      slot="app-content-scroll-area"
      {...props}
    />
  );
}
