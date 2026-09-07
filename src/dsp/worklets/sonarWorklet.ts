/** WASM 호스트. 블록당 1회 상태 전진, 원음/청취 보조를 별도 출력한다. */
import './workletPolyfill.ts';
import init, { DspEngine } from '@dsp/dsp_core.js';

const hasPerformance = typeof globalThis.performance?.now === 'function';
const nowMs = () => hasPerformance ? globalThis.performance.now() : Date.now();

class SonarProcessor extends AudioWorkletProcessor {
  private engine: DspEngine | null = null;
  private bootstrapping = false;
  private raw = new Float32Array(128);
  private monitor = new Float32Array(128);
  private bass = new Float32Array(72);
  private bassMs = 0;
  private windowMs = 0;
  private durations = new Float64Array(256);
  private count = 0;
  private elapsed = 0;
  private overruns = 0;

  constructor() {
    super();
    this.port.onmessage = ({ data }) => {
      if (data?.type === 'init' && data.wasmBytes instanceof ArrayBuffer) {
        void this.bootstrap(data.wasmBytes); return;
      }
      if (!this.engine) return;
      try {
        switch (data?.type) {
          case 'worldScene':
            if (data.targets instanceof Float32Array && data.profiled instanceof Float32Array)
              this.engine.set_world_scene(data.targets, data.profiled, data.receiverDepthM);
            break;
          case 'scene': this.engine.set_targets(data.targets); break;
          case 'profiledScene': this.engine.set_profiled_targets(data.targets); break;
          case 'profiledSceneV2': this.engine.set_profiled_targets_v2(data.targets); break;
          case 'ocean': this.engine.set_ocean(data.wind ?? 5, data.rain ?? 0); break;
          case 'beam': this.engine.set_beam(data.azimuth ?? 0, data.elevation ?? 0); break;
        }
      } catch (error) { this.port.postMessage({ type: 'error', message: String(error) }); }
    };
  }

  private async bootstrap(bytes: ArrayBuffer): Promise<void> {
    if (this.bootstrapping || this.engine) return;
    this.bootstrapping = true;
    try {
      await init({ module_or_path: bytes });
      this.engine = new DspEngine(sampleRate);
      // 호스트가 선택 장면을 보낼 때까지 기본 데모 표적을 비운다.
      this.engine.set_targets(new Float32Array(0));
      this.port.postMessage({ type: 'ready', sampleRate });
    } catch (error) { this.port.postMessage({ type: 'error', message: String(error) }); }
    finally { this.bootstrapping = false; }
  }

  process(_inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    const frames = outputs[0]?.[0]?.length ?? 0;
    if (frames === 0) return true;
    if (!this.engine) {
      for (const output of outputs) for (const channel of output) channel.fill(0);
      return true;
    }
    if (this.raw.length !== frames) {
      this.raw = new Float32Array(frames); this.monitor = new Float32Array(frames);
    }
    const begin = nowMs();
    this.engine.process_with_monitor(this.raw, this.monitor);
    for (const channel of outputs[0]) channel.set(this.raw);
    for (const channel of outputs[1] ?? []) channel.set(this.monitor);
    const duration = Math.max(0, nowMs() - begin);
    const blockMs = frames / sampleRate * 1000;
    this.durations[this.count++] = duration;
    this.elapsed += duration;
    this.overruns += Number(duration > blockMs);
    this.windowMs += blockMs;
    this.bassMs += blockMs;
    if (this.windowMs >= 200 || this.count === this.durations.length) {
      const sorted = this.durations.slice(0, this.count).sort();
      this.port.postMessage({ type: 'cpu', clock: hasPerformance ? 'performance' : 'date',
        meanMs: this.elapsed / this.count, p95Ms: sorted[Math.ceil(this.count * 0.95) - 1],
        maxMs: sorted[this.count - 1], budgetMs: blockMs * 0.5, blockMs, overruns: this.overruns });
      this.count = 0; this.elapsed = 0; this.windowMs = 0; this.overruns = 0;
    }
    if (this.bassMs >= 100) {
      this.engine.bass_scan(this.bass);
      this.port.postMessage({ type: 'bass', levels: this.bass });
      this.bassMs = 0;
    }
    return true;
  }
}
registerProcessor('sonar-processor', SonarProcessor);
