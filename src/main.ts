/**
 * 진입점 — AudioContext + AudioWorklet(WASM) + 10Hz 틱 루프 연결.
 *
 * M2 완료 기준 검증 (docs/개발 계획.md):
 *  - 표적을 거리/수심별로 배치했을 때 LOFAR 토널 + 광대역이 들림
 *  - RPM 변화가 들리며, 접근/이탈 시 도플러가 들림 (씬 버튼으로 전환)
 *  - worklet CPU 점유가 프레임 예산(10ms)의 50% 미만 (CPU readout)
 *
 * ⚠ Chromium WorkletGlobalScope는 fetch/URL/performance가 없으므로:
 *  - wasm 바이트는 **메인 스레드**에서 fetch 후 AudioWorkletNode port로 transfer.
 *  - CPU 측정은 worklet이 `performance`을 가질 때만 유효 (없으면 n/a 표시).
 */
import { TickLoop } from './core/sim/tickLoop.ts';
import { createDemoWorld, World } from './core/sim/world.ts';
import workletUrl from './dsp/worklets/sonarWorklet.ts?worker&url';
import { BassDisplay, LofarDisplay } from './render/displays/sonarDisplays.ts';
import wasmUrl from '../dsp-core/pkg/dsp_core_bg.wasm?url';

const tickEl = document.getElementById('tick')!;
const timeEl = document.getElementById('time')!;
const entityEl = document.getElementById('entities')!;
const audioStatusEl = document.getElementById('audio-status')!;
const cpuEl = document.getElementById('cpu')!;
const sceneEl = document.getElementById('scene')!;
const rmsEl = document.getElementById('rms')!;
const bearingEl = document.getElementById('bearing-value')!;
const elevationEl = document.getElementById('elevation-value')!;
const harmonicEl = document.getElementById('harmonic-value')!;
const bearingInput = document.getElementById('bearing') as HTMLInputElement;
const elevationInput = document.getElementById('elevation') as HTMLInputElement;
const harmonicInput = document.getElementById('harmonic') as HTMLInputElement;
const bassCanvas = document.getElementById('bass-display') as HTMLCanvasElement;
const lofarCanvas = document.getElementById('lofar-display') as HTMLCanvasElement;

const bassDisplay = new BassDisplay(bassCanvas);
const lofarDisplay = new LofarDisplay(lofarCanvas, 500);

const world: World = createDemoWorld();

// --- 10Hz 시뮬레이션 틱 + 60fps 렌더 ---
const loop = new TickLoop(
  (ctx) => {
    // M2: 시뮬레이션 로직 없음. 이후: 자함 동역학, 타겟 스크립트, TMA.
    void ctx;
  },
  () => {
    // 렌더: HUD 갱신
    tickEl.textContent = String(loop.tick);
    timeEl.textContent = `${loop.time.toFixed(1)}s`;
    entityEl.textContent = `${world.all().length} entities`;
  },
);

// --- 씬 프리셋 (표적당 8 float: bearing, range, depth, rpm, blades, tonal_db, cavitation, rel_vel) ---
const SCENES: Record<string, { label: string; data: number[] }> = {
  demo: {
    label: 'DEMO · 3 targets',
    data: [
      45, 3000, 50, 90, 5, 150, 0.3, 5,
      300, 8000, 200, 70, 4, 145, 0.1, -3,
      0, 1500, 30, 110, 6, 155, 0.6, 0,
    ],
  },
  approach: {
    label: 'APPROACH · Doppler +',
    data: [0, 2000, 40, 120, 6, 150, 0.4, 8],
  },
  recede: {
    label: 'RECEDING · Doppler −',
    data: [0, 2000, 40, 120, 6, 150, 0.4, -8],
  },
  rpmUp: {
    label: 'RPM 180 · high',
    data: [0, 1500, 40, 180, 6, 152, 0.5, 0],
  },
  rpmDown: {
    label: 'RPM 60 · low',
    data: [0, 1500, 40, 60, 6, 152, 0.5, 0],
  },
};

let node: AudioWorkletNode | null = null;
let analyser: AnalyserNode | null = null;
let hasPerformance = false;
let cpuDiagLogged = false;
let lofarLevels = new Float32Array(0);

