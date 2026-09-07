/** 오디오 세션 자원 소유권. 실패/중지 뒤에도 새 세션을 시작할 수 있다. */
export class SonarSession {
  context: AudioContext | null = null;
  node: AudioWorkletNode | null = null;
  private abort: AbortController | null = null;
  private generation = 0;
  private rejectReady: ((error: Error) => void) | null = null;
  private timeout: ReturnType<typeof setTimeout> | null = null;

  async start(moduleUrl: string, wasmUrl: string, onMessage: (message: any) => void, onFailure: (error: Error) => void): Promise<boolean> {
    if (this.context) return false;
    const generation = ++this.generation;
    const abort = new AbortController();
    this.abort = abort;
    try {
      const context = new AudioContext();
      this.context = context;
      const resume = context.resume();
      await Promise.all([resume, context.audioWorklet.addModule(moduleUrl)]);
      if (generation !== this.generation) return false;
      const node = new AudioWorkletNode(context, 'sonar-processor', { numberOfOutputs: 2, outputChannelCount: [1, 1] });
      this.node = node;
      const bytes = await fetch(wasmUrl, { cache: 'no-cache', signal: abort.signal }).then(async (r) => {
        if (!r.ok) throw new Error(`WASM fetch ${r.status}`);
        return r.arrayBuffer();
      });
      if (generation !== this.generation) return false;
      await new Promise<void>((resolve, reject) => {
        this.rejectReady = reject;
        this.timeout = setTimeout(() => reject(new Error('WASM 초기화 시간 초과')), 15000);
        let ready = false;
        const fail = (error: Error) => {
          if (generation !== this.generation) return;
          if (!ready) reject(error);
          else { void this.stop(); onFailure(error); }
        };
        node.onprocessorerror = () => fail(new Error('오디오 프로세서 오류'));
        node.port.onmessage = ({ data }) => {
          if (generation !== this.generation) return;
          if (data?.type === 'error') { fail(new Error(data.message)); return; }
          if (data?.type === 'ready') { ready = true; resolve(); }
          else if (data?.type !== 'requestInit') onMessage(data);
        };
        node.port.postMessage({ type: 'init', wasmBytes: bytes }, [bytes]);
      });
      if (this.timeout) clearTimeout(this.timeout);
      this.timeout = null; this.rejectReady = null;
      return generation === this.generation;
    } catch (error) {
      if (generation !== this.generation) return false;
      await this.stop(); throw error;
    }
  }

  async stop(): Promise<void> {
    ++this.generation;
    this.abort?.abort(); this.abort = null;
    if (this.timeout) clearTimeout(this.timeout);
    this.timeout = null;
    this.rejectReady?.(new Error('오디오 초기화 취소')); this.rejectReady = null;
    const context = this.context;
    this.context = null;
    if (this.node) {
      this.node.onprocessorerror = null;
      this.node.port.onmessage = null;
      this.node.port.close(); this.node.disconnect(); this.node = null;
    }
    if (context && context.state !== 'closed') await context.close().catch(() => {});
  }
}
