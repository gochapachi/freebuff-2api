// 从上游 freebuff-models.ts 提取模型合同行 → 生成 tests/fixtures/freebuff-models.snapshot.json
// 用法: node scripts/extract_upstream_models.mjs <freebuff-models.ts 路径> [<common/src 根路径>]
// 输出: 打印 JSON（重定向到 tests/fixtures/freebuff-models.snapshot.json）
import fs from 'node:fs';
import path from 'node:path';

const modelsPath = process.argv[2];
const commonRoot = process.argv[3] || path.dirname(path.dirname(modelsPath));
if (!modelsPath) { console.error('usage: node extract_upstream_models.mjs <freebuff-models.ts> [commonRoot]'); process.exit(2); }
const src = fs.readFileSync(modelsPath, 'utf8');

// ---------- 1) 解析 id 常量对象引用（model-config.ts 等） ----------
const objMembers = {}; // "ObjName.key" -> literal
function scanObjMembers(file) {
  if (!fs.existsSync(file)) return;
  const s = fs.readFileSync(file, 'utf8');
  const objRe = /(?:export\s+)?const\s+(\w+)\s*=\s*\{([\s\S]*?)\}\s*as\s+const/g;
  let m;
  while ((m = objRe.exec(s)) !== null) {
    const name = m[1], body = m[2];
    const memRe = /(\w+)\s*:\s*(['"])([^'"]+)\2/g;
    let mm;
    while ((mm = memRe.exec(body)) !== null) objMembers[name + '.' + mm[1]] = mm[3];
  }
}
const mc = path.join(commonRoot, 'constants', 'model-config.ts');
const ids = path.join(commonRoot, 'constants', 'freebuff-model-ids.ts');
scanObjMembers(mc);
scanObjMembers(path.join(commonRoot, 'constants', 'freebuff-model-entitlements.ts'));

// ---------- 2) 收集 MODEL_ID 常量定义（多行兼容） ----------
const idMap = {}; // const name -> literal id
const defRe = /(?:export\s+)?const\s+(FREEBUFF_\w+_MODEL_ID|LIMITED_FREEBUFF_MODEL_ID|FALLBACK_FREEBUFF_MODEL_ID)\s*=\s*([^\n;]+)/g;
function collectDefs(s, file) {
  let m;
  let lastIndex = 0;
  const re = new RegExp(defRe.source, 'g');
  while ((m = re.exec(s)) !== null) { lastIndex = m.index; }
  // 多行赋值：值到行尾；换行后 'xxx' 续行（第二行以引号开头）
  const lines = s.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const mm = lines[i].match(/(?:export\s+)?const\s+(FREEBUFF_\w+_MODEL_ID|LIMITED_FREEBUFF_MODEL_ID|FALLBACK_FREEBUFF_MODEL_ID)\s*=\s*(.*)$/);
    if (!mm) continue;
    let val = mm[2].trim().replace(/\/\/.*$/,'').trim();
    if (val.endsWith(',') ) val = val.replace(/,$/, '');
    if (val === '') { // 值在下一行(可能是字符串字面量行)
      const next = lines[i+1] ? lines[i+1].trim().replace(/\/\/.*$/, '').trim() : '';
      const q = next.match(/^(['"])(.*?)\1$/);
      if (q) val = q[2];
    }
    if (val.startsWith("'") || val.startsWith('"')) { idMap[mm[1]] = val.slice(1, val.length - 1); }
    else if (objMembers[val]) { idMap[mm[1]] = objMembers[val]; }
    else if (val.startsWith('FREEBUFF_') || /^[A-Z_]+$/.test(val)) {
      // 引用其它常量：稍后解析
      idMap[mm[1]] = '__REF:' + val;
    }
  }
  void lastIndex;
}
collectDefs(src, modelsPath);
if (fs.existsSync(ids)) collectDefs(fs.readFileSync(ids, 'utf8'), ids);
// 递归解析 __REF
for (let i = 0; i < 4; i++) {
  for (const k of Object.keys(idMap)) {
    const v = idMap[k];
    if (typeof v === 'string' && v.startsWith('__REF:')) {
      const ref = v.slice(6);
      idMap[k] = idMap[ref] !== undefined ? idMap[ref] : v;
    }
  }
}
function resolveRef(name) {
  if (idMap[name] && !String(idMap[name]).startsWith('__REF:')) return idMap[name];
  // 兜底手工映射（证据来源：model-config.ts / 上游实测）
  const manual = {
    FREEBUFF_MIMO_V25_MODEL_ID: 'mimo/mimo-v2.5',
    FREEBUFF_DEEPSEEK_V4_FLASH_MODEL_ID: 'deepseek/deepseek-v4-flash',
    FREEBUFF_DEEPSEEK_V4_PRO_MODEL_ID: 'deepseek/deepseek-v4-pro',
    FREEBUFF_MINIMAX_M3_MODEL_ID: 'minimax/minimax-m3',
    FREEBUFF_SOLAR_PRO_4_MODEL_ID: 'upstage/solar-pro4',
    FALLBACK_FREEBUFF_MODEL_ID: 'mimo/mimo-v2.5',
  };
  return manual[name] || null;
}

// ---------- 3) 提取模型 const 块 ----------
const rows = [];
const re = /(?:export\s+)?const\s+\w+\s*=\s*\{([\s\S]*?)\}\s*as\s+const\s+satisfies\s+FreebuffModelOption/g;
let m;
while ((m = re.exec(src)) !== null) {
  const body = m[1];
  const get = (k) => {
    const rx = new RegExp('\\b' + k + '\\s*:\\s*((?:[^,{}]|\\{[^}]*\\})*)');
    const mm = body.match(rx);
    if (!mm) return undefined;
    let v = mm[1].trim().replace(/\/\/.*$/gm, '').trim();
    if (v.startsWith("'") || v.startsWith('"')) return v.slice(1, v.length - 1);
    if (v === 'true') return true;
    if (v === 'false') return false;
    if (v.startsWith('[')) { const arr = [...v.matchAll(/'([^']+)'/g)].map((x) => x[1]); return arr.length ? arr : null; }
    return v;
  };
  const idRef = get('id');
  if (!idRef) continue;
  const id = idRef.includes('FREEBUFF_') || idRef.includes('LIMITED_') || idRef.includes('FALLBACK_')
    ? resolveRef(nameOf(idRef)) : idRef;
  if (!id) { process.stderr.write('WARN 无法解析 id: ' + idRef + '\n'); continue; }
  if (id.includes('${')) continue;
  rows.push({
    id,
    displayName: get('displayName') ?? null,
    availability: get('availability') ?? 'always',
    premium: get('premium') ?? false,
    multimodal: get('multimodal') ?? false,
    efforts: Array.isArray(get('efforts')) ? get('efforts') : (get('efforts') === null ? null : undefined),
    reasoningEffort: get('reasoningEffort') ?? null,
    defaultEffort: get('defaultEffort') ?? null,
    unavailableFallbackRaw: (() => { const v = get('unavailableFallback'); if (!v) return null; return v.includes('FREEBUFF_')||v.includes('FALLBACK_')||v.includes('LIMITED_') ? resolveRef(nameOf(v)) : v; })(),
    experimental: get('experimental') ?? false,
    catalog: true,
  });
}
function nameOf(s) { return s.trim(); }

const out = {
  _source: modelsPath,
  _vended_at: new Date().toISOString().slice(0, 10),
  _note: '由 scripts/extract_upstream_models.mjs 程序化提取；catalog=true = 上游 SUPPORTED_FREEBUFF_MODELS 合同行', 
  models: rows,
};
process.stdout.write(JSON.stringify(out, null, 2) + '\n');