import { afterEach, describe, expect, it, vi } from 'vitest';
import { SonarSession } from '../src/audio/sonarSession.ts';

afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });
function setup(ready = true) {
  const contexts: any[] = [], nodes: any[] = [];
  vi.stubGlobal('AudioContext', class {
    state = 'running';
    resume = vi.fn(async () => {});
    close = vi.fn(async () => { this.state = 'closed'; });
    audioWorklet = { addModule: vi.fn(async () => {}) };
    constructor() { contexts.push(this); }
  });
  vi.stubGlobal('AudioWorkletNode', class {
    disconnect = vi.fn();
    onprocessorerror: (() => void) | null = null;
    port = {
      onmessage: null as ((e: any) => void) | null,
      close: vi.fn(),
      postMessage: vi.fn(() => { if (ready) queueMicrotask(() => this.port.onmessage?.({ data: { type: 'ready' } })); }),
    };
    constructor() { nodes.push(this); }
  });
  const fetch = vi.fn(async () => ({ ok: true, arrayBuffer: async () => new ArrayBuffer(8) }));
  vi.stubGlobal('fetch', fetch);
  return { contexts, nodes, fetch };
}
describe('오디오 세션 복구', () => {
  it('WASM 요청 실패 시 정리하고 새 세션으로 재시도한다', async () => {
    const { contexts, nodes, fetch } = setup();
    fetch.mockRejectedValueOnce(new Error('offline'));
    const session = new SonarSession();
    await expect(session.start('/module', '/wasm', vi.fn(), vi.fn())).rejects.toThrow('offline');
    expect(contexts[0].close).toHaveBeenCalledOnce();
    expect(nodes[0].port.close).toHaveBeenCalledOnce();
    expect(session.context).toBeNull();
    expect(await session.start('/module', '/wasm', vi.fn(), vi.fn())).toBe(true);
    await session.stop();
    expect(contexts[1].close).toHaveBeenCalledOnce();
  });
  it('ready 대기 중 취소하면 시작 성공으로 처리하지 않는다', async () => {
    const { nodes } = setup(false);
    const session = new SonarSession();
    const pending = session.start('/module', '/wasm', vi.fn(), vi.fn());
    await vi.waitFor(() => expect(nodes[0]?.port.postMessage).toHaveBeenCalled());
    await session.stop();
    expect(await pending).toBe(false);
    expect(session.node).toBeNull();
  });
  it('ready 시간 초과 뒤 자원을 정리한다', async () => {
    vi.useFakeTimers(); setup(false);
    const session = new SonarSession();
    const result = session.start('/module', '/wasm', vi.fn(), vi.fn());
    const rejected = expect(result).rejects.toThrow('시간 초과');
    await vi.advanceTimersByTimeAsync(15001);
    await rejected;
    expect(session.context).toBeNull();
  });
  it('실행 중 processorerror도 세션을 종료하고 알린다', async () => {
    const { nodes } = setup(); const failure = vi.fn();
    const session = new SonarSession();
    await session.start('/module', '/wasm', vi.fn(), failure);
    nodes[0].onprocessorerror();
    expect(failure).toHaveBeenCalledOnce();
    expect(session.context).toBeNull();
  });
});
