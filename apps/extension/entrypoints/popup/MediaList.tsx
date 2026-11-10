// The popup's media list: the videos, audio and HLS streams the current tab has played (found by
// the background's sniffing), each to hand to Omni Download.

import { Icon, type IconName } from 'omni-ui';
import { useEffect, useState } from 'react';

import { t } from '../../lib/i18n';
import { mediaName, mediaOf, type Media, type MediaKind } from '../../lib/media';
import { size } from '../../lib/page';

const ICON: Record<MediaKind, IconName> = { video: 'fileVideo', audio: 'fileAudio', hls: 'fileVideo' };
const KIND: Record<MediaKind, string> = { video: 'mediaVideo', audio: 'mediaAudio', hls: 'mediaHls' };

export function MediaList() {
  const [tabId, setTabId] = useState<number | null>(null);
  const [items, setItems] = useState<Media[]>([]);
  const [sent, setSent] = useState<Set<string>>(new Set());

  useEffect(() => {
    void browser.tabs.query({ active: true, currentWindow: true }).then(([tab]) => setTabId(tab?.id ?? null));
  }, []);

  // The list, and again whenever the background adds to it.
  useEffect(() => {
    if (tabId == null) return;
    const load = () => void mediaOf(tabId).then(setItems);
    load();
    const onChanged = (changes: Record<string, unknown>, area: string) => {
      if (area === 'session' && `media:${tabId}` in changes) load();
    };
    browser.storage.onChanged.addListener(onChanged);
    return () => browser.storage.onChanged.removeListener(onChanged);
  }, [tabId]);

  const download = (m: Media) => {
    void browser.runtime.sendMessage({ type: 'download-media', media: m });
    setSent((s) => new Set(s).add(m.url));
  };

  return (
    <section className="pmedia" aria-label={t('mediaTitle')}>
      <div className="psec">{t('mediaTitle')}</div>
      {items.length === 0 ? (
        <p className="pnone">{t('mediaNone')}</p>
      ) : (
        <ul>
          {[...items].reverse().map((m) => {
            const name = mediaName(m);
            const done = sent.has(m.url);
            return (
              <li key={m.url}>
                <Icon name={ICON[m.kind]} className={`pmi ${m.kind}`} />
                <span className="pmtxt">
                  <span className="pname" title={m.url}>
                    {name}
                  </span>
                  <small>{[t(KIND[m.kind]), m.size != null ? size(m.size) : null].filter(Boolean).join(' · ')}</small>
                </span>
                <button type="button" className="iconbtn" disabled={done} title={done ? t('mediaSent') : t('mediaDownload')} aria-label={`${t('mediaDownload')} ${name}`} onClick={() => download(m)}>
                  <Icon name={done ? 'check' : 'download'} />
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
