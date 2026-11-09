// The extension APIs the tests call inside the service worker (`worker.evaluate`).
declare const chrome: {
  storage: { local: { set(items: object): Promise<void> } };
  downloads: { search(query: object): Promise<{ filename: string; state: string }[]> };
};
