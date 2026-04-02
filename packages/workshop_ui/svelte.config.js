import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    adapter: adapter({
      // SPA mode: single fallback page for all routes
      fallback: 'index.html',
      strict: false
    }),
    paths: {
      base: ''
    },
    prerender: {
      // Explicitly prerender root for SPA
      entries: ['*'],
      handleUnseenRoutes: 'ignore'
    }
  }
};

export default config;