/**
 * 출력 RMS(0..1) — AnalyserNode 시간 영역에서.
 * M2 청취 기준(LOFAR 토널+광대역, RPM, 도플러)의 객관 검증:
 *  씬별로 RMS가 0이 아니고 서로 다르면 실제 다른 음이 생성된다는 증거.
 */
function measureRms(): number {
  if (!analyser) return 0;
  const buf = new Float32Array(analyser.fftSize);
  analyser.getFloatTimeDomainData(buf);
  let sum = 0;
  for (let i = 0; i < buf.length; i++) sum += buf[i] * buf[i];
  return Math.sqrt(sum / buf.length);
}

function rmsToDb(rms: number): string {
  return rms > 0 ? `${(20 * Math.log10(rms)).toFixed(1)} dB` : '−∞';
}

/** 씬 전환 1s 후(엔진이 새 표적으로 수렴할 시간) 씬 RMS를 로그 — 객관 검증. */
function sendScene(key: string): void {
  const scene = SCENES[key];
  if (!node || !scene) return;
  node.port.postMessage({ type: 'scene', targets: new Float32Array(scene.data) });
  sceneEl.textContent = scene.label;
  setTimeout(() => {
    console.log(`[main] scene RMS ${key} = ${rmsToDb(measureRms())}`);
  }, 1000);
}

function sendBeam(): void {
  const azimuth = Number(bearingInput.value);
  const elevation = Number(elevationInput.value);
  bearingEl.textContent = `${azimuth.toFixed(0).padStart(3, '0')}°`;
  elevationEl.textContent = `${elevation >= 0 ? '+' : ''}${elevation.toFixed(0)}°`;
  bassDisplay.setBearing(azimuth);
  node?.port.postMessage({ type: 'beam', azimuth, elevation });
}

function setHarmonicRuler(): void {
  const fundamental = Number(harmonicInput.value);
  harmonicEl.textContent = `${fundamental.toFixed(1)} Hz`;
  lofarDisplay.setFundamental(fundamental);
}

bearingInput.addEventListener('input', sendBeam);
elevationInput.addEventListener('input', sendBeam);
harmonicInput.addEventListener('input', setHarmonicRuler);
bassCanvas.addEventListener('pointerdown', (event) => {
  bearingInput.value = bassDisplay.bearingAtClientX(event.clientX).toFixed(0);
  sendBeam();
});
sendBeam();
setHarmonicRuler();

// 씬 버튼 배선
for (const key of Object.keys(SCENES)) {
  const btn = document.getElementById(`scene-${key}`) as HTMLButtonElement | null;
  if (btn) {
    btn.addEventListener('click', () => sendScene(key));
  }
}

