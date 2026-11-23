/** Opt in with SPECIFY_TEST_CPU_THROTTLE=6 (or another rate) to reproduce timing issues in Chromium. */
export async function throttleAppPage(page) {
  const configured = process.env.SPECIFY_TEST_CPU_THROTTLE;
  if (configured === undefined) return;
  const rate = Number(configured);
  if (!Number.isFinite(rate) || rate < 1) throw new Error('SPECIFY_TEST_CPU_THROTTLE must be a number at least 1');
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Emulation.setCPUThrottlingRate', { rate });
}
