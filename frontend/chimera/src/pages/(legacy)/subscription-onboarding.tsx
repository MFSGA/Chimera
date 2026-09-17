import { createFileRoute } from '@tanstack/react-router';
import BestEffortSubscriptionImport from '@/components/profiles/best-effort-subscription-import';

export const Route = createFileRoute('/(legacy)/subscription-onboarding')({
  component: SubscriptionOnboardingPage,
});

function SubscriptionOnboardingPage() {
  return <BestEffortSubscriptionImport />;
}
