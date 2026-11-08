// 无头浏览器夹具回归入口：自动起停仅前端 vite，依次运行两份夹具脚本。
// 用法：pnpm run ui-check（可用 UI_CHECK_PORT 换端口；Chrome 路径沿用各脚本的环境变量）。
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const port = process.env.UI_CHECK_PORT ?? '5185';
const url = `http://127.0.0.1:${port}`;
const viteBin = resolve(root, 'node_modules', 'vite', 'bin', 'vite.js');
const suites = ['scripts/workspace-ui-check.mjs', 'scripts/model-exposure-ui-check.mjs'];

const vite = spawn(process.execPath, [viteBin, '--host', '127.0.0.1', '--port', port, '--strictPort'], {
  cwd: root,
  stdio: ['ignore', 'ignore', 'pipe'],
});
let viteLog = '';
vite.stderr.on('data', (chunk) => { viteLog += chunk.toString(); });

async function waitReady() {
  for (let i = 0; i < 60; i += 1) {
    try {
      const response = await fetch(url);
      if (response.ok) return true;
    } catch {}
    await delay(500);
  }
  return false;
}

let failed = false;
try {
  if (!(await waitReady())) {
    throw new Error(`vite 未在 30 秒内就绪：${viteLog.slice(-800)}`);
  }
  for (const suite of suites) {
    console.log(`\n===== ${suite} =====`);
    const result = await new Promise((resolveExit) => {
      const child = spawn(process.execPath, [resolve(root, suite), url], {
        cwd: root,
        stdio: 'inherit',
      });
      child.on('exit', (code) => resolveExit(code ?? 1));
    });
    if (result !== 0) failed = true;
  }
} catch (error) {
  console.error(String(error));
  failed = true;
} finally {
  vite.kill('SIGTERM');
  await delay(500);
  if (vite.exitCode === null) vite.kill('SIGKILL');
}

console.log(failed ? '\nui-check: FAILED' : '\nui-check: all suites passed');
process.exit(failed ? 1 : 0);
