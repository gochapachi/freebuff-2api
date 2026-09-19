// 合成最终 fixture：从 extract 输出 + 网关静态表补全 efforts + 手工补漏行
import fs from 'node:fs';
const draft = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const outFile = process.argv[3];

const EFF = {
  glm: ['low','high','max'],
  full: ['low','medium','high','xhigh','max'],
  muse: ['minimal','low','medium','high','xhigh'],
};
const effortsById = {
  'z-ai/glm-5.3-flash': EFF.glm,
  'deepseek/deepseek-v4-flash': EFF.glm,
  'deepseek/deepseek-v4-flash-max': EFF.glm,
  'deepseek/deepseek-v4-pro': EFF.glm,
  'deepseek/deepseek-v4-pro-max': EFF.glm,
  'openai/gpt-5.6-luna': EFF.full,
  'openai/gpt-5.6-luna-es': EFF.full,
  'openai/gpt-5.6-luna-max': EFF.full,
  'google/gemini-3.8-flash': EFF.full,
  'google/gemini-3.1-flash-lite': EFF.full,
  'google/gemini-3.5-flash-lite': EFF.full,
  'meta/muse-spark-1.2-contributor': EFF.muse,
  'meta/muse-spark-1.3-contributor': EFF.muse,
  'anthropic/claude-fable-5': EFF.full,
  'stealth/ox-alpha': EFF.glm,
};

const rows = [];
for (const r of draft.models) {
  rows.push({
    id: r.id,
    displayName: r.displayName,
    availability: r.availability,
    premium: r.id === 'upstage/solar-pro4' ? true : r.premium,
    multimodal: r.multimodal,
    efforts: effortsById[r.id] ?? null,
    reasoningEffort: r.reasoningEffort,
    defaultEffort: r.defaultEffort,
    unavailableFallback: r.unavailableFallbackRaw || null,
    experimental: !!r.experimental,
    catalog: true,
  });
}
// 手工补漏行（多行常量解析失败，证据：freebuff-models.ts L456-459 / L502-506 / L549-553）
const manualCatalog = [
  { id: 'deepseek/deepseek-v4-pro-max', availability: 'always', premium: true, multimodal: false, efforts: EFF.glm, fallback: null },
  { id: 'deepseek/deepseek-v4-flash-max', availability: 'always', premium: true, multimodal: false, efforts: EFF.glm, fallback: null },
  { id: 'meta/muse-spark-1.2-contributor', availability: 'always', premium: true, multimodal: false, efforts: EFF.muse, fallback: 'deepseek/deepseek-v4-flash' },
  { id: 'meta/muse-spark-1.3-contributor', availability: 'always', premium: true, multimodal: false, efforts: EFF.muse, fallback: 'deepseek/deepseek-v4-flash' },
];
for (const r of manualCatalog) {
  if (!rows.some(x => x.id === r.id)) rows.push({ id: r.id, displayName: null, availability: r.availability, premium: r.premium, multimodal: r.multimodal, efforts: r.efforts, reasoningEffort: null, defaultEffort: null, unavailableFallback: r.fallback, experimental: false, catalog: true });
}
// 网关自管/registry 行（不属于上游 SUPPORTED_FREEBUFF_MODELS 合同）
const agentRows = [
  { id: 'google/gemini-3.1-flash-lite', availability: 'always', premium: false, multimodal: true, efforts: EFF.full, note: 'free-agents 历史 file-picker/root 行；上游当前 file-picker 已切换 gemini-2.5-flash-lite' },
  { id: 'google/gemini-3.5-flash-lite', availability: 'always', premium: false, multimodal: true, efforts: EFF.full, note: '网关 root 免费行；上游目录暂无同 id' },
  { id: 'google/gemini-2.5-flash-lite', availability: 'always', premium: false, multimodal: true, efforts: EFF.full, note: '上游 free-agents.ts file-picker 当前模型；网关动态 registry 行，无静态 meta → "未经策略验证"' },
];
for (const r of agentRows) {
  rows.push({ id: r.id, displayName: null, availability: r.availability, premium: r.premium, multimodal: r.multimodal, efforts: r.efforts, reasoningEffort: null, defaultEffort: null, unavailableFallback: null, experimental: false, catalog: false, note: r.note });
}

const out = {
  _source: draft._source,
  _vended_at: draft._vended_at,
  _note: '脚本生成：scripts/extract_upstream_models.mjs + scripts/compose_fixture.mjs；catalog=true=上游合同行（与 src/models.rs MODEL_META_ROWS 做漂移比对），catalog=false=网关自管/registry 行（仅信息）。',
  models: rows,
};
// 排序：catalog true 先行，按 id
rows.sort((a, b) => (a.catalog === b.catalog ? a.id.localeCompare(b.id) : a.catalog ? -1 : 1));
fs.writeFileSync(outFile, JSON.stringify(out, null, 2) + '\n', 'utf8');
console.log('fixture rows=' + rows.length + ' -> ' + outFile);