// --- AudioContext + WASM AudioWorklet ---
async function startAudio(): Promise<void> {
  const ctx = new AudioContext();

  await ctx.audioWorklet.addModule(workletUrl);
  node = new AudioWorkletNode(ctx, 'sonar-processor');

  let wasmSent = false;
  node.port.onmessage = (e: MessageEvent) => {
    const data = e.data;
    if (data?.type === 'requestInit') {
      if (wasmSent) return;
      wasmSent = true;
      // no-cache: wasm 파일명이 고정이라 HTTP 캐시에 구 빌드가 남아 있으면
      // 물리 수정이 반영되지 않음 (조건부 요청으로 항상 재검증).
      fetch(wasmUrl, { cache: 'no-cache' })
        .then((r) => {
          if (!r.ok) throw new Error(`wasm fetch ${r.status}`);
          return r.arrayBuffer();
        })
        .then((bytes) => {
          // transfer: 메인 스레드에서 바이트를 worklet으로 zero-copy 이동
          node!.port.postMessage({ type: 'init', wasmBytes: bytes }, [bytes]);
        })
        .catch((err) => {
          audioStatusEl.textContent = `WASM LOAD ERROR: ${err.message || err}`;
          console.error('[main] wasm fetch failed', err);
        });
    } else if (data?.type === 'ready') {
      hasPerformance = !!data.hasPerformance;
      console.log('[main] worklet clock env', data.env);
      audioStatusEl.textContent = `WASM DSP: RUNNING (${data.targets} targets)`;
      audioStatusEl.classList.add('ok');
      // 엔진 기본 데모 씬 + 해양/빔 설정을 명시적으로 전송 (port 경로 검증)
      sendScene('demo');
      node!.port.postMessage({ type: 'ocean', wind: 5, rain: 0 });
      bearingInput.value = '45';
      sendBeam();
      if (!hasPerformance) {
        cpuEl.textContent = 'n/a (no clock)';
      }
    } else if (data?.type === 'cpu') {
      // CPU readout. 완료 기준: per-block CPU < 5ms (10ms 프레임 예산의 50%).
      // worklet 클럭은 Date.now()(~1ms) — 블럭당 CPU(~<1ms)는 1ms 해상도 아래라
      // per-block 델타(실제 process() 소요)는 양자화된 추정치, 스팬(블럭 간 갭 포함)은
      // 보수적 **상한**. 표시는 델타(실제 CPU), pass/fail은 상한으로 (상한 < 5ms이면
      // 실제 CPU도 반드시 < 5ms). RT: 오디오 스레드가 실시간을 유지하는지 (스팬 ≤ 오디오).
      if (data.resolution < 0) {
        cpuEl.textContent = 'n/a (no clock)';
        return;
      }
      const cpu = data.perBlock; // 실제 process() 소요 (1ms 클럭 양자화)
      const upper = data.perBlockSpan; // 블럭 간 갭 포함 상한
      // Date.now 기반 벽시계 스팬에는 스케줄러 지터가 섞이므로 10% 여유를 둔다.
      // 실제 underrun 검출기는 아니며, 장기적으로 AudioWorklet glitch telemetry로 교체한다.
      const rt = data.spanMs <= data.audioMs * 1.1;
      const pass = upper < 5;
      cpuEl.textContent = `${cpu.toFixed(2)}ms/bl · ≤${upper.toFixed(1)} · RT${rt ? '✓' : '✗'}`;
      cpuEl.classList.toggle('warn', !pass);
      cpuEl.classList.toggle('ok', pass);
      if (!cpuDiagLogged) {
        cpuDiagLogged = true;
        console.log('[main] CPU diag', {
          perBlockDelta: data.perBlock,
          perBlockSpan: data.perBlockSpan,
          spanMs: data.spanMs,
          audioMs: data.audioMs,
          blockCount: data.blockCount,
          resolution: data.resolution,
        });
      }
    } else if (data?.type === 'bass' && data.levels instanceof Float32Array) {
      bassDisplay.update(data.levels);
    } else if (data?.type === 'error') {
      audioStatusEl.textContent = `WASM ERROR: ${data.message}`;
    }
  };

  // node → analyser → gain → destination
  // AnalyserNode는 출력 경로의 사후 측정용 (사용자 기준 Web Audio의 보조 역할).
  analyser = new AnalyserNode(ctx, {
    fftSize: 32768,
    minDecibels: -110,
    maxDecibels: -20,
    smoothingTimeConstant: 0.72,
  });
  lofarLevels = new Float32Array(analyser.frequencyBinCount);
  const gain = new GainNode(ctx);
  gain.gain.value = 0.5;
  node.connect(analyser).connect(gain).connect(ctx.destination);

  // 출력 레벨 폴러 — HUD 갱신 + 무음/씬 변화 객관 검증
  let visualFrame = 0;
  setInterval(() => {
    if (!analyser) return;
    analyser.getFloatFrequencyData(lofarLevels);
    lofarDisplay.update(lofarLevels, ctx.sampleRate);
    visualFrame += 1;
    if (visualFrame % 4 === 0) rmsEl.textContent = rmsToDb(measureRms());
  }, 100);

  if (ctx.state === 'suspended') {
    await ctx.resume();
  }
  audioStatusEl.textContent = 'DSP LOADED (loading WASM…)';
}

// 브라우저 자동재생 정책: 첫 사용자 제스처에 오디오 시작.
// 틱 루프는 오디오와 독립 — 오디오 실패해도 시계는 뜀.
document.getElementById('start')!.addEventListener('click', async () => {
  loop.start();
  (document.getElementById('start') as HTMLButtonElement).disabled = true;
  startAudio().catch((err) => {
    audioStatusEl.textContent = `AUDIO ERROR: ${err.message || err}`;
    console.error('[main] audio init failed', err);
  });
});
