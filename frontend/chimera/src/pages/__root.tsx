import { createRootRoute } from "@tanstack/react-router";
import React from "react";

export const Route = createRootRoute({
  component: App,
  // errorComponent: Catch,
  // pendingComponent: Pending,
});

export default function App() {
  const [count, setCount] = React.useState(0);
  return <div>App is {count}</div>;
}
