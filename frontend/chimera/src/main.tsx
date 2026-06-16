import './assets/styles/tailwind.css';
import { createRouter, RouterProvider } from '@tanstack/react-router';
import React from 'react';
import ReactDOM, { createRoot } from 'react-dom/client';
import { routeTree } from './routeTree.gen';
import { setupFrontendConsoleBridge } from './services/frontend-console-bridge';
// manually import language utils, inject paraglide custom strategy
import '@/utils/language-new';

const container = document.getElementById('root')!;

void setupFrontendConsoleBridge();

// Set up a Router instance
const router = createRouter({
  routeTree,
  defaultPreload: 'intent',
});

// Register things for typesafety
declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}

createRoot(container).render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>,
);
