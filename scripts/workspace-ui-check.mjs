// Local fixture regression check. Requires Node.js and Chrome, no npm test dependency.
// Start frontend only: pnpm exec vite --host 127.0.0.1 --port 5185 --strictPort
// Run: node scripts/workspace-ui-check.mjs http://127.0.0.1:5185
import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const baseUrl = process.argv[2] ?? 'http://127.0.0.1:5185';
const chromePath = process.env.WORKSPACE_UI_CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const profile = await mkdtemp(join(tmpdir(), 'rcodex-workspace-ui-'));
const checks = [];
const pageErrors = [];
const chrome = spawn(chromePath, [
  '--headless=new', '--remote-debugging-pipe', '--no-first-run', '--no-default-browser-check',
  '--disable-background-networking', '--disable-component-update', '--disable-default-apps',
  '--disable-extensions', '--disable-sync', '--metrics-recording-only', '--mute-audio',
  '--window-size=1280,1100', `--user-data-dir=${profile}`, 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe'] });
let chromeErrors = '';
chrome.stderr.on('data', (chunk) => { chromeErrors = (chromeErrors + chunk.toString()).slice(-5000); });
let nextId = 1;
let bytes = Buffer.alloc(0);
let sessionId;
const pending = new Map();
chrome.on('error', (error) => {
  for (const task of pending.values()) task.reject(error);
  pending.clear();
});
chrome.stdio[4].on('data', (chunk) => {
  bytes = Buffer.concat([bytes, chunk]);
  let end;
  while ((end = bytes.indexOf(0)) !== -1) {
    const frame = bytes.subarray(0, end).toString();
    bytes = bytes.subarray(end + 1);
    if (!frame) continue;
    const message = JSON.parse(frame);
    if (message.id !== undefined) {
      const task = pending.get(message.id);
      if (!task) continue;
      pending.delete(message.id);
      clearTimeout(task.timer);
      if (message.error) task.reject(new Error(JSON.stringify(message.error)));
      else task.resolve(message.result);
    } else if (message.method === 'Runtime.exceptionThrown') {
      const detail = message.params.exceptionDetails;
      pageErrors.push(detail.exception?.description ?? detail.text);
    }
  }
});
function call(method, params = {}, attached = true) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`CDP timeout: ${method}; ${chromeErrors}`));
    }, 15000);
    pending.set(id, { resolve, reject, timer });
    chrome.stdio[3].write(JSON.stringify({ id, method, params, ...(attached && sessionId ? { sessionId } : {}) }) + '\0');
  });
}
async function evaluate(fn, ...args) {
  const result = await call('Runtime.evaluate', {
    expression: `(${fn.toString()})(...${JSON.stringify(args)})`,
    awaitPromise: true, returnByValue: true,
  });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
  return result.result.value;
}
async function waitFor(fn, ...args) {
  const deadline = Date.now() + 12000;
  while (Date.now() < deadline) {
    if (await evaluate(fn, ...args)) return;
    await delay(60);
  }
  throw new Error(`DOM wait timed out: ${fn.toString()} args=${JSON.stringify(args)}`);
}
function assert(name, condition, details = undefined) {
  const item = { name, passed: Boolean(condition), ...(details !== undefined ? { details } : {}) };
  checks.push(item);
  if (!condition) throw new Error(`Assertion failed: ${name} ${JSON.stringify(details ?? '')}`);
}
async function click(selector) {
  const clicked = await evaluate((value) => {
    const node = document.querySelector(value);
    if (!node || node.disabled) return false;
    node.click();
    return true;
  }, selector);
  if (!clicked) throw new Error(`No enabled element: ${selector}`);
}

