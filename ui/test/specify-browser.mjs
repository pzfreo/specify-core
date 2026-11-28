import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { chromium, expect } from '@playwright/test';
// <SpecifyReview> in the harness page (test/harness, ?specify=1) on the spindle: editable, then readOnly.
// The harness's engine records each call the component makes of it on window.__calls. Run by
// `npm run test:browser` (it builds the harness); `npm run fetch:wasm` first.
const check = expect.configure({ timeout: 90000 });
const base = process.env.BASE_URL ?? 'http://127.0.0.1:4291';
const server = process.env.BASE_URL ? null : spawn(process.execPath, ['node_modules/vite/bin/vite.js', 'preview', '--config', 'vite.harness.config.ts', '--host', '127.0.0.1', '--port', '4291', '--strictPort'], { stdio: 'inherit' });
for (let n = 0; n < 100; n++) { try { if ((await fetch(base)).ok) break; } catch {} await new Promise(resolve => setTimeout(resolve, 200)); }
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined, args: ['--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader'] });
const problems = [];
async function openPage(readOnly, file = 'test/fixtures/spindle_pmi_before.step.gz', ready = '#specify-panel .pmi-lab-row-head', query = '') {
  const page = await browser.newPage({ viewport: { width: 1300, height: 1100 } });
  page.on('pageerror', error => problems.push(String(error)));
  await page.goto(`${base}/?specify=1${readOnly ? '&readOnly=1' : ''}${query}`);
  await page.locator('#specify-file').setInputFiles(file);
  await check(page.locator(ready).first()).toBeVisible();
  // Display always follows origin/main's preview contract, even if inspection is available.
  await check.poll(() => page.evaluate(() => window.__calls.includes('preview'))).toBe(true);
  assert.ok(!(await page.evaluate(() => window.__calls)).includes('inspection'), 'opening a review never requests inspection');
  return page;
}
const calls = page => page.evaluate(() => [...window.__calls]);
const questionAnswers = page => page.evaluate(() => window.__questionAnswers);
// The first hole row, opened (it may already be: the first thing needing a person opens at the start).
const openHoleRow = async page => {
  const head = page.locator('#specify-panel .pmi-lab-row-head', { hasText: /hole/i }).first();
  if (await head.getAttribute('aria-expanded') !== 'true') await head.click();
  await check(head).toHaveAttribute('aria-expanded', 'true');
};
const panel = page => page.locator('#specify-panel');
// Diagnose disabled/covered host controls before starting a download-event timeout. A trial
// click preserves Playwright's hit testing and never runs an immediate or deferred export.
async function downloadFrom(page, button) {
  await check(button).toBeEnabled();
  await button.click({ trial: true });
  const [file] = await Promise.all([page.waitForEvent('download'), button.click()]);
  return file.path();
}
const inspectionDownload = (page, name) => downloadFrom(page,
  page.locator('#specify-exports').getByRole('button', { name, exact: true }));
