// Each screen at its own path: `/`, `/library`, `/options`. A fresh load at
// any of them is served the app, which shows the screen for the path.
export const routes = { '/': 'S1', '/library': 'S2', '/options': 'S3' };

export const route = $state({ path: typeof location === 'undefined' ? '/' : location.pathname });

export function screenFor(path) {
  return routes[path.replace(/\/+$/, '') || '/'] ?? 'S1';
}

export function navigate(path) {
  if (path === route.path) return;
  history.pushState({}, '', path);
  route.path = path;
  window.scrollTo(0, 0);
}

if (typeof window !== 'undefined') {
  window.addEventListener('popstate', () => {
    route.path = location.pathname;
  });
}

/** For `<a href>`: an ordinary click moves inside the app; others are left to the browser. */
export function link(event) {
  if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  event.preventDefault();
  navigate(new URL(event.currentTarget.href).pathname);
}
