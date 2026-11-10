// The extension APIs the tests call inside the service worker or an extension page
// (`worker.evaluate`, `page.evaluate`).
declare const chrome: {
  storage: {
    local: { set(items: object): Promise<void> };
    session: { get(key: string): Promise<Record<string, unknown>> };
  };
  downloads: { search(query: object): Promise<{ filename: string; state: string }[]> };
  tabs: { query(query: object): Promise<{ id?: number }[]> };
  action: { getBadgeText(details: { tabId: number }): Promise<string> };
  runtime: { sendMessage(message: object): Promise<unknown> };
};
