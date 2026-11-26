import { test } from 'node:test';
import assert from 'node:assert/strict';
import { getConfig, keepAlive, MAX_OPEN_BYTES, openFile, Refused } from '../app/local.ts';
import { createSession } from '../app/answers.ts';
import { editsSaver } from '../app/save.ts';
import { SESSION_API, SessionApiMismatch } from '../app/session-api.ts';

const config = { name: 'part.step', saveTarget: '/tmp/part.specified.step' };
const flush = async () => { for (let n = 0; n < 20; n++) await Promise.resolve(); };

// Session API 2 (specify-core 0.3.0): Save sends an edit list. specify-core 0.2.0 gives no api and
// speaks contract 1 (answers PUT and saved), which this page can no longer follow.
test('config without api (specify-core 0.2.0, contract 1) refuses startup', async (t) => {
  t.mock.method(globalThis, 'fetch', async () => Response.json(config));
  await assert.rejects(getConfig(), (error) => error instanceof SessionApiMismatch
    && error.message === 'This Specify page speaks session API 2, but specify-core speaks 1: update specify-core or the app');
});

test('config with the supported session API is accepted', async (t) => {
  t.mock.method(globalThis, 'fetch', async () => Response.json({ api: SESSION_API, ...config }));
  assert.deepEqual(await getConfig(), { api: SESSION_API, ...config });
});

test('config with a mismatched API refuses startup and names both versions', async (t) => {
  t.mock.method(globalThis, 'fetch', async () => Response.json({ api: 3, ...config }));
  await assert.rejects(getConfig(), (error) => error instanceof SessionApiMismatch
    && error.message === 'This Specify page speaks session API 2, but specify-core speaks 3: update specify-core or the app');
});

