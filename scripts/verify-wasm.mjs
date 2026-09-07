// 실제 UI 프리셋을 동일 월드 어댑터와 배포용 WASM으로 렌더한다.
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import init, { DspEngine } from '../dsp-core/pkg/dsp_core.js';
import { DEMO_SCENES } from '../src/dsp/demoScenes.ts';
import { worldFromScene, encodeWorld } from '../src/dsp/sceneWorld.ts';

await init({ module_or_path: readFileSync(new URL('../dsp-core/pkg/dsp_core_bg.wasm', import.meta.url)) });
const writeAudio = process.argv.includes('--write-audio');
if (writeAudio) mkdirSync('artifacts/offline/presets', { recursive: true });
function wav(path, samples, fs) {
  const b = Buffer.alloc(44 + samples.length * 4);
  b.write('RIFF', 0); b.writeUInt32LE(b.length - 8, 4); b.write('WAVEfmt ', 8);
  b.writeUInt32LE(16, 16); b.writeUInt16LE(3, 20); b.writeUInt16LE(1, 22);
  b.writeUInt32LE(fs, 24); b.writeUInt32LE(fs * 4, 28); b.writeUInt16LE(4, 32); b.writeUInt16LE(32, 34);
  b.write('data', 36); b.writeUInt32LE(samples.length * 4, 40);
  samples.forEach((x, i) => b.writeFloatLE(x, 44 + i * 4)); writeFileSync(path, b);
}
function stats(samples) {
  let sum = 0, peak = 0, crossings = 0;
  samples.forEach((x, i) => {
    assert(Number.isFinite(x)); sum += x * x; peak = Math.max(peak, Math.abs(x));
    if (i && (x < 0) !== (samples[i - 1] < 0)) crossings++;
  });
  return { rms: Math.sqrt(sum / samples.length), peak, crossings };
}
function peakNear(samples, fs, expected) {
  let best = { frequency: 0, power: -1 };
  const windowed = Float64Array.from(samples, (x, i) => x * (0.5 - 0.5 * Math.cos(2 * Math.PI * i / (samples.length - 1))));
  for (let step = -20; step <= 20; step++) {
    const frequency = expected + step * 0.05;
    const coefficient = 2 * Math.cos(2 * Math.PI * frequency / fs);
    let s1 = 0, s2 = 0;
    for (const x of windowed) { const s = x + coefficient * s1 - s2; s2 = s1; s1 = s; }
    const power = s1 * s1 + s2 * s2 - coefficient * s1 * s2;
    if (power > best.power) best = { frequency, power };
  }
  return best.frequency;
}
const summary = [];
for (const fs of [44100, 48000]) {
  for (const [key, scene] of Object.entries(DEMO_SCENES)) {
    const world = worldFromScene(scene);
    const message = encodeWorld(world);
    const engine = new DspEngine(fs);
    engine.set_world_scene(message.targets, message.profiled, message.receiverDepthM);
    assert.equal(engine.target_count(), message.contacts);
    const target = world.all().find(t => t.id !== world.ownshipId);
    const dy = target.position[1] - message.receiverDepthM;
    const range = message.targets[1] ?? message.profiled[1];
    engine.set_beam(scene.beamAzimuthDeg, Math.asin(dy / range) * 180 / Math.PI);
    const raw = new Float32Array(fs * 4), monitor = new Float32Array(raw.length);
    engine.process(new Float32Array(fs / 2));
    const block = new Float32Array(128), listen = new Float32Array(128);
    const durations = [];
    for (let offset = 0; offset < raw.length; offset += 128) {
      const begin = performance.now(); engine.process_with_monitor(block, listen); durations.push(performance.now() - begin);
      raw.set(block.subarray(0, Math.min(128, raw.length - offset)), offset);
      monitor.set(listen.subarray(0, Math.min(128, raw.length - offset)), offset);
    }
    durations.sort((a, b) => a - b);
    const metrics = stats(raw), aided = stats(monitor);
    const repeat = new DspEngine(fs);
    repeat.set_world_scene(message.targets, message.profiled, message.receiverDepthM);
    repeat.set_beam(scene.beamAzimuthDeg, Math.asin(dy / range) * 180 / Math.PI);
    repeat.process(new Float32Array(fs / 2));
    const repeated = new Float32Array(4096);
    repeat.process(repeated);
    assert.deepEqual(repeated, raw.slice(0, repeated.length), `${key}: deterministic raw output`);
    repeat.free();
    assert(metrics.peak < 0.99, `${key}: output clipping`);
    assert(aided.peak < 0.99, `${key}: monitor clipping`);
    const row = { fs, key, ...metrics, monitorRms: aided.rms, p95Ms: durations[Math.floor(durations.length * .95)], budgetMs: 128 / fs * 500 };
    if (['approach', 'recede', 'rpmUp', 'rpmDown'].includes(key)) {
      const fundamental = target.bladeRate * (1 + message.targets[7] / 1500);
      row.toneHz = peakNear(raw, fs, fundamental);
      row.monitorUpperHz = peakNear(monitor, fs, 240 + fundamental);
      assert(Math.abs(row.toneHz - fundamental) <= .25, `${key}: raw tone ${row.toneHz}, expected ${fundamental}`);
      assert(Math.abs(row.monitorUpperHz - (240 + fundamental)) <= .25, `${key}: monitor sideband`);
    }
    if (writeAudio) { wav(`artifacts/offline/presets/${key}-${fs}-raw.wav`, raw, fs); wav(`artifacts/offline/presets/${key}-${fs}-monitor.wav`, monitor, fs); }
    summary.push(row); console.log(JSON.stringify(row)); engine.free();
  }
}
// 기존 네이티브 골든과 동일 장면: 비트 폭에 따른 링 인덱스 차이를 검출한다.
const fields = readFileSync('data/acoustics/golden/dsp_reference_scene.csv', 'utf8').trim().split('\n')[1].split(',').map(Number);
const reference = new DspEngine(fields[0]);
reference.set_targets(new Float32Array([0, 1000, 30, 120, 5, 155, .4, 5]));
reference.set_beam(0, 0); reference.process(new Float32Array(fields[0] * fields[1]));
const samples = new Float32Array(fields[0] * fields[2]); reference.process(samples);
const result = stats(samples);
assert(Math.abs(result.rms - fields[3]) <= fields[4]);
assert(Math.abs(result.peak - fields[5]) <= fields[6]);
assert(Math.abs(result.crossings - fields[7]) <= fields[8]); reference.free();
console.log('WASM/native golden agreement:', result);
if (writeAudio) writeFileSync('artifacts/offline/presets/summary.json', JSON.stringify(summary, null, 2));
