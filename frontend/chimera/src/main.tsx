// import React from "react";

// import App from "./App";

import './assets/styles/tailwind.css';
import { createRouter, RouterProvider } from '@tanstack/react-router';
import React from 'react';
import ReactDOM, { createRoot } from 'react-dom/client';
import { routeTree } from './routeTree.gen';
import './services/i18n';

// import React from "react";

const container = document.getElementById('root')!;

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
