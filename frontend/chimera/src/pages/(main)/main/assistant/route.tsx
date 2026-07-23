import { createFileRoute } from '@tanstack/react-router';
import { AgentPage } from '@/features/agent';

export const Route = createFileRoute('/(main)/main/assistant')({
  component: AgentPage,
});
