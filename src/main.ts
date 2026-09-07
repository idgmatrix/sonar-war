/** 정적 음향 비교 장면: World → Rust/WASM → 센서 표시/선택적 청취 보조. */
import { TickLoop } from './core/sim/tickLoop.ts';
import { BassDisplay, LofarDisplay } from './render/displays/sonarDisplays.ts';
import { DEMO_SCENES } from './dsp/demoScenes.ts';
import { worldFromScene, encodeWorld } from './dsp/sceneWorld.ts';
import { SonarSession } from './audio/sonarSession.ts';
import workletUrl from './dsp/worklets/sonarWorklet.ts?worker&url';
import wasmUrl from '../dsp-core/pkg/dsp_core_bg.wasm?url';

const el = (id: string) => document.getElementById(id)!;
const input = (id: string) => el(id) as HTMLInputElement;
const start = el('start') as HTMLButtonElement;
const session = new SonarSession();
const bassCanvas = el('bass-display') as HTMLCanvasElement;
const bass = new BassDisplay(bassCanvas);
const lofar = new LofarDisplay(el('lofar-display') as HTMLCanvasElement, 500);
let selectedScene = 'demo';
let world = worldFromScene(DEMO_SCENES[selectedScene]);
let running = false;
let starting = false;
let uiGeneration = 0;
let poll: ReturnType<typeof setInterval> | null = null;
let analyser: AnalyserNode | null = null;
let rawGain: GainNode | null = null;
let monitorGain: GainNode | null = null;
const loop = new TickLoop(() => {}, () => {
  el('tick').textContent = String(loop.tick);
  el('time').textContent = `${loop.time.toFixed(1)}s`;
});

function sendBeam(): void {
  const azimuth = Number(input('bearing').value);
  const elevation = Number(input('elevation').value);
  el('bearing-value').textContent = `${azimuth.toFixed(0).padStart(3, '0')}°`;
  el('elevation-value').textContent = `${elevation >= 0 ? '+' : ''}${elevation.toFixed(1)}°`;
  bass.setBearing(azimuth);
  if (running) session.node?.port.postMessage({ type: 'beam', azimuth, elevation });
}
function ruler(): void {
  const hz = Number(input('harmonic').value);
  el('harmonic-value').textContent = `${hz.toFixed(2)} Hz`;
  lofar.setFundamental(hz);
}
function selectScene(key: string): void {
  selectedScene = key;
  const scene = DEMO_SCENES[key];
  world = worldFromScene(scene);
  const message = encodeWorld(world);
  el('entities').textContent = String(message.contacts);
  el('scene').textContent = scene.label;
  if (running) session.node?.port.postMessage(message);
  input('bearing').value = String(scene.beamAzimuthDeg);
  const target = world.all().find((t) => t.id !== world.ownshipId)!;
  const ownship = world.get(world.ownshipId)!;
  const range = Math.hypot(...target.position.map((v, i) => v - ownship.position[i]));
  input('elevation').value = String(Math.asin((target.position[1] - ownship.position[1]) / range) * 180 / Math.PI);
  input('harmonic').value = String(target.bladeRate * (1 + (message.targets[7] ?? message.profiled[6] ?? 0) / 1500));
  bass.clear(); lofar.clear();
  sendBeam(); ruler();
}
function listeningMode(): void {
  const now = session.context?.currentTime ?? 0;
  const aid = input('listening-aid').checked;
  rawGain?.gain.setTargetAtTime(aid ? 0 : 0.5, now, 0.03);
  monitorGain?.gain.setTargetAtTime(aid ? 0.5 : 0, now, 0.03);
}
function clearAudioUi(): void {
  if (poll) clearInterval(poll);
  poll = null;
  analyser?.disconnect(); rawGain?.disconnect(); monitorGain?.disconnect();
  analyser = null; rawGain = null; monitorGain = null;
  running = false; starting = false;
  loop.stop();
  start.disabled = false; start.textContent = 'Start sonar';
  el('audio-status').classList.remove('ok');
  el('cpu').textContent = '—'; el('cpu').classList.remove('ok', 'warn');
}
function failed(error: unknown): void {
  clearAudioUi();
  el('audio-status').textContent = `AUDIO ERROR: ${error instanceof Error ? error.message : String(error)}`;
  start.textContent = 'Retry sonar';
}
function telemetry(data: any): void {
  if (data?.type === 'bass' && data.levels instanceof Float32Array) bass.update(data.levels);
  if (data?.type === 'cpu') {
    // Date.now 정밀도에서는 짧은 블록의 통과/실패를 판정하지 않는다.
    const fine = data.clock === 'performance';
    const pass = fine && data.p95Ms < data.budgetMs && data.overruns === 0;
    el('cpu').textContent = fine
      ? `p95 ${data.p95Ms.toFixed(2)} / ${data.budgetMs.toFixed(2)}ms · late ${data.overruns}`
      : `~${data.meanMs.toFixed(2)}ms · budget ${data.budgetMs.toFixed(2)} · coarse clock`;
    el('cpu').classList.toggle('ok', pass);
    el('cpu').classList.toggle('warn', fine && !pass);
  }
}