try {
  // A host implementing only the original three methods keeps origin/main's shaft preview,
  // including all 15 sequential balloons and its complete Form 3 (Material is balloon 12).
  {
    const page = await openPage(false, 'test/fixtures/gramil_shaft_before.step.gz', '#specify-panel .pmi-lab-row-head', '&legacy=1');
    const input = panel(page).locator('[data-question="part.material"] input');
    await input.fill('Steel');
    await input.press('Enter');
    await panel(page).getByRole('tab', { name: /^Characteristics/ }).click();
    const material = panel(page).locator('.pmi-lab-table tr').filter({ hasText: 'Material: Steel' });
    await check(material.locator('td').nth(1)).toHaveText('12');
    const download = (name) => inspectionDownload(page, name);
    const csv = readFileSync(await download('Inspection list (CSV)'), 'utf8');
    const expected = [
      'M2×0.4-6H', '⌖ ⌀0.2', 'M4×0.7-6H', '⌖ ⌀0.25',
      '36.1 from datum B', '10.4 from datum B', '4 from datum B', '3 from bottom face',
      '4.9 from bottom face', '4.75 from back face', '37.6 from 2 left faces',
      'Material: Steel', 'General tolerance: ISO 2768-m', 'Surface finish: Ra 3.2', 'Edges: Break sharp edges 0.2 max',
    ];
    assert.deepEqual(csv.trimEnd().split('\n').slice(1).map((line) => [line.split(',')[0], line.split(',')[3]]), expected.map((text, i) => [String(i + 1), text]));
    const form3 = execFileSync('unzip', ['-p', await download('AS9102 FAI (Excel)'), 'xl/worksheets/sheet3.xml'], { encoding: 'utf8' });
    for (const [i, text] of expected.entries()) {
      assert.match(form3, new RegExp(`<c r="A${i + 5}"[^>]*><is><t [^>]*>${i + 1}</t>`));
      assert.ok(form3.includes(text), `Form 3 retains ${text}`);
    }
    assert.ok(!(await calls(page)).includes('inspection'), 'the host exposes only three methods');
    await page.close();
  }

  // A legacy export gated on sign-in must read rows, skips and notes at execution time,
  // just as origin/main does. Queue both formats, then edit and skip before resuming.
  {
    const page = await openPage(false, 'test/fixtures/gramil_shaft_before.step.gz', '#specify-panel .pmi-lab-row-head', '&legacy=1&defer=1');
    const input = panel(page).locator('[data-question="part.material"] input');
    await input.fill('Steel');
    await input.press('Enter');
    await panel(page).getByRole('tab', { name: /^Characteristics/ }).click();
    await check(panel(page).locator('.pmi-lab-table')).toContainText('Material: Steel');
    const exports = page.locator('#specify-exports');
    await exports.getByRole('button', { name: 'Inspection list (CSV)', exact: true }).click();
    await exports.getByRole('button', { name: 'AS9102 FAI (Excel)', exact: true }).click();
    await panel(page).getByRole('tab', { name: 'Review/Edit', exact: true }).click();
    await panel(page).getByRole('button', { name: /^Material/ }).click();
    await input.fill('Brass CW614N');
    await input.press('Enter');
    await panel(page).getByRole('tab', { name: /^Characteristics/ }).click();
    await check(panel(page).locator('.pmi-lab-table')).toContainText('Material: Brass CW614N');
    await panel(page).getByRole('checkbox', { name: 'Inspect 1', exact: true }).uncheck();
    const resume = () => downloadFrom(page, page.getByRole('button', { name: 'Resume download', exact: true }));
    const csv = readFileSync(await resume(), 'utf8');
    assert.match(csv, /12,,Part,Material: Brass CW614N/);
    assert.doesNotMatch(csv, /Steel|M2×0.4-6H/);
    const book = await resume();
    const form2 = execFileSync('unzip', ['-p', book, 'xl/worksheets/sheet2.xml'], { encoding: 'utf8' });
    const form3 = execFileSync('unzip', ['-p', book, 'xl/worksheets/sheet3.xml'], { encoding: 'utf8' });
    assert.match(form2, /MATERIAL Brass CW614N/);
    assert.match(form3, /Material: Brass CW614N/);
    assert.doesNotMatch(form2 + form3, /Steel|M2×0.4-6H/);
    await page.close();
  }

  // Editable: the controls the read-only checks look for are there.
  {
    const page = await openPage(false);
    await check(panel(page).getByText('Read only')).toHaveCount(0);
    await check(panel(page).getByRole('tab', { name: 'Review/Edit' })).toBeVisible();
    // The spindle has one suggestion (no cards or Keep all to see: its row is open, and Keep all needs two).
    await check(panel(page).getByText(/^\d+ suggestions?$/)).toBeVisible();
    await panel(page).locator('.pmi-lab-row-head', { hasText: 'Added by you' }).click();
    await check(panel(page).getByRole('button', { name: '+ Add a requirement' })).toBeVisible();
    await panel(page).locator('.pmi-lab-row-head', { hasText: 'Datums' }).click();
    await check(panel(page).getByRole('button', { name: '+ Add datum' })).toBeVisible();
    // ADR P11: the spindle's file states its datums, so they read "from the file" with no Remove (a
    // save never edits what the file states); a suggested datum's Remove is checked on the shaft below,
    // and so is a hole's open question (the spindle's file states its holes: no hole row).
    await check(panel(page).locator('.pmi-lab-datums .chip.existing').first()).toContainText('from the file');
    await check(panel(page).getByRole('button', { name: /^Remove datum / })).toHaveCount(0);
    // Show controls are visible by default at the bottom (no Show menu button, no Labels checkbox).
    const view = page.locator('#specify-view');
    await check(view.locator('.pmi-lab-show')).toHaveCount(0);
    await check(view.getByLabel('Labels', { exact: true })).toHaveCount(0);
    for (const label of ['Suggested by Specify', 'In the file', 'General dimensions', 'Hide callouts facing away']) {
      await check(view.getByLabel(label, { exact: true })).toBeVisible();
    }
    await page.close();
  }
  // Display keeps the host preview (Material is balloon 12); inspection is an export-only
  // request. Its given-only Material row has no stored ID, so its CSV number stays empty.
  {
    const page = await openPage(false, 'test/fixtures/gramil_shaft_before.step.gz', '#specify-panel .pmi-lab-row-head', '&keep=host');
    const before = (await calls(page)).length;
    await page.evaluate(() => { window.__questionReplies = []; });
    const material = panel(page).locator('[data-question="part.material"] input');
    await material.fill('Aluminium 6082-T6');
    await material.press('Enter');
    assert.deepEqual((await calls(page)).slice(before), ['questions'], 'Enter requests a review');
    await check(material).toBeFocused();
    const characteristics = panel(page).getByRole('tab', { name: /^Characteristics/ });
    await characteristics.click();
    await check(characteristics).toBeFocused();
    assert.deepEqual((await calls(page)).slice(before), ['questions'], 'blur during review must not submit Material again');
    await page.evaluate(() => { window.__questionReplies.shift()(); });
    const row = panel(page).locator('.pmi-lab-table tr').filter({ hasText: 'Material: Aluminium 6082-T6' });
    // This expectation follows origin/main's display; export data no longer replaces preview.
    await check(row.getByRole('checkbox', { name: 'Inspect 12', exact: true })).toBeVisible();
    // By the time the first preview is visible, a duplicate queued before release would
    // already have started its review.
    assert.deepEqual((await calls(page)).slice(before), ['questions', 'preview'], 'blur must not queue a second Material review');
    const download = (name) => inspectionDownload(page, name);
    const csv = readFileSync(await download('Inspection list (CSV)'), 'utf8');
    assert.deepEqual(csv.trimEnd().split('\n').slice(1), [',,Part,Material: Aluminium 6082-T6,,,,,,,']);
    assert.deepEqual((await calls(page)).slice(before), ['questions', 'preview', 'inspection'], 'only the requested CSV adds inspection traffic');
    const path = await download('AS9102 FAI (Excel)');
    const form3 = execFileSync('unzip', ['-p', path, 'xl/worksheets/sheet3.xml'], { encoding: 'utf8' });
    assert.doesNotMatch(form3, /<c r="A5"/, 'unnumbered Material has an empty number cell');
    assert.doesNotMatch(form3, /Material: Aluminium 6082-T6/, 'Material belongs only on Form 2');
    assert.match(execFileSync('unzip', ['-p', path, 'xl/worksheets/sheet2.xml'], { encoding: 'utf8' }), /MATERIAL Aluminium 6082-T6/);
    assert.doesNotMatch(form3, /ISO 2768|Ra 3.2|M4×|M2×/, 'no unapproved part notes or threads');
    assert.deepEqual((await calls(page)).slice(before), ['questions', 'preview', 'inspection', 'inspection'], 'only the requested workbook adds another inspection');
    await page.close();
  }
  // Reviews run in submission order, so the first reply cannot overwrite the later
  // answer to the same question. Free text keeps its spelling.
  {
    const page = await openPage(false, 'test/fixtures/gramil_shaft_before.step.gz', '#specify-panel .pmi-lab-row-head', '&keep=host');
    const before = (await calls(page)).length;
    await page.evaluate(() => { window.__questionReplies = []; });
    const material = panel(page).locator('[data-question="part.material"] input');
    await material.fill('Aluminium 6082-T6');
    await material.press('Enter');
    await material.fill('brass cw614n');
    await material.press('Enter');
    await panel(page).getByRole('tab', { name: /^Characteristics/ }).click();
    assert.deepEqual((await calls(page)).slice(before), ['questions'], 'only the first answer is reviewed while its reply is held');
    await page.evaluate(() => { window.__questionReplies.shift()(); });
    await check.poll(async () => (await calls(page)).slice(before)).toEqual(['questions', 'preview', 'questions']);
    await page.evaluate(() => { window.__questionReplies.shift()(); });
    const table = panel(page).locator('.pmi-lab-table');
    await check(table).toContainText('Material: brass cw614n');
    assert.deepEqual((await calls(page)).slice(before), ['questions', 'preview', 'questions', 'preview']);
    const csv = readFileSync(await inspectionDownload(page, 'Inspection list (CSV)'), 'utf8');
    assert.deepEqual(csv.trimEnd().split('\n').slice(1), [',,Part,Material: brass cw614n,,,,,,,']);
    await page.close();
  }
  // The shaft's datums are suggested, not stated: each can be removed (moved here with b0a828d7's
  // spindle check above; the block it sat in tests a left-out change).
  {
    const page = await openPage(false, 'test/fixtures/gramil_shaft_before.step.gz');
    await panel(page).locator('.pmi-lab-row-head', { hasText: 'Datums' }).click();
    await check(panel(page).getByRole('button', { name: /^Remove datum / }).first()).toBeVisible();
    await openHoleRow(page);
    await check(panel(page).locator('.pmi-lab-item.on')).toBeVisible();
    assert.ok(await panel(page).locator('.pmi-lab-item.on button:not([disabled])').count() > 0, 'an open question has live answer buttons');
    await page.close();
  }
  // Read only: a PMI viewer. The spindle Specify wrote (semantic PMI linked to graphical PMI) shows what is
  // set; nothing to answer, tick, export or change.
  {
    const page = await openPage(true, 'test/fixtures/19-spindle-semantic-links-only.step.gz', '#specify-panel .pmi-lab-table tbody tr');
    const exportsBox = page.locator('#specify-exports');
    await check(panel(page).getByText('PMI (view only)', { exact: true })).toBeVisible();
    await check(panel(page).getByText('Read only')).toHaveCount(0);
    await check(panel(page).getByRole('tab')).toHaveCount(0);
    for (const selector of ['input[type=checkbox]', 'input[type=radio]', 'input[type=text]', 'select', '.pmi-lab-item', '.pmi-lab-row-head']) {
      await check(panel(page).locator(selector), selector).toHaveCount(0);
    }
    for (const name of ['Keep all', 'Reset to default', 'Looks right', '\\+ Add', 'Remove', 'Edit', 'Call out', 'Download CSV']) {
      await check(panel(page).getByRole('button', { name: new RegExp(name) }), name).toHaveCount(0);
    }
    await check(panel(page).getByText(/\d+ suggestions?/)).toHaveCount(0);
    await check(panel(page).getByText('left out of the CSV')).toHaveCount(0);
    await check(exportsBox.getByRole('button')).toHaveCount(0);
    // What is set: the datums, and the characteristics the file states.
    await check(panel(page).getByRole('button', { name: 'Datum A' })).toBeVisible();
    await check(panel(page).getByRole('button', { name: 'Datum B' })).toBeVisible();
    await check(panel(page).getByText('characteristics set on this part.')).toBeVisible();
    const table = panel(page).locator('.pmi-lab-table');
    await check(table.getByText('H11').first()).toBeVisible();
    await check(table.getByText(/⌖/).first()).toBeVisible();
    // What the general tolerance covers, folded under it in one row (no tick box: checked above).
    await check(table.locator('tr.general-summary', { hasText: /dimensions? under ISO 2768/ })).toBeVisible();
    // Single PMI view: no View PMI mode switch or Show dropdown; top direction view buttons and bottom
    // show checkboxes are visible by default, and 3D annotation-plane ink renders directly in blueprint blue.
    await check(page.getByRole('radiogroup', { name: 'View PMI' })).toHaveCount(0);
    const view = page.locator('#specify-view');
    await check(view.locator('.pmi-lab-show')).toHaveCount(0);
    await check(view.getByLabel('Labels', { exact: true })).toHaveCount(0);
    await check(view.getByLabel('Suggested by Specify', { exact: true })).toHaveCount(0);
    for (const label of ['In the file', 'General dimensions', 'Hide callouts facing away']) {
      await check(view.getByLabel(label, { exact: true })).toBeVisible();
    }
    const viewBar = view.locator('.pmi-lab-view-bar');
    await check(viewBar.getByRole('button', { name: 'Best view', exact: true })).toBeVisible();
    assert.ok((await viewBar.locator('button').allTextContents()).every((t) => !/^Plane \d+/.test(t)), 'views are named by direction, not Plane N');
    const overlay = page.locator('#specify-view [data-drawn-graphics]');
    await check.poll(async () => Number(await overlay.getAttribute('data-drawn-graphics'))).toBeGreaterThan(0);
    // The file's own strokes are counted: the attribute an embedding host's tests read.
    assert.ok(Number(await overlay.getAttribute('data-drawn-polylines')) > 0, 'data-drawn-polylines counts the drawn file graphics');
    const drawnItems = (await overlay.getAttribute('data-drawn-items')) ?? '';
    assert.ok(drawnItems.includes('#0c36c0'), `drawn callouts render in dark blueprint blue:\n${drawnItems}`);
    // Unpresented M2 thread renders as a fallback callout on the part; no border/card on plain labels.
    const fallbackIn = page.locator('#specify-view .pmi:not(.hidden) .pmi-in');
    await check(fallbackIn.first()).toBeVisible();
    // A characteristic chosen is marked and lights its drawn callout.
    const row = table.locator('tbody tr').first();
    await row.click();
    await check(row).toHaveClass(/\bon\b/);
    // Clicking a drawn callout on the 3D canvas selects its characteristic row and lights its ink.
    const position = table.locator('tbody tr').filter({ hasText: /⌖.*0\.2/ }).first();
    await check(position).not.toHaveClass(/\bon\b/);
    const pointFor = async (itemId) => {
      const raw = (await overlay.getAttribute('data-drawn-points')) ?? '';
      const line = raw.split('\n').find(l => l.startsWith(`${itemId} `));
      if (!line) return null;
      const [x, y] = line.slice(itemId.length + 1).split(',').map(Number);
      return Number.isFinite(x) && Number.isFinite(y) ? [x, y] : null;
    };
    await check.poll(() => pointFor('position:holes:15.16')).not.toBeNull();
    const [px, py] = await pointFor('position:holes:15.16');
    const overlayBox = await overlay.boundingBox();
    assert.ok(overlayBox, '3D PMI overlay has a bounding box');
    // A fallback label can sit over the point (the layout keeps room for balloons, so callouts move): the click
    // is on the callout, so the labels let it through for this check.
    await page.addStyleTag({ content: '#specify-view .pmi { pointer-events: none !important; } #specify-view .pmi * { pointer-events: none !important; }' });
    await page.mouse.click(overlayBox.x + px, overlayBox.y + py);
    await check(position).toHaveClass(/\bon\b/);
    await check(row).not.toHaveClass(/\bon\b/);
    await check.poll(async () => (await overlay.getAttribute('data-drawn-items')) ?? '').toContain('position:holes:15.16 #007a8c');
    await panel(page).getByRole('button', { name: 'Datum B' }).click();
    await check(panel(page).locator('.pmi-lab-datums .chip.on')).toHaveCount(1);
    await check(table.locator('tbody tr.on')).toHaveCount(0);
    await page.waitForTimeout(500);
    const made = await calls(page);
    // Restore origin/main's read-only call contract: inspection is reserved for exports.
    assert.ok(!made.some(call => call !== 'preview'), `only previews reach the engine: ${made}`);
    assert.ok(made.includes('preview'), 'the answers given are labelled');
    await page.close();
  }
  // A phone: each Show checkbox and its label is a tall enough target to tap (at least 32px, 44px as drawn).
  {
    const page = await openPage(false);
    await page.setViewportSize({ width: 390, height: 800 });
    const labels = page.locator('#specify-view .pmi-lab-show-bar label');
    await check(labels).toHaveCount(5);
    for (const label of await labels.all()) {
      const box = await label.boundingBox();
      const name = (await label.textContent())?.trim();
      assert.ok(box && box.height >= 44, `Show checkbox "${name}" is a 44px target on a phone, got ${box?.height}`);
      const input = await label.locator('input').boundingBox();
      assert.ok(input && input.width >= 20, `Show checkbox "${name}" is drawn at touch size, got ${input?.width}`);
    }
    await page.close();
  }
  // Many-plane part (CTC04) with General dimensions on in an 800x400 clipped viewer: view buttons
  // stay capped in a compact bar and the rest remain reachable via the "More views ▾" dropdown.
  {
    const page = await openPage(true, '../tests/fixtures/characteristic-numbers-ctc04-main.step.gz', '#specify-view .pmi-lab-view-bar button');
    await page.setViewportSize({ width: 800, height: 400 });
    await page.addStyleTag({
      content: '#specify-view { position: fixed; inset: 0 378px 0 0; height: auto !important; overflow: hidden; } #specify-view .pmi-lab.pmi-lab-pane { position: absolute; }',
    });
    const view = page.locator('#specify-view');
    await view.getByLabel('General dimensions', { exact: true }).check();
    const viewBar = view.locator('.pmi-lab-view-bar');
    const moreToggle = viewBar.getByRole('button', { name: 'More views ▾', exact: true });
    // CTC04 is a Specify-written file, laid out as Specify lays out an unsaved one (the same before and after
    // Save): four views. The overflow menu is exercised only when a layout makes more than fit.
    await check(viewBar.getByRole('button', { name: 'Best view', exact: true })).toBeVisible();
    const overflows = await moreToggle.count() > 0;
    const barButtons = await viewBar.locator('.pmi-lab-view-pills > button').allTextContents();
    assert.ok(barButtons.length <= 5, `view bar stays compact, got ${barButtons.length}: ${barButtons.join(', ')}`);
    const viewBox = await view.boundingBox();
    const barBox = await viewBar.boundingBox();
    assert.ok(viewBox && barBox, 'view and view bar have bounding boxes');
    assert.ok(barBox.y + barBox.height <= viewBox.y + viewBox.height, `view bar stays inside 800x400 viewer (${barBox.y + barBox.height} <= ${viewBox.y + viewBox.height})`);
    assert.ok(barBox.height < 120, `view bar does not wrap into a tall column (${barBox.height}px)`);
    if (overflows) {
    await moreToggle.click();
    const menu = viewBar.locator('.pmi-lab-view-more-menu');
    await check(menu).toBeVisible();
    const menuBox = await menu.boundingBox();
    assert.ok(menuBox, 'More views menu has a bounding box');
    assert.ok(menuBox.y + menuBox.height <= viewBox.y + viewBox.height, `More views menu fits inside clipped viewer (${menuBox.y + menuBox.height} <= ${viewBox.y + viewBox.height})`);
    assert.ok(menuBox.x >= viewBox.x && menuBox.x + menuBox.width <= viewBox.x + viewBox.width, 'More views menu stays horizontally inside clipped viewer');
    const menuButtons = menu.locator('button');
    const overflowCount = await menuButtons.count();
    assert.ok(overflowCount >= 10, `expected many overflow views on CTC04, got ${overflowCount}`);
    const lastView = menuButtons.last();
    await lastView.scrollIntoViewIfNeeded();
    await lastView.click();
    await check(menu).toHaveCount(0);
    }
    await page.close();
  }
  assert.deepEqual(problems, [], 'no page errors');
  console.log('specify-browser passed');
} finally {
  await browser.close();
  server?.kill();
}
