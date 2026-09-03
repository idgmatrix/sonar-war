/**
 * AudioWorklet 호스트 — WASM DSP 엔진을 브릿지한다.
 *
 * 역할 (docs/개발 계획.md §3):
 *  - WASM 로드 (wasm-pack 생성 JS)
 *  - 매 process() 블록에서 WASM `engine.process()` **1회** 호출 (상태 포함 엔진)
 *  - 모노 출력 → 모든 채널에 복사 (채널 복사는 호스트 책임)
 *  - worklet CPU 점유 측정 → 메인 스레드 보고 (M2 완료 기준: 프레임 예산 10ms의 50% 미만)
 *
 * Web Audio API는 이 worklet 이후의 출력 경로(게인/라우팅/사후 필터)만 담당한다.
 *
 * ⚠ Chromium `WorkletGlobalScope`는 `TextDecoder`/`fetch`/`URL`/`Response`가 없다.
 *  - `workletPolyfill.ts`를 **가장 먼저** import → glue top-level `new TextDecoder`가
 *    ReferenceError로 모듈 evaluation을 abort시키지 않게 한다.
 *  - wasm 바이트는 메인 스레드가 fetch 후 port로 전달 → `init(bytes)`로 로드(fetch 불필요).
 *
 * ⚠ `WorkletGlobalScope`는 `performance`도 노출하지 않을 수 있다.
 *  - CPU 측정은 `performance.now()`이 있으면 사용하고, 없으면 0을 반환해
 *    호스트가 "n/a"로 처리한다. `ready` 메시지에 `hasPerformance` 프로브를 함께 보낸다.
 */
import './workletPolyfill.ts'; // ← glue보다 먼저 평가되어야 함 (side-effect import)
import init, { DspEngine } from '@dsp/dsp_core.js';

/**
 * worklet 내 현재 시각(ms).
 * WorkletGlobalScope는 `performance`이 없지만 `Date.now()`은 있으므로 그 순서로
 * 폴백한다. 둘 다 없으면 0 (호스트가 n/a 처리).
 * 해상도: perf.now()는 µs급, Date.now()는 ~1ms — 블럭당 CPU(~0.1ms)는 1ms 해상도
 * 아래이므로 per-block 델타가 아니라 **윈도우 스팬**(상한)으로 측정한다.
 */
function nowMs(): number {
  const g = globalThis as unknown as {
    performance?: { now?: () => number };
    Date?: { now?: () => number };
  };
  if (g.performance && typeof g.performance.now === 'function') {
    return g.performance.now();
  }
  if (g.Date && typeof g.Date.now === 'function') {
    return g.Date.now();
  }
  return 0;
}

/**
 * CPU 보고 주기 (오디오 시간 기준, ms).
 * Date.now() 해상도(~1ms)를 스팬이 충분히 초과하도록 200ms로 — 윈도우 스팬이
 * 해상도보다 훨씬 길어야 per-block 상한이 의미 있다.
 */
const CPU_WINDOW_MS = 200;
/** BASS 전 방위 레벨을 UI에 보내는 간격 (오디오 시간 기준). */
const BASS_WINDOW_MS = 100;
const BASS_BINS = 72;

class SonarProcessor extends AudioWorkletProcessor {
  private engine: DspEngine | null = null;
  private ready = false;
  private bootstrapping = false;
  /** 모노 스래치 — 블럭당 1회 `engine.process`로 채운 뒤 채널로 복사. */
  private mono: Float32Array = new Float32Array(0);
  // CPU 집계 (오디오 시간 윈도우)
  private cpuAccumMs = 0;
  private audioAccumMs = 0;
  private blockCount = 0;
  private spanStart = -1; // 윈도우 첫 t0
  private spanEnd = 0; // 윈도우 마지막 t1
  private resolution: number | null = null; // worklet 타이머 해상도 (ms)
  private bassAccumMs = 0;
  private readonly bassLevels = new Float32Array(BASS_BINS);

  constructor() {
    super();
    this.port.onmessage = (e: MessageEvent) => {
      const data = e.data;
      if (!data) return;
      if (data.type === 'init' && data.wasmBytes instanceof ArrayBuffer) {
        void this.bootstrap(data.wasmBytes);
        return;
      }
      if (!this.ready || !this.engine) return;
      switch (data.type) {
        case 'scene':
          if (data.targets instanceof Float32Array) this.engine.set_targets(data.targets);
          break;
        case 'ocean':
          this.engine.set_ocean(data.wind ?? 5.0, data.rain ?? 0.0);
          break;
        case 'beam':
          this.engine.set_beam(data.azimuth ?? 0.0, data.elevation ?? 0.0);
          break;
      }
    };
    // 메인 스레드에게 wasm 바이트를 요청 (race-free handshake)
    this.port.postMessage({ type: 'requestInit' });
  }

