import { defineConfig } from 'vite';
import { fileURLToPath, URL } from 'node:url';

// AudioWorklet 파일은 Vite가 worklet 스크립트로 취급해야 한다.
// `?worker&url` 또는 `new URL(..., import.meta.url)` 방식으로 참조하면
// Vite가 별도 번들링해 worklet 로더가 절대 경로로 로드할 수 있다.
export default defineConfig(({ mode }) => ({
  // GitHub Pages 프로젝트 사이트는 저장소 이름 아래에서 서비스된다.
  // 로컬 개발은 기존처럼 `/`, `npm run build:pages`만 `/sonar-war/`를 사용한다.
  base: mode === 'pages' ? '/sonar-war/' : '/',
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
      '@dsp': fileURLToPath(new URL('./dsp-core/pkg', import.meta.url)),
    },
  },
  worker: {
    format: 'es',
  },
  build: {
    target: 'es2022',
  },
}));
