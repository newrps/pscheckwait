import adapter from "@sveltejs/adapter-static";

/** @type {import('@sveltejs/kit').Config} */
export default {
  kit: {
    adapter: adapter({
      pages: "build",
      assets: "build",
      fallback: "index.html",
      precompress: false,
      strict: true,
    }),
    paths: {
      base: "/admin",
      relative: false,
    },
    prerender: {
      handleHttpError: "ignore",
    },
  },
};