  private async bootstrap(wasmBytes: ArrayBuffer): Promise<void> {
    if (this.bootstrapping || this.ready) return;
    this.bootstrapping = true;
    try {
      // bytes 전달 → glue의 fetch/URL/Response 경로를 모두 우회
      await init(wasmBytes);
      this.engine = new DspEngine(sampleRate);
      this.ready = true;
      // worklet 클럭 환경 프로브 — 어떤 시계(performance.now / Date.now)가
      // 실제로 호출 가능한지 확인해 CPU 측정 전략을 결정한다.
      const g = globalThis as any;
      const env = {
        perfDefined: typeof g.performance !== 'undefined',
        perfNowFn: typeof g.performance?.now === 'function',
        dateDefined: typeof g.Date !== 'undefined',
        dateNowFn: typeof g.Date?.now === 'function',
      };
      this.port.postMessage({
        type: 'ready',
        sampleRate,
        targets: this.engine.target_count(),
        hasPerformance: nowMs() !== 0, // 이제 perf.now()가 실제로 호출 가능할 때만 true
        env,
      });
    } catch (err) {
      console.error('[SonarWorklet] WASM init failed', err);
      this.port.postMessage({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      });
    } finally {
      this.bootstrapping = false;
    }
  }

  /**
   * worklet 유효 클럭 해상도(ms)를 1회 측정 — `nowMs()`(perf→Date 폴백)의 최소 양의
   * 델타. Date.now()는 ~1ms 해상도라 블럭당 CPU(~0.1ms)를 직접 재면 0으로 반올림되므로
   * 윈도우 스팬(상한) 측정이 필요함을 판단하는 진단 지표. 1회성 — 첫 process()에서만.
   */
  private measureResolution(): number {
    if (nowMs() === 0) return -1; // 어떤 클럭도 없음
    let min = Infinity;
    let last = nowMs();
    for (let i = 0; i < 20000; i++) {
      const t = nowMs();
      if (t > last) {
        const d = t - last;
        if (d < min) min = d;
        last = t;
      }
    }
    return min === Infinity ? 0 : min;
  }

  process(
    _inputs: Float32Array[][],
    outputs: Float32Array[][],
    _parameters: Record<string, Float32Array>,
  ): boolean {
    const out = outputs[0];
    const frames = out[0].length;
    if (this.ready && this.engine) {
      if (this.mono.length !== frames) this.mono = new Float32Array(frames);
      const t0 = nowMs();
      // 상태 포함 엔진 — 블럭당 정확히 1회만 호출 (채널 수만큼 호출하면 시간 축이
      // 채널 수 배로 빨라진다). 모노로 합성 후 아래에서 채널로 복사.
      this.engine.process(this.mono);
      const t1 = nowMs();
      for (let ch = 0; ch < out.length; ch++) {
        out[ch].set(this.mono);
      }
      // CPU 집계 (오디오 시간 윈도우).
      //  - cpuAccumMs: 블럭별 (t1-t0) 합 — 타이머 해상도보다 짧은 블럭은 0으로 반올림됨.
      //  - span: 윈도우 첫 t0 → 마지막 t1 — 블럭 간 갭 포함이므로 per-block CPU의 상한.
      //    perBlockSpan = span/blockCount ≥ 실제 per-block CPU (갭은 우리 CPU가 아님).
      this.cpuAccumMs += t1 - t0;
      this.audioAccumMs += (frames / sampleRate) * 1000;
      this.bassAccumMs += (frames / sampleRate) * 1000;
      this.blockCount += 1;
      if (this.spanStart < 0) this.spanStart = t0;
      this.spanEnd = t1;
      if (this.resolution === null) this.resolution = this.measureResolution();
      if (this.audioAccumMs >= CPU_WINDOW_MS) {
        const spanMs = this.spanEnd - this.spanStart;
        this.port.postMessage({
          type: 'cpu',
          cpuMs: this.cpuAccumMs, // 블럭별 델타 합 (해상도 미만이면 0)
          spanMs, // 윈도우 벽시계 스팬 (per-block CPU 상한)
          audioMs: this.audioAccumMs,
          blockCount: this.blockCount,
          perBlock: this.cpuAccumMs / this.blockCount, // 델타 기준 (해상도 영향)
          perBlockSpan: this.blockCount > 0 ? spanMs / this.blockCount : 0, // 스팬 기준 상한
          duty: this.audioAccumMs > 0 ? (spanMs / this.audioAccumMs) * 100 : 0,
          resolution: this.resolution, // worklet 타이머 해상도 (ms), -1=없음
        });
        this.cpuAccumMs = 0;
        this.audioAccumMs = 0;
        this.blockCount = 0;
        this.spanStart = -1;
      }
      if (this.bassAccumMs >= BASS_WINDOW_MS) {
        // Rust가 동일 하이드로폰 링 버퍼에서 계산한 전 방위 빔 레벨이다.
        // TS는 음향을 재합성하지 않고 시각화만 담당한다.
        this.engine.bass_scan(this.bassLevels);
        this.port.postMessage({ type: 'bass', levels: this.bassLevels });
        this.bassAccumMs = 0;
      }
    } else {
      // 초기화 중/실패: 무음
      for (let ch = 0; ch < out.length; ch++) {
        out[ch].fill(0);
      }
    }
    return true;
  }
}

registerProcessor('sonar-processor', SonarProcessor);