/** Browser timers run against wall time. Sleeping moves time without executing callbacks. */
function browser(t) {
  let now = 0;
  let id = 0;
  const timers = new Map();
  const cleanup = [];
  const window = new EventTarget();
  const document = Object.assign(new EventTarget(), { visibilityState: 'visible' });
  const schedule = (callback, delay, repeat = false) => {
    timers.set(++id, { callback, at: now + delay, delay, repeat });
    return id;
  };
  window.setTimeout = (callback, delay) => schedule(callback, delay);
  window.setInterval = (callback, delay) => schedule(callback, delay, true);
  window.clearTimeout = window.clearInterval = (id) => timers.delete(id);
  const original = new Map(['window', 'document', 'navigator'].map((key) => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  Object.defineProperties(globalThis, {
    window: { configurable: true, value: window },
    document: { configurable: true, value: document },
    navigator: { configurable: true, value: { sendBeacon: () => true } },
  });
  t.mock.method(Date, 'now', () => now);
  t.after(() => {
    for (const close of cleanup) close();
    for (const [key, descriptor] of original) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  });
  return {
    window, document,
    cleanup(close) { cleanup.push(close); },
    sleep(ms) { now += ms; },
    async advance(ms) {
      await flush();
      const end = now + ms;
      for (;;) {
        const due = [...timers].filter(([, timer]) => timer.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        const [id, timer] = due;
        now = Math.max(now, timer.at);
        if (timer.repeat) timer.at = now + timer.delay;
        else timers.delete(id);
        timer.callback();
        await flush();
      }
      now = end;
      await flush();
    },
  };
}

function command(t) {
  const calls = [];
  const state = { mode: 'ok' };
  t.mock.method(globalThis, 'fetch', (_url, { signal }) => {
    const call = { signal, started: Date.now() };
    calls.push(call);
    // The command's config names its session API (2): one without it is specify-core 0.2.0's, refused.
    if (state.mode === 'ok') return Promise.resolve(Response.json({ api: SESSION_API, ...config }));
    if (state.mode === 'busy') return Promise.resolve(Response.json({ error: 'busy' }, { status: 503 }));
    if (state.mode === 'offline') return Promise.reject(new TypeError('connection refused'));
    return new Promise((resolve, reject) => {
      call.answer = () => resolve(Response.json({ api: SESSION_API, ...config }));
      signal.addEventListener('abort', () => reject(signal.reason), { once: true });
    });
  });
  return { state, calls };
}

test('a stopped command continues probing and a later success restores Save eligibility', async (t) => {
  const clock = browser(t);
  const { state } = command(t);
  const statuses = [];
  const configs = [];
  const heartbeat = keepAlive((c) => configs.push(c), (stopped) => statuses.push(stopped), () => {});
  clock.cleanup(heartbeat.stop);
  heartbeat.refresh();
  await flush();
  state.mode = 'offline';
  await clock.advance(1_000);
  heartbeat.refresh();
  await clock.advance(23_000);
  assert.deepEqual(statuses, [], 'the full failure window has not elapsed');
  await clock.advance(1_000);
  // Changed timing: the idle time before the first failed probe cannot consume its grace period.
  assert.deepEqual(statuses, [], '24 seconds since the first failed probe');
  await clock.advance(1_000);
  assert.deepEqual(statuses, [true]);
  state.mode = 'ok';
  await clock.advance(4_000);
  assert.deepEqual(statuses, [true, false], 'recovery clears the stopped state');
  assert.equal(configs.length, 2);
});

test('one failed 30-second heartbeat cannot disable Save before a full 25-second failure window', async (t) => {
  const clock = browser(t);
  const { state } = command(t);
  const statuses = [];
  const heartbeat = keepAlive(() => {}, (stopped) => statuses.push(stopped), () => {});
  clock.cleanup(heartbeat.stop);
  heartbeat.refresh();
  await flush();
  state.mode = 'offline';
  await clock.advance(30_000);
  assert.deepEqual(statuses, [], 'the first failed heartbeat only starts the grace period');
  state.mode = 'ok';
  await clock.advance(4_000);
  assert.deepEqual(statuses, [], 'one transient failure never disabled Save');
  state.mode = 'offline';
  await clock.advance(30_000);
  await clock.advance(24_000);
  assert.deepEqual(statuses, []);
  await clock.advance(1_000);
  assert.deepEqual(statuses, [true], 'sustained failure consumes a complete 25-second window');
});

test('sustained HTTP 503 responses keep the command alive', async (t) => {
  const clock = browser(t);
  const { state, calls } = command(t);
  const statuses = [];
  state.mode = 'busy';
  const heartbeat = keepAlive(() => {}, (stopped) => statuses.push(stopped), () => {});
  clock.cleanup(heartbeat.stop);
  heartbeat.refresh();
  await clock.advance(180_000);
  assert.ok(calls.length > 30, 'busy commands are retried');
  assert.deepEqual(statuses, []);
});

test('a fresh probe after sleep survives overdue timers and a healthy response', async (t) => {
  const clock = browser(t);
  const { state, calls } = command(t);
  const statuses = [];
  const heartbeat = keepAlive(() => {}, (stopped) => statuses.push(stopped), () => {});
  clock.cleanup(heartbeat.stop);
  state.mode = 'offline';
  heartbeat.refresh();
  await clock.advance(24_000);
  clock.sleep(600_000);
  state.mode = 'pending';
  // Start a healthy wake probe before the overdue interval callback gets its turn.
  heartbeat.refresh();
  const fresh = calls.at(-1);
  await clock.advance(4_000);
  assert.equal(fresh.signal.aborted, false);
  assert.deepEqual(statuses, []);
  fresh.answer();
  await flush();
  state.mode = 'offline';
  heartbeat.refresh();
  await clock.advance(24_000);
  assert.deepEqual(statuses, [], 'the healthy response restarted the observation window');
});

test('visibility resume resets the failure window and gives an in-flight probe time to answer', async (t) => {
  const clock = browser(t);
  const { state, calls } = command(t);
  const statuses = [];
  const heartbeat = keepAlive(() => {}, (stopped) => statuses.push(stopped), () => {});
  clock.cleanup(heartbeat.stop);
  state.mode = 'offline';
  heartbeat.refresh();
  await clock.advance(20_000);
  state.mode = 'pending';
  heartbeat.refresh();
  const fresh = calls.at(-1);
  await clock.advance(4_000);
  clock.document.dispatchEvent(new Event('visibilitychange'));
  await clock.advance(3_000);
  assert.equal(fresh.signal.aborted, false, 'the pre-resume response timeout no longer applies');
  fresh.answer();
  await flush();
  assert.deepEqual(statuses, []);
});

test('a probe hung before sleep is timed out only after a fresh response window on wake', async (t) => {
  const clock = browser(t);
  const { state, calls } = command(t);
  const heartbeat = keepAlive(() => {}, () => {}, () => {});
  clock.cleanup(heartbeat.stop);
  state.mode = 'pending';
  heartbeat.refresh();
  const old = calls.at(-1);
  clock.sleep(60_000);
  await clock.advance(0);
  assert.equal(old.signal.aborted, false);
  await clock.advance(5_000);
  assert.equal(old.signal.aborted, true);
  state.mode = 'ok';
  await clock.advance(4_000);
  assert.equal(calls.length, 2, 'a hung request does not prevent later recovery');
});

test('pagehide cancels an in-flight Save before sending closed', async (t) => {
  const clock = browser(t);
  const events = [];
  let signal;
  t.mock.method(globalThis, 'fetch', (_url, init) => new Promise((_resolve, reject) => {
    assert.equal(init.method, 'POST');
    signal = init.signal;
    events.push('save');
    signal.addEventListener('abort', () => {
      events.push('abort save');
      reject(signal.reason);
    }, { once: true });
  }));
  const session = createSession(0, { 0: {} });
  const store = session.store(0);
  const heartbeat = keepAlive(() => {}, () => {}, () => session.close());
  clock.cleanup(heartbeat.stop);
  const saving = editsSaver(store, session, () => ({})).save({});
  const cancelled = assert.rejects(saving, { name: 'AbortError' });
  await flush();
  t.mock.method(navigator, 'sendBeacon', () => {
    assert.equal(signal.aborted, true);
    events.push('closed');
    return true;
  });
  clock.window.dispatchEvent(Object.assign(new Event('pagehide'), { persisted: true }));
  assert.equal(signal.aborted, false, 'bfcache keeps an active Save');
  clock.window.dispatchEvent(Object.assign(new Event('pagehide'), { persisted: false }));
  await cancelled;
  assert.deepEqual(events, ['save', 'abort save', 'closed']);
});

test('openFile refuses empty and oversized files before uploading and propagates server 413', async (t) => {
  let fetched = 0;
  t.mock.method(globalThis, 'fetch', async () => {
    fetched += 1;
    return Response.json({ error: `uncompressed STEP is larger than ${MAX_OPEN_BYTES} bytes` }, { status: 413 });
  });
  await assert.rejects(openFile(new File([], 'empty.step')), /the STEP file is empty/);
  const big = { name: 'big.step', size: MAX_OPEN_BYTES + 1, arrayBuffer: async () => new ArrayBuffer(0) };
  await assert.rejects(openFile(big), new RegExp(`larger than ${MAX_OPEN_BYTES} bytes`));
  assert.equal(fetched, 0, 'empty and over-limit files do not upload');
  await assert.rejects(
    openFile(new File([new Uint8Array([0x1f, 0x8b])], 'bomb.step.gz')),
    (error) => error instanceof Refused && error.status === 413 && error.message.includes(String(MAX_OPEN_BYTES)),
  );
  assert.equal(fetched, 1);
});
