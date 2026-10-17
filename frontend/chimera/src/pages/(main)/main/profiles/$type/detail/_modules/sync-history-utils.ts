export function hasDegradedEffects(jsonText: string): boolean {
  try {
    const value: unknown = JSON.parse(jsonText);
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      return false;
    }
    const degradationCount = (value as { degradation_count?: unknown })
      .degradation_count;
    return typeof degradationCount === 'number' && degradationCount > 0;
  } catch {
    // A malformed retained run summary must not break the history page.
    return false;
  }
}
