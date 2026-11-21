// Chrome DevTools Protocol over a WebView's or a browser's debugging port:
// the page targets, and a session that runs script in one.
export async function targets(port) {
  return (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).filter(target => target.type === 'page');
}

export async function tab(target) {
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve, { once: true });
    ws.addEventListener('error', reject, { once: true });
  });
  let id = 0;
  async function cdp(method, params) {
    const req = ++id;
    return new Promise((resolve, reject) => {
      const listener = (event) => {
        const answer = JSON.parse(event.data);
        if (answer.id !== req) return;
        ws.removeEventListener('message', listener);
        if (answer.error) reject(answer.error);
        else resolve(answer.result);
      };
      ws.addEventListener('message', listener);
      // A window closed under the call (the reader closing its site) ends it.
      ws.addEventListener('close', () => reject(new Error(`${method}: connection closed`)), { once: true });
      ws.send(JSON.stringify({ id: req, method, params }));
    });
  }
  return {
    cdp,
    async run(expression) {
      const result = await cdp('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? JSON.stringify(result.exceptionDetails));
      return result.result.value;
    },
    close: () => ws.close(),
  };
}
