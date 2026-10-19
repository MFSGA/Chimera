/** Tracks write events so a stale backend echo cannot replace optimistic state. */
export class KvStorageWriteEchoes {
  private readonly pending = new Map<string, Map<string, number>>();
  private activeKey: string;

  constructor(activeKey: string) {
    this.activeKey = activeKey;
  }

  setActiveKey(key: string): void {
    this.activeKey = key;
  }

  isActiveKey(key: string): boolean {
    return this.activeKey === key;
  }

  add(key: string, value: string): void {
    const values = this.pending.get(key) ?? new Map<string, number>();
    values.set(value, (values.get(value) ?? 0) + 1);
    this.pending.set(key, values);
  }

  consume(key: string, value: string): boolean {
    const values = this.pending.get(key);
    const count = values?.get(value);
    if (count === undefined) return false;

    if (count === 1) values?.delete(value);
    else values?.set(value, count - 1);
    if (values?.size === 0) this.pending.delete(key);
    return true;
  }

  remove(key: string, value: string): void {
    this.consume(key, value);
  }
}
