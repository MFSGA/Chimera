import { useQuery } from '@tanstack/react-query';
import { queries } from './query-bindings';

/**
 * The selected core's authoritative config capabilities. Uses the same
 * backend matrix as the runtime builder, rather than duplicating core names
 * in the frontend. Invalidated when the committed app config changes.
 */
export const useConfigFeatures = () => useQuery(queries.getConfigFeatures());