async function navigate(query = '', reset = false) {
  if (reset) await evaluate(() => localStorage.removeItem('rcodex.fixture.workspaces'));
  await call('Page.navigate', { url: `${baseUrl}/?ui-fixture=1&no-failure=1&workspace-fixture=1${query}` });
  await waitFor(() => Boolean(document.querySelector('.space-channel-entry')) && Boolean(document.querySelector('[aria-label="新增隔离空间"]')));
  await delay(800);
}
const tabs = () => evaluate(() => [...document.querySelectorAll('[role="tab"]')].map((tab) => tab.textContent.replace('●', '').trim()));
const selected = () => evaluate(() => document.querySelector('[role="tab"][aria-selected="true"]')?.textContent.replace('●', '').trim() ?? null);
async function selectTab(name) {
  const clicked = await evaluate((label) => {
    const tab = [...document.querySelectorAll('[role="tab"]')].find((node) => node.textContent.replace('●', '').trim() === label);
    if (!tab || tab.disabled) return false;
    tab.click(); return true;
  }, name);
  if (!clicked) throw new Error(`Unable to select tab ${name}`);
}
async function clickText(scope, text) {
  const clicked = await evaluate((selector, label) => {
    const node = [...document.querySelectorAll(selector + ' button')].find((button) => button.textContent.trim() === label);
    if (!node || node.disabled) return false;
    node.click(); return true;
  }, scope, text);
  if (!clicked) throw new Error(`No enabled button ${text} in ${scope}`);
}
async function openDeletion() {
  await click('[role="tab"][aria-selected="true"] .workspace-tab-delete');
  await waitFor(() => Boolean(document.querySelector('#workspace-delete-title')));
}
async function confirmDeletion(expectedCount) {
  await clickText('[aria-labelledby="workspace-delete-title"]', '确认永久删除');
  await waitFor((count) => document.querySelectorAll('[role="tab"]').length === count && !document.querySelector('#workspace-delete-title'), expectedCount);
}
const deleteCalls = () => evaluate(() => Number(document.documentElement.dataset.fixtureWorkspaceDeleteCalls ?? 0));
try {
  const target = await call('Target.createTarget', { url: 'about:blank' }, false);
  sessionId = (await call('Target.attachToTarget', { targetId: target.targetId, flatten: true }, false)).sessionId;
  await call('Runtime.enable');
  await call('Page.enable');
  await navigate('&multiple-workspaces=1');
  assert('fixed A-B-C order despite preferred-first snapshot', JSON.stringify(await tabs()) === JSON.stringify(['隔离空间 A', '隔离空间 B', '隔离空间 C']));
  const spaceB = await evaluate(() => JSON.parse(localStorage.getItem('rcodex.fixture.workspaces')).compositions.find((item) => item.launchPresetId === 'preset-managed-app-b'));
  for (const name of ['隔离空间 B', '隔离空间 C', '隔离空间 A', '隔离空间 C']) {
    await selectTab(name);
    await waitFor((label) => {
      const tab = document.querySelector('[role="tab"][aria-selected="true"]');
      return tab?.textContent.replace('●', '').trim() === label && ![...document.querySelectorAll('[role="tab"]')].some((item) => item.disabled);
    }, name);
    assert(`select ${name} preserves order`, JSON.stringify(await tabs()) === JSON.stringify(['隔离空间 A', '隔离空间 B', '隔离空间 C']));
  }
  await navigate('&multiple-workspaces=1');
  assert('reload keeps slot order and selected space', JSON.stringify(await tabs()) === JSON.stringify(['隔离空间 A', '隔离空间 B', '隔离空间 C']) && await selected() === '隔离空间 C');
  assert('deletion entry stays hidden until its tab is hovered', await evaluate(() => {
    const trash = document.querySelector('[role="tab"][aria-selected="true"] .workspace-tab-delete');
    return Boolean(trash) && !trash.classList.contains('is-disabled') && getComputedStyle(trash).visibility === 'hidden';
  }));
  const hoveredTab = await evaluate(() => {
    const tab = document.querySelector('[role="tab"][aria-selected="true"]');
    const rect = tab.getBoundingClientRect();
    return { x: Math.round(rect.x + rect.width / 2), y: Math.round(rect.y + rect.height / 2) };
  });
  await call('Input.dispatchMouseEvent', { type: 'mouseMoved', x: hoveredTab.x, y: hoveredTab.y });
  await delay(150);
  assert('hovering a deletable tab reveals its trash', await evaluate(() => {
    const trash = document.querySelector('[role="tab"][aria-selected="true"] .workspace-tab-delete');
    return getComputedStyle(trash).visibility === 'visible';
  }));
  await openDeletion();
  assert('permanent deletion confirmation names space and data', await evaluate(() => {
    const dialog = document.querySelector('[aria-labelledby="workspace-delete-title"]');
    return dialog.textContent.includes('隔离空间 C') && dialog.textContent.includes('无法撤销') && dialog.textContent.includes('完整请求档案') && dialog.textContent.includes('全局模型渠道与密钥');
  }));
  await clickText('[aria-labelledby="workspace-delete-title"]', '取消');
  await waitFor(() => !document.querySelector('#workspace-delete-title'));
  assert('cancel leaves all spaces and sends no deletion', await deleteCalls() === 0 && (await tabs()).length === 3);
  await openDeletion();
  await confirmDeletion(2);
  assert('confirm deletes only selected C and chooses lowest slot', JSON.stringify(await tabs()) === JSON.stringify(['隔离空间 A', '隔离空间 B']) && await selected() === '隔离空间 A' && await deleteCalls() === 1);
  const spaceBAfter = await evaluate(() => JSON.parse(localStorage.getItem('rcodex.fixture.workspaces')).compositions.find((item) => item.launchPresetId === 'preset-managed-app-b'));
  assert('other space configuration stays unchanged', JSON.stringify(spaceB) === JSON.stringify(spaceBAfter));
  await click('[aria-label="新增隔离空间"]');
  await waitFor(() => document.querySelectorAll('[role="tab"]').length === 3 && !document.querySelector('[aria-label="新增隔离空间"]').disabled);
  assert('new space reuses empty slot C without renaming others', JSON.stringify(await tabs()) === JSON.stringify(['隔离空间 A', '隔离空间 B', '隔离空间 C']));
  await openDeletion(); await confirmDeletion(2);
  await openDeletion(); await confirmDeletion(1);
  await openDeletion(); await confirmDeletion(0);
  assert('last space can be removed and leaves explicit empty state', await evaluate(() => document.body.textContent.includes('尚未配置工作区') && !document.querySelector('.workspace-tab-delete') && !document.querySelector('[aria-label="新增隔离空间"]').disabled));
  await navigate('&multiple-workspaces=1');
  assert('empty state survives reload without automatic recreation', (await tabs()).length === 0);
  await click('[aria-label="新增隔离空间"]');
  await waitFor(() => document.querySelectorAll('[role="tab"]').length === 1);
  assert('empty state can create a new empty A', await selected() === '隔离空间 A' && await evaluate(() => {
    const saved = JSON.parse(localStorage.getItem('rcodex.fixture.workspaces'));
    return saved.presets.length === 1 && saved.compositions[0].models.length === 0 && saved.compositions[0].apiKeyProviderIds.length === 0;
  }));
  await navigate('&multiple-workspaces=1');
  await click('[aria-label="添加渠道"]');
  await waitFor(() => Boolean(document.querySelector('.channel-add-menu')));
  assert('unexposed channel stays listed but marked unavailable with hover hint', await evaluate(() => {
    const items = [...document.querySelectorAll('.channel-add-menu button')];
    const spare = items.find((button) => button.textContent.includes('Spare Relay'));
    const relay = items.find((button) => button.textContent.includes('Team Relay'));
    const blocked = spare?.closest('.disabled-hint');
    return Boolean(spare && relay && blocked)
      && spare.disabled === true
      && spare.textContent.includes('不可用')
      && (blocked.getAttribute('aria-label') ?? '').includes('透出')
      && relay.disabled === false
      && relay.textContent.includes('可用')
      && !relay.closest('.disabled-hint');
  }));
  await navigate('&multiple-workspaces=1&workspace-active=1', true);
  assert('active orphaned CLI prevents workspace deletion', await evaluate(() => {
    const trash = document.querySelector('[role="tab"][aria-selected="true"] .workspace-tab-delete');
    return Boolean(trash?.classList.contains('is-disabled')) && getComputedStyle(trash).display === 'none' && Boolean(trash?.getAttribute('aria-label')?.includes('先停止'));
  }));
  await navigate('&multiple-workspaces=1&workspace-selection-error=1', true);
  await selectTab('隔离空间 B');
  await delay(350);
  await waitFor(() => ![...document.querySelectorAll('[role="tab"]')].some((item) => item.disabled));
  assert('failed selection rolls back to previous space without reordering', await selected() === '隔离空间 A' && JSON.stringify(await tabs()) === JSON.stringify(['隔离空间 A', '隔离空间 B', '隔离空间 C']));
  await navigate('&multiple-workspaces=1&workspace-delete-error=1', true);
  await openDeletion();
  await clickText('[aria-labelledby="workspace-delete-title"]', '确认永久删除');
  await waitFor(() => document.querySelector('[aria-labelledby="workspace-delete-title"]')?.textContent.includes('删除未完成'));
  assert('failed deletion keeps space and explains partial deletion/retry', (await tabs()).length === 3 && await evaluate(() => {
    const dialog = document.querySelector('[aria-labelledby="workspace-delete-title"]');
    return dialog.textContent.includes('测试目录暂不可用') && dialog.textContent.includes('部分数据可能无法恢复') && [...dialog.querySelectorAll('button')].some((button) => button.textContent.trim() === '确认永久删除' && !button.disabled);
  }));
  assert('no uncaught browser errors', pageErrors.length === 0, pageErrors);
  console.log(JSON.stringify({ passed: true, checks }, null, 2));
} catch (error) {
  let pageText = '';
  try { pageText = await evaluate(() => document.body.innerText.slice(0, 6000)); } catch {}
  console.log(JSON.stringify({ passed: false, error: String(error), checks, pageErrors, pageText, chromeErrors }, null, 2));
  process.exitCode = 1;
} finally {
  for (const task of pending.values()) clearTimeout(task.timer);
  pending.clear();
  if (chrome.exitCode === null) {
    chrome.kill('SIGTERM');
    await Promise.race([new Promise((resolve) => chrome.once('exit', resolve)), delay(2000)]);
    if (chrome.exitCode === null) chrome.kill('SIGKILL');
  }
  await rm(profile, { recursive: true, force: true });
}
