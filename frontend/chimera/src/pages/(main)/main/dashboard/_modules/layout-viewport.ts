import type { GridSize } from '@chimera/ui/dnd-grid';

/** Prevent viewport changes from altering the source of an active drag/resize.
 * Updates are replayed once both the gesture and a pending save have settled.
 * If the available grid changed mid-gesture, the old-grid drop is discarded.
 */
export function createDashboardViewportCoordinator(
  onApply: (size: GridSize) => void,
) {
  let applied: GridSize | null = null;
  let pending: GridSize | null = null;
  let interacting = false;
  let saving = false;
  let resizedDuringInteraction = false;
  let needsReselect = false;

  const same = (a: GridSize | null, b: GridSize) =>
    a?.cols === b.cols && a.rows === b.rows;

  const flush = () => {
    if (interacting || saving || !pending) return;
    const latest = pending;
    pending = null;
    const reselect = needsReselect;
    needsReselect = false;
    if (!same(applied, latest) || reselect) {
      applied = latest;
      onApply(latest);
    }
  };

  return {
    onSizeChange: (size: GridSize) => {
      if (interacting && !same(applied, size)) resizedDuringInteraction = true;
      if (interacting || saving) {
        pending = size;
        return;
      }
      pending = size;
      flush();
    },
    onInteractionChange: (active: boolean) => {
      if (active) {
        interacting = true;
        resizedDuringInteraction = false;
        needsReselect = true;
        // Re-select after the interaction even if a pending settle timer was
        // cancelled but no ResizeObserver event occurred.
        pending ??= applied;
      } else {
        // The drop/cancel handler has already consulted this flag before
        // finishing the gesture. Do not let a rejected old-grid drop poison
        // the next direct add/remove action outside a drag lifecycle.
        interacting = false;
        resizedDuringInteraction = false;
        flush();
      }
    },
    onSavingChange: (active: boolean) => {
      if (active && !saving) {
        // A direct library click can start saving while a resize settle
        // timer is pending, without a grid drag interaction.
        pending ??= applied;
        needsReselect = true;
      }
      saving = active;
      flush();
    },
    shouldDiscardCommit: () => resizedDuringInteraction,
    isInteracting: () => interacting,
    hasPendingSize: () => pending !== null,
  };
}
