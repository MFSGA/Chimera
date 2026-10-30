if (typeof window === 'undefined') {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (globalThis as any).window = {} as any;
}

export * from './hooks';
export * from './utils';
export * from './materialYou';
export * from './dnd-grid';
export * from './search-field';

export * from './animated-item';
export * from './button';
export * from './card';
export * from './circle';
export * from './linear-progress';
export * from './progress';
export * from './ripple';
export * from './scroll-area';
export * from './separator';
export { default as HighlightText } from './highlight-text';
export * from './input';
export * from './slider';
// Keep the legacy Material You SwitchItem at the root export.
// The reference primitive SwitchItem is available from '@chimera/ui/switch'.
export { Switch } from './switch';
export * from './modal';
export * from './tooltip';
export * from './animated-tabs';
export { default as AnimatedTabs } from './animated-tabs';
export { default as TextMarquee } from './text-marquee';
export * from './context-menu';
export * from './dropdown-menu';
export * from './segmented-button';
export * from './navigation-sidebar';
// The Material You Sparkline remains the root export for legacy dashboards.
// Ref-style Sparkline is available at '@chimera/ui/sparkline'.
export {
  Sidebar as SliderSidebar,
  SidebarLabelItem,
  SidebarProvider,
  SidebarToggleButton,
  useSidebar,
} from './slider-sidebar';
