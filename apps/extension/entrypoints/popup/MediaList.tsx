// The popup's media list: the videos, audio and streams the current tab has played (found by the
// background's sniffing), each to hand to Omni Download. A site's picture and sound served apart
// (DASH) are offered as one video, which the app joins.

import { Icon, type IconName } from 'omni-ui';
import { useEffect, useState } from 'react';

import { t } from '../../lib/i18n';
import { mediaOf, offers, type Media, type Offer } from '../../lib/media';
import { size } from '../../lib/page';

const ICON: Record<Offer['kind'], IconName> = { video: 'fileVideo', audio: 'fileAudio', hls: 'fileVideo', dash: 'fileVideo' };

/** What kind of item it is, as the list says it. */
function kindLabel(o: Offer): string {
  if (o.kind === 'dash') return o.audio ? t('mediaDash') : t('mediaDashSilent');
  return t(o.kind === 'video' ? 'mediaVideo' : o.kind === 'audio' ? 'mediaAudio' : 'mediaHls');
}

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

  const download = (o: Offer) => {
    void browser.runtime.sendMessage({ type: 'download-media', media: o.media, audio: o.audio, name: o.name });
    setSent((s) => new Set(s).add(o.media.url));
  };

  const list = offers(items);
  return (
    <section className="pmedia" aria-label={t('mediaTitle')}>
      <div className="psec">{t('mediaTitle')}</div>
      {list.length === 0 ? (
        <p className="pnone">{t('mediaNone')}</p>
      ) : (
        <ul>
          {list.map((o) => {
            const done = sent.has(o.media.url);
            return (
              <li key={o.media.url}>
                <Icon name={ICON[o.kind]} className={`pmi ${o.kind === 'dash' ? 'video' : o.kind}`} />
                <span className="pmtxt">
                  <span className="pname" title={o.audio ? `${o.media.url}\n${o.audio.url}` : o.media.url}>
                    {o.name}
                  </span>
                  <small>{[kindLabel(o), o.size != null ? size(o.size) : null].filter(Boolean).join(' · ')}</small>
                </span>
                <button type="button" className="iconbtn" disabled={done} title={done ? t('mediaSent') : t('mediaDownload')} aria-label={`${t('mediaDownload')} ${o.name}`} onClick={() => download(o)}>
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
