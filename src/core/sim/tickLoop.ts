/**
 * 고정 시간 스텝 시뮬레이션 루프.
 *
 * - 시뮬레이션: 10Hz 고정 틱 (물리 결정론성, 네트워크 동기화 기준)
 * - 렌더링: requestAnimationFrame (60fps) — 틱 사이 상태는 보간
 *
 * accumulator 패턴으로 렌더 프레임과 시뮬레이션 스텝을 분리한다.
 */

export const SIM_HZ = 10;
export const SIM_DT = 1 / SIM_HZ; // 0.1s

export interface TickContext {
  /** 시뮬레이션 시간 (s) */
  time: number;
  /** 이번 틱의 고정 dt (s) */
  dt: number;
  /** 현재 틱 번호 */
  tick: number;
}

export type TickFn = (ctx: TickContext) => void;
export type RenderFn = (interpolation: number) => void;

export class TickLoop {
  private accumulator = 0;
  private lastTime = 0;
  private running = false;
  private rafId = 0;
  time = 0;
  tick = 0;

  constructor(private onTick: TickFn, private onRender: RenderFn) {}

  start(): void {
    if (this.running) return;
    this.running = true;
    this.lastTime = performance.now();
    const frame = (now: number) => {
      if (!this.running) return;
      let frameTime = (now - this.lastTime) / 1000;
      this.lastTime = now;
      // 스피크 방지: 한 프레임에 최대 0.25s만 누적 (탭이 백그라운드에서 떠있을 때)
      if (frameTime > 0.25) frameTime = 0.25;
      this.accumulator += frameTime;

      while (this.accumulator >= SIM_DT) {
        this.accumulator -= SIM_DT;
        this.time += SIM_DT;
        this.tick += 1;
        this.onTick({ time: this.time, dt: SIM_DT, tick: this.tick });
      }

      const interpolation = this.accumulator / SIM_DT;
      this.onRender(interpolation);
      this.rafId = requestAnimationFrame(frame);
    };
    this.rafId = requestAnimationFrame(frame);
  }

  stop(): void {
    this.running = false;
    cancelAnimationFrame(this.rafId);
  }
}
