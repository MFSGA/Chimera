import assert from 'node:assert/strict';

async function waitForApp() {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

describe('Chimera main UI deep-link import', () => {
  it('opens the remote profile dialog with deep-link metadata prefilled', async () => {
    await waitForApp();

    const currentHref = await browser.getUrl();
    const target = new URL('/main/profiles/profile', currentHref);
    target.searchParams.set(
      'subscribeUrl',
      'https://example.com/subscription.yaml',
    );
    target.searchParams.set('subscribeName', 'Deep Link Profile');
    target.searchParams.set(
      'subscribeDesc',
      'Imported from a Chimera deep link',
    );

    await browser.url(target.href);
    await waitForApp();

    const name = await $('[name="name"]');
    const url = await $('[name="url"]');
    const desc = await $('[name="desc"]');

    await name.waitForExist({ timeout: 15_000 });
    await url.waitForExist({ timeout: 15_000 });
    await desc.waitForExist({ timeout: 15_000 });

    assert.equal(await name.getValue(), 'Deep Link Profile');
    assert.equal(await url.getValue(), 'https://example.com/subscription.yaml');
    assert.equal(await desc.getValue(), 'Imported from a Chimera deep link');
  });
});
