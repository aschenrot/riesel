import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, loadEnv } from 'vite';

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), 'TODO_');
  const configured = env.TODO_API_ORIGIN;
  const target = configured ? new URL(configured) : null;

  if (target && (target.protocol !== 'http:' || target.hostname !== '127.0.0.1')) {
    throw new Error('TODO_API_ORIGIN must be a local http://127.0.0.1:PORT authority');
  }

  return {
    plugins: [sveltekit()],
    server: {
      host: '127.0.0.1',
      proxy: target
        ? {
            '/api': {
              target: target.origin,
              changeOrigin: true,
              configure(proxy) {
                // Dev-only same-origin bridge. Production serves assets and API
                // on the *same* Rust authority; no cross-origin bypass there.
                proxy.on('proxyReq', (outgoing) => {
                  outgoing.setHeader('Host', target.host);
                  outgoing.setHeader('Origin', target.origin);
                  outgoing.setHeader('Sec-Fetch-Site', 'same-origin');
                });
              }
            }
          }
        : undefined
    }
  };
});
