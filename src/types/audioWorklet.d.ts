/**
 * AudioWorklet 글로벌 스코프 앰비언트 타입.
 *
 * `AudioWorkletProcessor` / `sampleRate` / `registerProcessor`는 메인 스레드
 * DOM 타입에 없어, worklet 파일이 tsc 타입 체크를 통과하도록 선언한다.
 * 런타임에는 브라우저가 AudioWorklet 글로벌 스코프로 실제 값을 제공한다.
 */

declare class AudioWorkletProcessor {
  readonly port: MessagePort;
  process(
    inputs: Float32Array[][],
    outputs: Float32Array[][],
    parameters: Record<string, Float32Array>,
  ): boolean | void;
}

declare const sampleRate: number;

declare function registerProcessor(
  name: string,
  processorConstructor: new () => AudioWorkletProcessor,
): void;
