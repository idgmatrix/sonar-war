/**
 * AudioWorklet 호스트 — WASM DSP 엔진을 브릿지한다.
 *
 * 역할 (docs/개발 계획.md §3):
 *  - WASM 로드 (wasm-pack 생성 JS)
 *  - 매 process() 블록에서 WASM `engine.process()` 호출
 *  - worklet 출력 → AudioContext 연결
 *
 * Web Audio API는 이 worklet 이후의 출력 경로(게인/라우팅/사후 필터)만 담당한다.
 * M0: WASM이 440Hz 정현파를 출력하는 것을 검증.
 *
 * ⚠ Chromium `WorkletGlobalScope`는 `TextDecoder`/`fetch`/`URL`/`Response`가 없다.
 *  - `workletPolyfill.ts`를 **가장 먼저** import → glue top-level `new TextDecoder`가
 *    ReferenceError로 모듈 evaluation을 abort시키지 않게 한다.
 *  - wasm 바이트는 메인 스레드가 fetch 후 port로 전달 → `init(bytes)`로 로드(fetch 불필요).
 */
import './workletPolyfill.ts'; // ← glue보다 먼저 평가되어야 함 (side-effect import)
import init, { DspEngine } from '@dsp/dsp_core.js';

class SonarProcessor extends AudioWorkletProcessor {
  private engine: DspEngine | null = null;
  private ready = false;
  private bootstrapping = false;

  constructor() {
    super();
    this.port.onmessage = (e: MessageEvent) => {
      const data = e.data;
      if (data?.type === 'init' && data.wasmBytes instanceof ArrayBuffer) {
        void this.bootstrap(data.wasmBytes);
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
      this.engine.set_sine_freq(440.0);
      this.ready = true;
      this.port.postMessage({ type: 'ready', sampleRate });
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

  process(
    _inputs: Float32Array[][],
    outputs: Float32Array[][],
    _parameters: Record<string, Float32Array>,
  ): boolean {
    const out = outputs[0];
    if (this.ready && this.engine) {
      // mono 출력 → 모든 채널에 동일 신호
      for (let ch = 0; ch < out.length; ch++) {
        this.engine.process(out[ch]);
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
