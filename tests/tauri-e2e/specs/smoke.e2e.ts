describe('Chimera desktop smoke test', () => {
  it('boots the Tauri window and renders the React root', async () => {
    await browser.waitUntil(
      async () =>
        (await browser.execute(() => document.readyState)) === 'complete',
      {
        timeout: 30_000,
        timeoutMsg: 'The Chimera document did not finish loading.',
      },
    );

    const root = await $('#root');
    await root.waitForExist({ timeout: 30_000 });
    await browser.waitUntil(
      async () =>
        (await browser.execute(
          () => document.getElementById('root')?.childElementCount ?? 0,
        )) > 0,
      {
        timeout: 30_000,
        timeoutMsg: 'React did not render content into #root.',
      },
    );

    await expect(root).toExist();
  });
});