start.addEventListener('click', async () => {
  const generation = ++uiGeneration;
  if (running || starting) {
    clearAudioUi();
    await session.stop();
    if (generation === uiGeneration) el('audio-status').textContent = 'Stopped';
    return;
  }
  starting = true; start.textContent = 'Cancel';
  el('audio-status').textContent = 'Loading sonar…';
  try {
    if (!await session.start(workletUrl, wasmUrl, telemetry, failed) || generation !== uiGeneration) return;
    starting = false; running = true;
    const ctx = session.context!;
    const node = session.node!;
    analyser = new AnalyserNode(ctx, { fftSize: 32768, minDecibels: -110, maxDecibels: -20, smoothingTimeConstant: 0.72 });
    rawGain = new GainNode(ctx, { gain: input('listening-aid').checked ? 0 : 0.5 });
    monitorGain = new GainNode(ctx, { gain: input('listening-aid').checked ? 0.5 : 0 });
    node.connect(analyser, 0); analyser.connect(rawGain).connect(ctx.destination);
    node.connect(monitorGain, 1); monitorGain.connect(ctx.destination);
    const levels = new Float32Array(analyser.frequencyBinCount);
    const waveform = new Float32Array(analyser.fftSize);
    poll = setInterval(() => {
      if (!analyser) return;
      analyser.getFloatFrequencyData(levels); lofar.update(levels, ctx.sampleRate);
      analyser.getFloatTimeDomainData(waveform);
      const rms = Math.sqrt(waveform.reduce((sum, x) => sum + x * x, 0) / waveform.length);
      el('rms').textContent = rms > 0 ? `${(20 * Math.log10(rms)).toFixed(1)} dBFS` : '−∞';
    }, 100);
    selectScene(selectedScene);
    node.port.postMessage({ type: 'ocean', wind: 5, rain: 0 });
    el('audio-status').textContent = 'WASM DSP: RUNNING';
    el('audio-status').classList.add('ok');
    start.textContent = 'Stop sonar'; loop.start();
  } catch (error) {
    if (generation !== uiGeneration) return;
    await session.stop();
    if (generation === uiGeneration) failed(error);
  }
});
input('bearing').addEventListener('input', sendBeam);
input('elevation').addEventListener('input', sendBeam);
input('harmonic').addEventListener('input', ruler);
input('listening-aid').addEventListener('change', listeningMode);
bassCanvas.addEventListener('pointerdown', (event) => {
  input('bearing').value = bass.bearingAtClientX(event.clientX).toFixed(0); sendBeam();
});
for (const key of Object.keys(DEMO_SCENES)) el(`scene-${key}`).addEventListener('click', () => selectScene(key));
window.addEventListener('pagehide', () => { clearAudioUi(); void session.stop(); });
selectScene(selectedScene);
