import { createRootRoute } from "@tanstack/react-router";
import { check } from "@tauri-apps/plugin-updater";

export const Route = createRootRoute({
  component: App,
  // errorComponent: Catch,
  // pendingComponent: Pending,
});

async function getVersion() {
  const update = await check();
  console.log(update);
  if (update) {
    console.log("Update available:");

    // await update.downloadAndInstall();
    // await relaunch();
  }
}

export default function App() {
  return (
    <div>
      <h1>Hello World</h1>
      <div onClick={getVersion}>get version</div>
    </div>
  );
}
