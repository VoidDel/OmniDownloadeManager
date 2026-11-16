// The popup's media list: the videos, audio and streams the current tab has played (found by the
// background's sniffing), each to hand to Omni Download. A site's picture and sound served apart
// (DASH) are offered as one video, which the app joins.
//
// Opening it looks at the page once more (`scan.ts`) for media the sniffing missed.
// Video and audio have tabs of their own when the page played both. Each row says the format,
// where it comes from and how big it is, copies its link, and downloads it (through the app's
// quick-download window); several chosen ones start at once, without a window each.

import { Icon, Segmented, type IconName } from 'omni-ui';
import { useEffect, useState } from 'react';

import { t, tn } from '../../lib/i18n';
import { formatOf, mediaOf, offers, offerTab, sourceOf, type Media, type Offer } from '../../lib/media';
import { size } from '../../lib/page';

const ICON: Record<Offer['kind'], IconName> = { video: 'fileVideo', audio: 'fileAudio', hls: 'fileVideo', dash: 'fileVideo' };

type Tab = 'all' | 'video' | 'audio';

/** What a row says under the name: format, a DASH picture's sound, source, size. */
function details(o: Offer): string {
  const parts = [
    formatOf(o) ?? t(offerTab(o) === 'audio' ? 'mediaAudio' : 'mediaVideo'),
    o.kind === 'dash' ? t(o.audio ? 'mediaDash' : 'mediaDashSilent') : null,
    sourceOf(o),
    o.size != null ? size(o.size) : null,
  ];
  return parts.filter(Boolean).join(' · ');
}

/** The tab whose media are listed: the active one, or one named in the address (`?tab=<id>`,
 *  for the end-to-end tests, which open the popup as a page of its own). */
async function currentTab(): Promise<number | null> {
  const named = Number(new URLSearchParams(location.search).get('tab'));
  if (Number.isInteger(named) && named > 0) return named;
  const [tab] = await browser.tabs.query({ active: true, currentWindow: true });
  return tab?.id ?? null;
}

const toSend = (o: Offer) => ({ media: o.media, audio: o.audio, name: o.name });

export function MediaList() {
  const [tabId, setTabId] = useState<number | null>(null);
  const [items, setItems] = useState<Media[]>([]);
  const [sent, setSent] = useState<Set<string>>(new Set());
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [copied, setCopied] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>('all');

  useEffect(() => {
    void currentTab().then(setTabId);
  }, []);

  // The list, and again whenever the background adds to it (the look at the page among others).
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

  const all = offers(items);
  const counts = { all: all.length, video: all.filter((o) => offerTab(o) === 'video').length, audio: all.filter((o) => offerTab(o) === 'audio').length };
  const tabs = counts.video > 0 && counts.audio > 0;
  const shown = tabs && tab !== 'all' ? all.filter((o) => offerTab(o) === tab) : all;
  const many = shown.length > 1;
  // What is chosen among the rows shown, and not sent yet.
  const picked = shown.filter((o) => chosen.has(o.media.url) && !sent.has(o.media.url));
  const open = shown.filter((o) => !sent.has(o.media.url));
  const allPicked = open.length > 0 && picked.length === open.length;

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
  const toggleAll = () => setChosen((c) => (allPicked ? new Set([...c].filter((u) => !open.some((o) => o.media.url === u))) : new Set([...c, ...open.map((o) => o.media.url)])));
  const copy = (o: Offer) => {
    const text = o.audio ? `${o.media.url}\n${o.audio.url}` : o.media.url;
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(o.media.url);
      setTimeout(() => setCopied((c) => (c === o.media.url ? null : c)), 1500);
    });
  };

  return (
    <section className="pmedia" aria-label={t('mediaTitle')}>
      <div className="pmhead">
        <span className="psec">{t('mediaTitle')}</span>
        {tabs && (
          <Segmented<Tab>
            label={t('mediaTitle')}
            value={tab}
            onChange={setTab}
            options={[
              ['all', `${t('mediaAll')} ${counts.all}`],
              ['video', `${t('mediaVideo')} ${counts.video}`],
              ['audio', `${t('mediaAudio')} ${counts.audio}`],
            ]}
          />
        )}
      </div>
      {shown.length === 0 ? (
        <p className="pnone">{t('mediaNone')}</p>
      ) : (
        <ul>
          {shown.map((o) => {
            const done = sent.has(o.media.url);
            return (
              <li key={o.media.url} className={done ? 'sent' : undefined}>
                {many && (
                  <input type="checkbox" className="pmcheck" aria-label={o.name} checked={done || chosen.has(o.media.url)} disabled={done} onChange={() => toggle(o)} />
                )}
                <Icon name={ICON[o.kind]} className={`pmi ${o.kind === 'dash' ? 'video' : o.kind}`} />
                <span className="pmtxt">
                  <span className="pname" title={o.audio ? `${o.media.url}\n${o.audio.url}` : o.media.url}>
                    {o.name}
                  </span>
                  <small>{details(o)}</small>
                </span>
                <button type="button" className="iconbtn pmcopy" title={copied === o.media.url ? t('mediaCopied') : t('mediaCopy')} aria-label={`${t('mediaCopy')} ${o.name}`} onClick={() => copy(o)}>
                  <Icon name={copied === o.media.url ? 'check' : 'copy'} />
                </button>
                <button type="button" className="iconbtn pmget" disabled={done} title={done ? t('mediaSent') : t('mediaDownload')} aria-label={`${t('mediaDownload')} ${o.name}`} onClick={() => download(o)}>
                  <Icon name={done ? 'check' : 'download'} />
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {many && (
        <div className="pmfoot">
          <label className="pmall">
            <input type="checkbox" className="pmcheck" checked={allPicked} disabled={!open.length} ref={(el) => void (el && (el.indeterminate = picked.length > 0 && !allPicked))} onChange={toggleAll} />
            {t('mediaSelectAll')}
          </label>
          <button type="button" className="btn outline sm" disabled={!picked.length} onClick={downloadPicked}>
            <Icon name="download" />
            {picked.length ? tn('mediaDownloadSome', picked.length) : t('mediaDownloadNone')}
          </button>
        </div>
      )}
    </section>
  );
}
