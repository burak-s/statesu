// The verification page must exist as a static shell (adapter-static prerender)
// and inspect the token only in the browser; it is never needed at build time.
export const prerender = true;
export const ssr = false;
