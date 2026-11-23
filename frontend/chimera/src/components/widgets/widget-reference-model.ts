import type { ProviderReference } from './widget-config';

/** A reference can only be declared missing after its provider-kind list was
 * successfully fetched. Pending and failed queries are not evidence of
 * deletion, and must not erase persisted references. */
export type ProviderReferenceChoice = {
  ref: ProviderReference;
  availability: 'available' | 'missing' | 'unknown';
};

export function providerReferenceChoices(
  selected: readonly ProviderReference[],
  proxyNames: readonly string[] | null,
  ruleNames: readonly string[] | null,
): ProviderReferenceChoice[] {
  const available: ProviderReference[] = [
    ...(proxyNames ?? []).map((name) => ({ kind: 'proxy' as const, name })),
    ...(ruleNames ?? []).map((name) => ({ kind: 'rule' as const, name })),
  ];
  const refs = [
    ...selected,
    ...available.filter(
      (reference) =>
        !selected.some(
          ({ kind, name }) =>
            kind === reference.kind && name === reference.name,
        ),
    ),
  ];

  return refs.map((ref) => {
    const names = ref.kind === 'proxy' ? proxyNames : ruleNames;
    return {
      ref,
      availability:
        names === null
          ? 'unknown'
          : names.includes(ref.name)
            ? 'available'
            : 'missing',
    };
  });
}

/** The selected value remains editable even if a provider was deleted or its
 * query failed; avoid adding unseen sources based on an unverified snapshot. */
export function canToggleProviderReference(
  choice: ProviderReferenceChoice,
  selected: boolean,
): boolean {
  return selected || choice.availability === 'available';
}
