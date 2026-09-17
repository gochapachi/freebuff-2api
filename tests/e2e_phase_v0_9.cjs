#!/usr/bin/env node
/**
 * v0.9 冒烟脚本 —— 覆盖本次新增能力：
 *  1. web Cookie 凭证池：/api/accounts/health 含 bearer + web-cookie，每条带 history 时间线
 *  2. 全配置导出/导入：schema 校验（坏版本 400）、round-trip 200、导入前备份目录
 *  3. /v1/models 元数据契约：meta 数组含 available/efforts/multimodal/fallback；已暂停模型标记不可用
 *  4. 鉴权纵深：/api/doctor 含 listen_scope 检查
 *  5. 面板 v0.9 控件：测试台多轮/图片/effort + 健康看板 + 今日推荐 + 数据迁移
 *
 * 前置：网关已启动（默认 47871），配置含一个 web Cookie 凭证（web-cookie kind 才会出现）。
 * 用法：node tests/e2e_phase_v0_9.cjs [port] [config.json]
 */
const http = require('node:http');
const PORT = parseInt(process.argv[2] || '47871', 10);
const HOST = '127.0.0.1';
const CFG = process.argv[3] || 'config.json';

let passed = 0, failed = 0;
const results = [];
function ok(name, cond, detail) {
  if (cond) { passed++; results.push(`  ✅ ${name}`); }
  else { failed++; results.push(`  ❌ ${name}${detail ? ' — ' + String(detail).slice(0, 300) : ''}`); }
}
function req(method, path, { body, headers } = {}) {
  return new Promise((resolve, reject) => {
    const data = body == null ? null : (typeof body === 'string' ? body : JSON.stringify(body));
    const h = Object.assign({ 'content-type': 'application/json' }, headers || {});
    if (data != null) h['content-length'] = Buffer.byteLength(data);
    const r = http.request({ host: HOST, port: PORT, path, method, headers: h }, (res) => {
      const chunks = [];
      res.on('data', c => chunks.push(c));
      res.on('end', () => {
        const buf = Buffer.concat(chunks);
        resolve({ status: res.statusCode, headers: res.headers, text: buf.toString('utf8'), json: (() => { try { return JSON.parse(buf.toString('utf8')); } catch (_) { return null; } })() });
      });
    });
    r.on('error', reject);
    r.setTimeout(30000, () => r.destroy(new Error('timeout')));
    if (data != null) r.write(data);
    r.end();
  });
}
async function json(method, p, body) { return req(method, p, { body }); }

(async () => {
  console.log(`\n=== Freebuff2API v0.9 冒烟（端口 ${PORT}） ===\n`);

  const hz = await json('GET', '/healthz');
  ok('网关存活 healthz 200', hz.status === 200);

  // 1) 凭证健康看板
  const health = await json('GET', '/api/accounts/health');
  ok('GET /api/accounts/health 200', health.status === 200, health.text);
  const hAccs = health.json && health.json.accounts || [];
  ok('health 含账号数组', Array.isArray(hAccs) && hAccs.length >= 1);
  ok('health 含 web-cookie 凭证', hAccs.some(a => a.kind === 'web-cookie'), 'kinds=' + hAccs.map(a => a.kind).join(','));
  ok('health 每条带时间线 history', hAccs.every(a => Array.isArray(a.history)));
  ok('health 字段齐全', hAccs.every(a => 'circuit_state' in a && 'health_score' in a && 'trips' in a && 'last_error' in a && 'cooldown_until' in a));

  // 2) 导出/导入
  const exp = await json('POST', '/api/export');
  ok('POST /api/export 200', exp.status === 200, exp.text);
  const data = exp.json && exp.json.data || {};
  ok('export schema_version 存在', typeof data.schema_version === 'string' && data.schema_version.length > 0);
  ok('export skills 数组', Array.isArray(data.skills));
  ok('export 不含 api_keys 明文', !JSON.stringify(data.config || {}).includes('api_keys'));
  ok('export 不含 auth_tokens 明文', !JSON.stringify(data).includes('auth_tokens'));
  const bad = Object.assign({}, data, { schema_version: '99' });
  const badImp = await json('POST', '/api/import', { data: bad });
  ok('import 坏 schema 400', badImp.status === 400, badImp.text);
  const imp = await json('POST', '/api/import', { data });
  ok('import round-trip 200', imp.status === 200, imp.text);
  ok('import 有备份目录', !!(imp.json && imp.json.imported && imp.json.imported.backed_up_to));

  // 3) /v1/models 元数据
  const models = await json('GET', '/v1/models');
  ok('GET /v1/models 200', models.status === 200);
  const meta = models.json && models.json.meta || [];
  ok('meta 数组 ≥ 19', meta.length >= 19, 'len=' + meta.length);
  ok('meta 字段齐全', meta.every(m => 'id' in m && 'available' in m && 'efforts' in m && 'multimodal' in m && 'premium' in m));
  const paused = meta.filter(m => m.available === false).map(m => m.id);
  ok('已暂停模型标记不可用', paused.includes('stealth/ox-alpha') && paused.includes('google/gemini-3.8-flash') && paused.includes('deepseek/deepseek-v4-pro'), paused.join(','));
  ok('默认模型可用', meta.some(m => m.id === 'z-ai/glm-5.3-flash' && m.available === true));

  // 4) 鉴权纵深 doctor
  const doctor = await json('GET', '/api/doctor');
  ok('doctor 含 listen_scope 检查', doctor.status === 200 && /listen_scope/.test(doctor.text));

  // 5) 面板 v0.9 控件
  const ui = await req('GET', '/ui');
  const html = ui.text || '';
  for (const [name, marker] of [
    ['测试台多轮/effort 控件', 'play-effort'],
    ['测试台图片上传区', 'play-drop'],
    ['凭证健康看板容器', 'health-wrap'],
    ['今日推荐容器', 'recommend-panel'],
    ['数据迁移（导出/导入）', 'migrate-status'],
    ['图片选择输入', 'play-file'],
  ]) {
    ok(`面板含 ${name}`, html.includes(marker));
  }

  console.log(results.join('\n'));
  console.log(`\n结果：${passed} 通过 / ${failed} 失败\n`);
  process.exit(failed ? 1 : 0);
})().catch(e => { console.error('E2E 异常：', e); process.exit(1); });
