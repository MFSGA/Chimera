import type { DashboardItem } from '../../../../../components/widgets/consts';
import { saveLayout, type DashboardLayoutStorage } from './layout-storage';

/** A user edit, not a stale snapshot of the entire dashboard layout. */
export type DashboardLayoutEdit = {
  sourceId: string;
  visibleIds: string[];
  nextItems: DashboardItem[];
};

/** Rebase an edit on the latest KV value, retaining widgets hidden by a
 * narrow viewport and other entries updated since the drag began. */
export function applyDashboardLayoutEdit(
  storage: DashboardLayoutStorage,
  edit: DashboardLayoutEdit,
): DashboardLayoutStorage {
  const source = storage.entries.find(({ id }) => id === edit.sourceId);
  if (!source) {
    throw new Error(`Dashboard layout entry ${edit.sourceId} was removed`);
  }

  const visibleIds = new Set(edit.visibleIds);
  const nextIds = new Set(edit.nextItems.map(({ id }) => id));
  // A failed KV draft may already show a newly added widget. Replaying that
  // edit must not append a second copy as an allegedly hidden source widget.
  const hidden = source.items.filter(
    ({ id }) => !visibleIds.has(id) && !nextIds.has(id),
  );
  return saveLayout(storage, source.id, [...edit.nextItems, ...hidden]);
}

export type DashboardLayoutSaveState = 'saved' | 'saving' | 'error';

/** Serialized layout saves. Failures retain only the user's edit intent, not
 * an obsolete whole-layout snapshot. The KV adapter handles its own revision
 * protection; every attempt reads the latest value through a functional write. */
export function createDashboardLayoutSaveQueue({
  write,
  refresh,
  onStateChange,
}: {
  write: (edit: DashboardLayoutEdit) => Promise<boolean>;
  refresh: () => Promise<unknown>;
  onStateChange: (state: DashboardLayoutSaveState) => void;
}) {
  let state: DashboardLayoutSaveState = 'saved';
  let failedIntent: DashboardLayoutEdit | null = null;

  const update = (next: DashboardLayoutSaveState) => {
    state = next;
    onStateChange(next);
  };

  const persist = async (edit: DashboardLayoutEdit): Promise<void> => {
    update('saving');
    let confirmed = false;
    try {
      confirmed = await write(edit);
    } catch {
      confirmed = false;
    }
    if (!confirmed) {
      // A revision conflict may have invalidated the KV draft. Re-read its
      // latest committed value before permitting a retry.
      try {
        await refresh();
      } catch {
        // The read error remains visible through the KV hook.
      }
    }
    if (confirmed) failedIntent = null;
    update(confirmed ? 'saved' : 'error');
  };

  return {
    getState: () => state,
    save: (edit: DashboardLayoutEdit): boolean => {
      if (state !== 'saved') return false;
      failedIntent = edit;
      void persist(edit);
      return true;
    },
    retry: (): boolean => {
      if (state !== 'error' || !failedIntent) return false;
      void persist(failedIntent);
      return true;
    },
  };
}
