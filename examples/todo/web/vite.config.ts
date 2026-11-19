import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, loadEnv } from 'vite';
import { trustedDevOrigin } from './vite-proxy-security';

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
      allowedHosts: ['127.0.0.1'],
      cors: false,
      proxy: target
        ? {
            '/api': {
              target: target.origin,
              changeOrigin: true,
              configure(proxy) {
                // Only rewrite a real same-origin Vite request. Forward
                // Sec-Fetch-Site unchanged so the Rust security gate can reject
                // cross-site reads, navigation and mutations.
                proxy.on('proxyReq', (outgoing, incoming) => {
                  outgoing.setHeader('Host', target.host);
                  outgoing.setHeader(
                    'Origin',
                    trustedDevOrigin(incoming.headers) ? target.origin : 'null'
                  );
                });
              }
            }
          }
        : undefined
    }
  };
});
