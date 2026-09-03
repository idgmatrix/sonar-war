/**
 * AudioWorklet 글로벌 스코프 polyfill.
 *
 * Chromium의 `WorkletGlobalScope`는 `TextDecoder`/`TextEncoder`를 노출하지 않는다
 * (dedicated Worker와 달리 fetch/URL/TextDecoder 등 대부분의 Web API가 없다).
 *
 * wasm-pack 생성 glue(`dsp_core.js`)는 모듈 **top-level**에서
 * `new TextDecoder('utf-8', …)`를 호출한다. 이 polyfill이 glue보다 **먼저 평가**되어야
 * (worklet에서 이 모듈을 가장 먼저 import) top-level `new TextDecoder`가 ReferenceError로
 * 모듈 evaluation을 abort시키지 않는다.
 *
 * `decode()`는 실제로는 Rust panic/error 메시지(`__wbindgen_throw`)에서만 호출된다.
 * M0에서는 거의 쓰이지 않지만, top-level 초기화와 향후 에러 경로에 대비해 완전한
 * UTF-8 디코더를 제공한다.
 */

/** UTF-8 바이트열 → JS 문자열 (BOM/surrogate pair/잘못된 시퀀스 처리). */
function utf8Decode(bytes: Uint8Array): string {
  let out = '';
  let i = 0;
  const n = bytes.length;
  while (i < n) {
    const b0 = bytes[i];
    let cp: number;
    let len: number;
    if (b0 < 0x80) {
      cp = b0;
      len = 1;
    } else if (b0 >= 0xc0 && b0 < 0xe0) {
      cp = b0 & 0x1f;
      len = 2;
    } else if (b0 >= 0xe0 && b0 < 0xf0) {
      cp = b0 & 0x0f;
      len = 3;
    } else if (b0 >= 0xf0 && b0 < 0xf8) {
      cp = b0 & 0x07;
      len = 4;
    } else {
      cp = 0x3f;
      len = 1; // 잘못된 리드 바이트 → REPLACEMENT CHARACTER
    }
    for (let k = 1; k < len; k++) {
      const b = bytes[i + k];
      if (b === undefined || (b & 0xc0) !== 0x80) {
        cp = 0x3f;
        len = 1;
        break; // 잘-formed 아님
      }
      cp = (cp << 6) | (b & 0x3f);
    }
    i += len;
    if (cp > 0xffff) {
      cp -= 0x10000;
      out += String.fromCharCode(0xd800 + (cp >> 10), 0xdc00 + (cp & 0x3ff));
    } else {
      out += String.fromCharCode(cp);
    }
  }
  return out;
}

/** JS 문자열 → UTF-8 바이트열. */
function utf8Encode(s: string): Uint8Array {
  const bytes: number[] = [];
  for (let i = 0; i < s.length; i++) {
    let cp = s.charCodeAt(i);
    if (cp >= 0xd800 && cp <= 0xdbff && i + 1 < s.length) {
      const lo = s.charCodeAt(i + 1);
      if (lo >= 0xdc00 && lo <= 0xdfff) {
        cp = 0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00);
        i++;
      }
    }
    if (cp < 0x80) {
      bytes.push(cp);
    } else if (cp < 0x800) {
      bytes.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
    } else if (cp < 0x10000) {
      bytes.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
    } else {
      bytes.push(
        0xf0 | (cp >> 18),
        0x80 | ((cp >> 12) & 0x3f),
        0x80 | ((cp >> 6) & 0x3f),
        0x80 | (cp & 0x3f),
      );
    }
  }
  return new Uint8Array(bytes);
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const g = globalThis as any;

if (typeof g.TextDecoder === 'undefined') {
  g.TextDecoder = class PolyfillTextDecoder {
    constructor(_label?: string, _options?: unknown) {}
    decode(input?: ArrayBufferView | ArrayBuffer): string {
      if (input === undefined) return '';
      const bytes =
        input instanceof ArrayBuffer
          ? new Uint8Array(input)
          : new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
      return utf8Decode(bytes);
    }
  };
}

if (typeof g.TextEncoder === 'undefined') {
  g.TextEncoder = class PolyfillTextEncoder {
    encode(s: string): Uint8Array {
      return utf8Encode(s);
    }
  };
}
