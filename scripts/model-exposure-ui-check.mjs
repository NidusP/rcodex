// Local fixture regression check. Requires Node.js and Chrome, no npm test dependency.
// Start frontend only: pnpm exec vite --host 127.0.0.1 --port 5185 --strictPort
// Run: node scripts/model-exposure-ui-check.mjs http://127.0.0.1:5185
import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const baseUrl = process.argv[2] ?? 'http://127.0.0.1:5185';
const chromePath = process.env.MODEL_UI_CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const profile = await mkdtemp(join(tmpdir(), 'rcodex-model-ui-'));
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
async function navigate(query = '') {
  await call('Page.navigate', { url: `${baseUrl}/?ui-fixture=1&no-failure=1${query}` });
  await waitFor(() => Boolean(document.querySelector('.space-channel-entry')) && document.body.textContent.includes('隔离空间 A'));
  await waitFor(() => !document.body.textContent.includes('正在读取模型透出状态'));
}
async function openCatalog() {
  await click('.space-channel-entry');
  await waitFor(() => Boolean(document.querySelector('[aria-label="Team Relay 操作"] [aria-label="查看支持模型"]')));
  await click('[aria-label="Team Relay 操作"] [aria-label="查看支持模型"]');
  await waitFor(() => Boolean(document.querySelector('[aria-label="透出 vision-test-model"]')));
}
async function waitForProbe(modelId, expected) {
  await waitFor((id, text) => {
    const input = document.querySelector(`[aria-label="透出 ${id}"]`);
    const row = input?.closest('.provider-model-catalog-row');
    return row?.textContent.includes(text) && !row?.textContent.includes('正在保存并检测');
  }, modelId, expected);
}
const checkSelector = (modelId) => `[aria-label="透出 ${modelId}"]`;
const probeSelector = (modelId) => `[aria-label="刷新图片探针 ${modelId}"]`;
const countCalls = () => evaluate(() => Number(document.documentElement.dataset.fixtureImageProbeCalls ?? 0));

try {
  const target = await call('Target.createTarget', { url: 'about:blank' }, false);
  sessionId = (await call('Target.attachToTarget', { targetId: target.targetId, flatten: true }, false)).sessionId;
  await call('Runtime.enable');
  await call('Page.enable');
  await navigate();
  assert('workspace only offers initially exposed model', await evaluate(() => {
    const group = document.querySelector('[aria-label="Team Relay 的模型"]');
    return Boolean(group) && !group.textContent.includes('Vision Test Model');
  }));
  await openCatalog();
  assert('referenced model cannot be unexposed', await evaluate(() => {
    const input = document.querySelector('[aria-label="透出 grok-code-fast-1"]');
    return input?.checked && input.disabled;
  }));
  assert('internal and image-generation models have no exposure checkbox', await evaluate(() =>
    !document.querySelector('[aria-label="透出 codex-auto-review"]') &&
    !document.querySelector('[aria-label="透出 grok-imagine-image-2.0"]')));
  assert('opening catalog sends no image probe', await countCalls() === 0);
  await click(checkSelector('vision-test-model'));
  assert('other model remains editable during probe', await evaluate(() =>
    !document.querySelector('[aria-label="透出 text-only-test-model"]').disabled));
  await waitForProbe('vision-test-model', '支持图片');
  assert('first exposure probes once', await countCalls() === 1);
  await click(checkSelector('vision-test-model'));
  await waitFor(() => {
    const input = document.querySelector('[aria-label="透出 vision-test-model"]');
    return input && !input.checked && !input.disabled;
  });
  await click(checkSelector('vision-test-model'));
  await waitForProbe('vision-test-model', '支持图片');
  assert('reexposure reuses cached result', await countCalls() === 1);
  await click(probeSelector('vision-test-model'));
  await waitForProbe('vision-test-model', '支持图片');
  assert('explicit refresh sends another probe', await countCalls() === 2);
  await click(checkSelector('text-only-test-model'));
  await waitForProbe('text-only-test-model', '不支持图片');
  assert('unsupported model remains exposed for text', await evaluate(() =>
    document.querySelector('[aria-label="透出 text-only-test-model"]').checked));

  await navigate('&model-probe=failed');
  await openCatalog();
  await click(checkSelector('vision-test-model'));
  await waitForProbe('vision-test-model', '图片检测失败');
  assert('timeout remains exposed and is shown as failed', await evaluate(() => {
    const input = document.querySelector('[aria-label="透出 vision-test-model"]');
    const row = input.closest('.provider-model-catalog-row');
    return input.checked && row.textContent.includes('请求超时') && !row.textContent.includes('不支持图片');
  }));

  await navigate('&exposure-load-error=1');
  await openCatalog();
  assert('load failure is visible and prevents new exposure', await evaluate(() =>
    document.body.textContent.includes('模型透出状态读取失败') &&
    document.querySelector('[aria-label="透出 vision-test-model"]').disabled));

  await navigate('&no-exposed-models=1');
  await click('[aria-label="添加渠道"]');
  await waitFor(() => Boolean(document.querySelector('[aria-label="未加入的渠道"]')));
  assert('channel without exposed models is disabled with explanation', await evaluate(() => {
    const items = [...document.querySelectorAll('[role="menuitem"]')];
    const spare = items.find((node) => node.textContent.includes('Spare Relay'));
    const blocked = spare?.closest('.disabled-hint');
    const disabled = spare.disabled || spare.getAttribute('data-disabled') !== null || spare.getAttribute('aria-disabled') === 'true';
    return Boolean(spare && blocked) && disabled
      && spare.textContent.includes('不可用')
      && (blocked.getAttribute('aria-label') ?? '').includes('请先在模型渠道中选择要透出的模型');
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
