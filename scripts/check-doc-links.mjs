// 文档链接与代码路径检查（第一轮只报告，不改退出码）。
// 用法：pnpm run check:docs（= node scripts/check-doc-links.mjs [--strict]）
// --strict：active 文档存在失效引用时退出码 1（供后续 CI 启用）。
import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { dirname, resolve, relative, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const strict = process.argv.includes('--strict');

function listMarkdown(dir, prefix = '') {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const rel = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) files.push(...listMarkdown(join(dir, entry.name), rel));
    else if (entry.name.endsWith('.md')) files.push(rel);
  }
  return files;
}

const docFiles = [
  'AGENTS.md',
  'README.md',
  // 嵌套子项目 AGENTS.md（agents.md 规范：就近生效）
  ...['src', 'src-tauri', 'website', 'scripts']
    .map((dir) => join(repoRoot, dir, 'AGENTS.md'))
    .filter((path) => existsSync(path))
    .map((path) => relative(repoRoot, path).split('\\').join('/')),
  ...listMarkdown(join(repoRoot, 'docs')).map((rel) => `docs/${rel}`),
];

function docStatus(absPath) {
  // 前 200 行：常规文档标记紧跟标题；带 YAML frontmatter 的文档（如设计资产）标记在其后。
  const lines = readFileSync(absPath, 'utf8').split('\n').slice(0, 200);
  for (const line of lines) {
    const match = line.match(/^> 状态：(active|historical|archived)（/);
    if (match) return { status: match[1], marked: true };
  }
  return { status: 'active', marked: false }; // AGENTS.md、README.md 与未标记文档按当前文档对待。
}

// 反引号中出现的仓库路径：限定已知名义根，避免把普通代码片段当路径。
const repoPathPattern =
  /`((?:AGENTS\.md|README\.md|package\.json)|(?:docs|src|src-tauri|scripts|website)\/[A-Za-z0-9._\-/]+)`/g;

// 文件内豁免指令：<!-- doc-links-allow: 路径1, 路径2 -->
// 用于历史完成记录里「故意保留」的已删除文件引用；匹配目标跳过检查。
function allowedTargets(content) {
  const allowed = new Set();
  for (const [, list] of content.matchAll(/<!--\s*doc-links-allow:\s*([^>]+?)-->/g)) {
    for (const item of list.split(/[,\n]/)) {
      const value = item.trim().replace(/`/g, '');
      if (value) allowed.add(value);
    }
  }
  return allowed;
}

const brokenByStatus = { active: [], historical: [], archived: [] };
const missingStatus = [];
const statePageIssues = [];
let checkedLinks = 0;
let checkedPaths = 0;

function record(doc, kind, target, detail) {
  brokenByStatus[doc.status].push({ doc: doc.rel, kind, target, detail });
}

for (const rel of docFiles) {
  const abs = join(repoRoot, rel);
  const marked = docStatus(abs);
  const doc = { rel, abs, status: marked.status };
  const content = readFileSync(abs, 'utf8');
  const allowed = allowedTargets(content);
  const isAllowed = (target) => allowed.has(target) || allowed.has(target.replace(/\/$/, ''));

  // 1) Markdown 相对链接
  for (const [, target] of content.matchAll(/\[[^\]]*\]\(([^)\s]+)\)/g)) {
    if (/^(https?:|#|mailto:)/.test(target)) continue;
    if (isAllowed(target)) continue;
    checkedLinks += 1;
    const withoutAnchor = target.split('#')[0];
    if (!withoutAnchor) continue;
    const resolved = resolve(dirname(abs), decodeURIComponent(withoutAnchor));
    if (!existsSync(resolved)) {
      record(doc, 'markdown-link', target, relative(repoRoot, resolved));
    }
  }

  // 2) 反引号中的仓库代码/文档路径（带 :line 后缀的先剥离）
  for (const match of content.matchAll(repoPathPattern)) {
    const raw = match[1];
    if (isAllowed(raw)) continue;
    const withoutLine = raw.replace(/:\d+$/, '');
    checkedPaths += 1;
    const resolved = resolve(repoRoot, withoutLine);
    if (!existsSync(resolved)) {
      record(doc, 'repo-path', withoutLine, '仓库内不存在该文件或目录');
    }
  }

  // 3) 状态标记：索引类文件（AGENTS/README 及其嵌套）豁免，其余文档必须有标记
  const isIndex = rel === 'AGENTS.md' || rel === 'README.md' || rel.endsWith('/AGENTS.md');
  if (!isIndex && !marked.marked) {
    missingStatus.push(rel);
  }

  // 4) 状态页维护：最近交接条目上限（3 条主条目 + 1 条「更早」概览）
  if (rel === 'docs/CURRENT-STATE.md') {
    const lines = content.split('\n');
    const sectionStart = lines.findIndex((line) => line.startsWith('## 6.'));
    if (sectionStart === -1) {
      statePageIssues.push('缺少「6. 最近任务交接」节');
    } else {
      const sectionEnd = lines.findIndex((line, i) => i > sectionStart && line.startsWith('## '));
      const bullets = lines
        .slice(sectionStart + 1, sectionEnd === -1 ? lines.length : sectionEnd)
        .filter((line) => line.startsWith('- '));
      if (bullets.length > 4) {
        statePageIssues.push(`最近交接 ${bullets.length} 条（上限 4：3 条主条目 + 1 条更早概览）`);
      }
    }
  }
}

for (const status of ['active', 'historical', 'archived']) {
  const items = brokenByStatus[status];
  const label = { active: '当前文档（需修正）', historical: '历史文档（仅提示，不改写）', archived: '已归档文档（仅提示，不改写）' }[status];
  console.log(`\n== ${status} — ${label}：${items.length} 处`);
  for (const item of items) console.log(`  [${item.kind}] ${item.doc} → ${item.target}`);
}

console.log(`\n== 缺状态标记（需补标记）：${missingStatus.length} 处`);
for (const rel of missingStatus) console.log(`  ${rel}`);
console.log(`\n== 状态页维护（docs/CURRENT-STATE.md）：${statePageIssues.length} 项`);
for (const issue of statePageIssues) console.log(`  ${issue}`);

console.log(`\nchecked: ${docFiles.length} docs, ${checkedLinks} links, ${checkedPaths} repo paths`);
const activeBroken = brokenByStatus.active.length;
const strictFailures = activeBroken + missingStatus.length + statePageIssues.length;
if (strict && strictFailures > 0) {
  console.error(`strict: ${activeBroken} active broken / ${missingStatus.length} missing status / ${statePageIssues.length} state-page issues`);
  process.exit(1);
}
console.log('report-only mode: exit 0（--strict 时 active 失效、缺标记、状态页超限才失败）');
