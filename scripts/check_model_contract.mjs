#!/usr/bin/env node
// check_model_contract.mjs — 校验 src/models.rs MODEL_META_ROWS 与 tests/fixtures/freebuff-models.snapshot.json 的 id/availability 对齐
// 用法: node scripts/check_model_contract.mjs [models.rs 路径] [fixture 路径]
import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const modelsPath = process.argv[2] || path.join(root, 'src', 'models.rs');
const fixturePath = process.argv[3] || path.join(root, 'tests', 'fixtures', 'freebuff-models.snapshot.json');

if (!fs.existsSync(modelsPath)) { console.error('✗ 找不到 models.rs: ' + modelsPath); process.exit(2); }
if (!fs.existsSync(fixturePath)) { console.error('✗ 找不到 fixture: ' + fixturePath); process.exit(2); }

const src = fs.readFileSync(modelsPath, 'utf8');
const fixture = JSON.parse(fs.readFileSync(fixturePath, 'utf8'));

// 提取 MODEL_META_ROWS 里每个 MetaRow 的 id / availability
const rows = [];
const blockRe = /MetaRow\s*\{([^}]*)\}/g;
let m;
while ((m = blockRe.exec(src)) !== null) {
  const id = m[1].match(/id:\s*"([^"]+)"/);
  const avail = m[1].match(/availability:\s*"([^"]+)"/);
  if (id) rows.push({ id: id[1], availability: avail ? avail[1] : null });
}

const catalog = fixture.models.filter((r) => r.catalog === true);
const errors = [];
const metaById = new Map(rows.map((r) => [r.id, r]));

for (const row of catalog) {
  const meta = metaById.get(row.id);
  if (!meta) { errors.push(`fixture catalog 行在 models.rs 缺失: ${row.id}`); continue; }
  if (meta.availability !== row.availability) {
    errors.push(`availability 不一致: ${row.id}  fixture=${row.availability}  models.rs=${meta.availability}`);
  }
}
for (const r of rows) {
  if (!fixture.models.some((x) => x.id === r.id)) {
    errors.push(`models.rs 元数据行不在 fixture 中: ${r.id}（请更新快照）`);
  }
}

if (errors.length) {
  console.error('✗ 模型合同漂移 ' + errors.length + ' 处：');
  for (const e of errors) console.error('  - ' + e);
  process.exit(1);
}
console.log(`✓ 模型合同一致：fixture catalog=${catalog.length} 行 / models.rs meta=${rows.length} 行（id+availability 全对齐）`);