import { createFileRoute, useNavigate } from '@tanstack/react-router';
import { useAtomValue } from 'jotai';
import { useEffect } from 'react';
import { memorizedRoutePathAtom } from '@/store';

export const Route = createFileRoute('/')({
  component: IndexPage,
});

function IndexPage() {
  const navigate = useNavigate();
  const memorizedNavigate = useAtomValue(memorizedRoutePathAtom);
  useEffect(() => {
    navigate({
      to:
        memorizedNavigate && memorizedNavigate !== '/'
          ? memorizedNavigate
          : '/dashboard',
    });
  }, [memorizedNavigate, navigate]);
  return null;
}
