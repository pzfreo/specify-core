/** Opt in with SPECIFY_TEST_CPU_THROTTLE=6 (or another rate) to reproduce timing issues in Chromium. */
export async function throttleAppPage(page) {
  const configured = process.env.SPECIFY_TEST_CPU_THROTTLE;
  if (configured === undefined) return;
  const rate = Number(configured);
  if (!Number.isFinite(rate) || rate < 1) throw new Error('SPECIFY_TEST_CPU_THROTTLE must be a number at least 1');
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Emulation.setCPUThrottlingRate', { rate });
}

/** Keep the most recent page requests, including unfinished ones, for Save timeouts. */
const saveTraffic = new WeakMap();
export function watchSaveTraffic(page) {
  const recent = new Map();
  page.on('request', request => {
    const url = new URL(request.url());
    const path = url.pathname.replace(/^\/[0-9a-f]{32}\//, '/');
    recent.set(request, { method: request.method(), path: path + url.search, status: 'pending' });
    if (recent.size > 20) recent.delete(recent.keys().next().value);
  });
  page.on('response', response => {
    const record = recent.get(response.request());
    if (record) record.status = response.status();
  });
  page.on('requestfailed', request => {
    const record = recent.get(request);
    if (record) record.status = request.failure()?.errorText ?? 'failed';
  });
  saveTraffic.set(page, async (error, stderr) => {
    if (error?.name !== 'TimeoutError' && !/\btimeout\b/i.test(error?.message ?? '')) return;
    const statePromise = page.evaluate(() => {
      const bar = document.querySelector('.spec-bar');
      return {
        bar: bar?.innerText,
        busy: bar?.getAttribute('aria-busy'),
        saving: bar?.querySelector('.app-save')?.textContent === 'Saving…',
        buttons: Array.from(bar?.querySelectorAll('button') ?? [], button => ({
          text: button.textContent, disabled: button.disabled, title: button.title,
        })),
        reviewDisabled: document.querySelector('fieldset.pmi-lab-rows')?.disabled,
        pickerDisabled: document.querySelector('.app-part select')?.disabled,
        alerts: Array.from(document.querySelectorAll('[role="alert"]'), node => node.innerText),
        engineOps: window.__engineOps?.slice(-20),
      };
    }).catch(error => ({ unavailable: error.message }));
    let timer;
    const deadline = new Promise(resolve => {
      timer = setTimeout(() => resolve({ unavailable: 'The page did not answer the diagnostics query.' }), 2000);
    });
    const state = await Promise.race([statePromise, deadline]).finally(() => clearTimeout(timer));
    console.error('Save timeout', JSON.stringify({ state, requests: [...recent.values()] }, null, 2));
    console.error('Recent server stderr', stderr().split('\n').slice(-20).join('\n').slice(-4000) || '(empty)');
  });
}

export async function pageSaveResponse(page, url, stderr, afterClick, options) {
  const response = page.waitForResponse(r => r.url() === `${url}save` && r.request().method() === 'POST', options);
  // A disabled Save can outlast the response waiter. Keep that rejection available to await.
  void response.catch(() => {});
  try {
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await afterClick?.();
    return await response;
  } catch (error) {
    await saveTraffic.get(page)?.(error, stderr);
    throw error;
  }
}
