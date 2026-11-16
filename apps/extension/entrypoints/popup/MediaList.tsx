// The popup's media tab: the videos, audio and streams the current tab has played (found by the
// background's sniffing), each to hand to Omni Download. A site's picture and sound served apart
// (DASH) are offered as one video, which the app joins.
//
// Each row starts with its format tile, coloured by kind (video, sound, stream); the tile is
// also how a row is chosen, for downloading several at once (they start without a window
// each). Video and audio have tabs of their own when the page played both.

import { useEffect, useState } from 'react';

import { qualityLabels } from '../../lib/hls';
import { t, tn } from '../../lib/i18n';
import { formatOf, mediaOf, offers, offerTab, sourceOf, type Media, type Offer } from '../../lib/media';
import { size } from '../../lib/page';
import { Icon, Tabs } from '../../lib/ui';

type Tab = 'all' | 'video' | 'audio';

/** The colour of a row's tile: a file with a picture, a sound, a stream (HLS, DASH). */
function kindClass(o: Offer): string {
  if (o.kind === 'hls' || o.kind === 'dash' || o.kind === 'mpd') return 'k-stream';
  return offerTab(o) === 'audio' ? 'k-audio' : 'k-video';
}

/** The tab whose media are listed: the active one, or one named in the address (`?tab=<id>`,
 *  for the end-to-end tests, which open the popup as a page of its own). */
async function currentTab(): Promise<number | null> {
  const named = Number(new URLSearchParams(location.search).get('tab'));
  if (Number.isInteger(named) && named > 0) return named;
  const [tab] = await browser.tabs.query({ active: true, currentWindow: true });
  return tab?.id ?? null;
}

/** The media the current tab has played, kept up to date while the popup is open. Opening it
 *  looks at the page once more (`scan.ts`) for media the sniffing missed. */
export function usePageMedia(): Media[] {
  const [tabId, setTabId] = useState<number | null>(null);
  const [items, setItems] = useState<Media[]>([]);

  useEffect(() => {
    void currentTab().then(setTabId);
  }, []);

  useEffect(() => {
    if (tabId == null) return;
    void browser.runtime.sendMessage({ type: 'scan-media', tabId }).catch(() => undefined);
    const load = () => void mediaOf(tabId).then(setItems);
    load();
    const onChanged = (changes: Record<string, unknown>, area: string) => {
      if (area === 'session' && `media:${tabId}` in changes) load();
    };
    browser.storage.onChanged.addListener(onChanged);
    return () => browser.storage.onChanged.removeListener(onChanged);
  }, [tabId]);

  return items;
}

