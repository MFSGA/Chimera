import { z } from 'zod';

/** Ref-aligned subscription URL: only HTTP and HTTPS are remote sources. */
export const subscriptionUrlSchema = z.url({ protocol: /^https?$/ });
