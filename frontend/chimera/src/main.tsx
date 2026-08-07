import './assets/styles/fonts.css';
import './assets/styles/tailwind.css';
import './assets/styles/main.css';
import { createRouter, RouterProvider } from '@tanstack/react-router';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import React from 'react';
import ReactDOM, { createRoot } from 'react-dom/client';
import { routeTree } from './routeTree.gen';
import { setupFrontendConsoleBridge } from './services/frontend-console-bridge';
// manually import language utils, inject paraglide custom strategy
import '@/utils/language-new';

const currentWindow = getCurrentWebviewWindow();

if (currentWindow.label === 'main') {
  const root = document.documentElement;
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;

  root.classList.add('chimera-main');
  root.classList.toggle('dark', prefersDark);
  root.classList.toggle('light', !prefersDark);
}

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