export function MediaList({ items, id }: { items: Media[]; id: string }) {
  const [sent, setSent] = useState<Set<string>>(new Set());
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [copied, setCopied] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>('all');
  /** The quality chosen for each master HLS playlist (by its address); unset: the best. */
  const [quality, setQuality] = useState<Record<string, string>>({});
  const toSend = (o: Offer) => ({ media: o.media, audio: o.audio, name: o.name, hlsVariant: quality[o.media.url] ?? o.media.variants?.[0]?.url ?? null });

  const all = offers(items);
  const counts = { all: all.length, video: all.filter((o) => offerTab(o) === 'video').length, audio: all.filter((o) => offerTab(o) === 'audio').length };
  const tabs = counts.video > 0 && counts.audio > 0;
  const shown = tabs && tab !== 'all' ? all.filter((o) => offerTab(o) === tab) : all;
  // What is chosen among the rows shown, and not sent yet.
  const open = shown.filter((o) => !sent.has(o.media.url));
  const picked = open.filter((o) => chosen.has(o.media.url));

  const markSent = (list: Offer[]) => {
    setSent((s) => new Set([...s, ...list.map((o) => o.media.url)]));
    setChosen((c) => new Set([...c].filter((u) => !list.some((o) => o.media.url === u))));
  };
  const download = (o: Offer) => {
    void browser.runtime.sendMessage({ type: 'download-media', ...toSend(o) });
    markSent([o]);
  };
  const downloadPicked = () => {
    if (!picked.length) return;
    void browser.runtime.sendMessage({ type: 'download-media-batch', items: picked.map(toSend) });
    markSent(picked);
  };
  const toggle = (o: Offer) =>
    setChosen((c) => {
      const next = new Set(c);
      if (!next.delete(o.media.url)) next.add(o.media.url);
      return next;
    });
  const copy = (o: Offer) => {
    const text = o.audio ? `${o.media.url}\n${o.audio.url}` : o.media.url;
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(o.media.url);
      setTimeout(() => setCopied((c) => (c === o.media.url ? null : c)), 1500);
    });
  };

  return (
    <section className="ppanel pmedia" id={id} role="tabpanel" aria-labelledby={`tab-${id}`}>
      {tabs && (
        <div className="pshead">
          <Tabs<Tab>
            label={t('mediaTitle')}
            value={tab}
            onChange={setTab}
            options={[
              ['all', `${t('mediaAll')} ${counts.all}`],
              ['video', `${t('mediaVideo')} ${counts.video}`],
              ['audio', `${t('mediaAudio')} ${counts.audio}`],
            ]}
          />
        </div>
      )}
      {shown.length === 0 ? (
        <p className="pnone">{t('mediaNone')}</p>
      ) : (
        <ul className="mlist">
          {shown.map((o) => {
            const done = sent.has(o.media.url);
            const on = !done && chosen.has(o.media.url);
            const variants = o.media.variants ?? [];
            const format = formatOf(o) ?? t(offerTab(o) === 'audio' ? 'mediaAudio' : 'mediaVideo');
            return (
              <li key={o.media.url} className={`mrow ${kindClass(o)}${on ? ' on' : ''}${done ? ' sent' : ''}`}>
                <button type="button" className="mtile" aria-pressed={on} disabled={done} title={t('mediaChoose')} aria-label={`${t('mediaChoose')} ${o.name}`} onClick={() => toggle(o)}>
                  {on ? <Icon name="check" /> : format}
                </button>
                <span className="mtxt">
                  <span className="mname" title={o.audio ? `${o.media.url}\n${o.audio.url}` : o.media.url}>
                    {o.name}
                  </span>
                  <span className="mmeta">
                    {variants.length > 1 && (
                      <select
                        className="mq"
                        aria-label={`${t('mediaQuality')} ${o.name}`}
                        title={t('mediaQuality')}
                        value={quality[o.media.url] ?? variants[0].url}
                        disabled={done}
                        onChange={(e) => setQuality((q) => ({ ...q, [o.media.url]: e.target.value }))}
                      >
                        {variants.map((v, i) => (
                          <option key={v.url} value={v.url}>
                            {qualityLabels(variants)[i]}
                          </option>
                        ))}
                      </select>
                    )}
                    <span className="src">{sourceOf(o)}</span>
                    {o.kind === 'dash' && <span className="note">{t(o.audio ? 'mediaDash' : 'mediaDashSilent')}</span>}
                    {o.size != null && <span className="size">{size(o.size)}</span>}
                  </span>
                </span>
                <button type="button" className="icon-btn mcopy" title={copied === o.media.url ? t('mediaCopied') : t('mediaCopy')} aria-label={`${t('mediaCopy')} ${o.name}`} onClick={() => copy(o)}>
                  <Icon name={copied === o.media.url ? 'check' : 'copy'} />
                </button>
                <button type="button" className="mget" disabled={done} title={done ? t('mediaSent') : t('mediaDownload')} aria-label={`${t('mediaDownload')} ${o.name}`} onClick={() => download(o)}>
                  <Icon name={done ? 'check' : 'download'} />
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {picked.length > 0 && (
        <div className="mbar" role="group" aria-label={t('mediaChosen')}>
          <span className="n">{tn('mediaChosenCount', picked.length)}</span>
          {picked.length < open.length && (
            <button type="button" className="link" onClick={() => setChosen((c) => new Set([...c, ...open.map((o) => o.media.url)]))}>
              {t('mediaSelectAll')}
            </button>
          )}
          <button type="button" className="link" onClick={() => setChosen(new Set())}>
            {t('mediaClear')}
          </button>
          <button type="button" className="btn primary" onClick={downloadPicked}>
            <Icon name="download" />
            {t('mediaDownloadChosen')}
          </button>
        </div>
      )}
    </section>
  );
}
