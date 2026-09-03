/**
 * 진입점 — AudioContext + AudioWorklet(WASM) + 10Hz 틱 루프 연결.
 *
 * M0 완료 기준 검증:
 *  - 빈 화면에 시계(틱 카운터)가 10Hz로 뜀
 *  - 엔티티 직렬화 왕복 테스트 통과 (tests/acousticEntity.test.ts)
 *  - WASM 스텁이 AudioWorklet에서 호출되어 440Hz 정현파가 출력됨
 *
 * ⚠ Chromium WorkletGlobalScope는 fetch/URL이 없으므로, wasm 바이트는 **메인 스레드**에서
 * fetch 한 뒤 AudioWorkletNode port로 transfer하여 worklet이 `init(bytes)`로 로드한다.
 */
import { TickLoop } from './core/sim/tickLoop.ts';
import { createDemoWorld, World } from './core/sim/world.ts';
import workletUrl from './dsp/worklets/sonarWorklet.ts?worker&url';
import wasmUrl from '../dsp-core/pkg/dsp_core_bg.wasm?url';

const tickEl = document.getElementById('tick')!;
const timeEl = document.getElementById('time')!;
const entityEl = document.getElementById('entities')!;
const audioStatusEl = document.getElementById('audio-status')!;

const world: World = createDemoWorld();

// --- 10Hz 시뮬레이션 틱 + 60fps 렌더 ---
const loop = new TickLoop(
  (ctx) => {
    // M0: 시뮬레이션 로직 없음. 이후: 자함 동역학, 타겟 스크립트, TMA.
    void ctx;
  },
  () => {
    // 렌더: HUD 갱신
    tickEl.textContent = String(loop.tick);
    timeEl.textContent = `${loop.time.toFixed(1)}s`;
    entityEl.textContent = `${world.all().length} entities`;
  },
);

// --- AudioContext + WASM AudioWorklet ---
async function startAudio(): Promise<void> {
  const ctx = new AudioContext();

  await ctx.audioWorklet.addModule(workletUrl);
  const node = new AudioWorkletNode(ctx, 'sonar-processor');

  let wasmSent = false;
  node.port.onmessage = (e: MessageEvent) => {
    const data = e.data;
    if (data?.type === 'requestInit') {
      if (wasmSent) return;
      wasmSent = true;
      fetch(wasmUrl)
        .then((r) => {
          if (!r.ok) throw new Error(`wasm fetch ${r.status}`);
          return r.arrayBuffer();
        })
        .then((bytes) => {
          // transfer: 메인 스레드에서 바이트를 worklet으로 zero-copy 이동
          node.port.postMessage({ type: 'init', wasmBytes: bytes }, [bytes]);
        })
        .catch((err) => {
          audioStatusEl.textContent = `WASM LOAD ERROR: ${err.message || err}`;
          console.error('[main] wasm fetch failed', err);
        });
    } else if (data?.type === 'ready') {
      audioStatusEl.textContent = 'WASM DSP: RUNNING (440Hz sine)';
      audioStatusEl.classList.add('ok');
    } else if (data?.type === 'error') {
      audioStatusEl.textContent = `WASM ERROR: ${data.message}`;
    }
  };

  const gain = new GainNode(ctx);
  gain.gain.value = 0.5;
  node.connect(gain).connect(ctx.destination);

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
