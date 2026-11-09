// Alt+click on a link keeps that download in the browser (docs/development-plan.md §5.3): tells
// the background before the download starts.

export default defineContentScript({
  matches: ['<all_urls>'],
  runAt: 'document_start',
  allFrames: true,
  main() {
    document.addEventListener(
      'click',
      (e) => {
        if (!e.altKey) return;
        const a = (e.target as Element | null)?.closest?.('a[href]') as HTMLAnchorElement | null;
        if (a?.href) void browser.runtime.sendMessage({ type: 'bypass', url: a.href }).catch(() => undefined);
      },
      true,
    );
  },
});
