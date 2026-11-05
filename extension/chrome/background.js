// Records where your browsing time goes: one "visit" per stretch of time on the active tab (closed when you switch
// tabs or windows, leave Chrome, or go idle), plus every search. Everything is kept in chrome.storage.local; if GTDCraft
// is running it also receives the new records every few minutes on http://127.0.0.1:8766 (nothing leaves your computer).
import { domainOf, searchOf, categorize, DEFAULT_LISTS, DAY_MS } from "./lib.js";

const KEEP_DAYS = 365; // a year, for interests over time
const GTDCRAFT = "http://127.0.0.1:8766/v1/activity";

let current = null; // { start, url, title, tabId }

async function settings() {
  const { lists = DEFAULT_LISTS, paused = false, sendToGtdcraft = true } = await chrome.storage.local.get(["lists", "paused", "sendToGtdcraft"]);
  return { lists, paused, sendToGtdcraft };
}

async function closeVisit(now = Date.now()) {
  if (!current) return;
  const v = current;
  current = null;
  if (now - v.start < 1000) return; // a blink
  const domain = domainOf(v.url);
  if (!domain) return;
  const { lists } = await settings();
  const visit = { start: v.start, end: now, url: v.url, domain, title: v.title || "", category: categorize(domain, lists) };
  const { visits = [], outbox = [] } = await chrome.storage.local.get(["visits", "outbox"]);
  visits.push(visit);
  outbox.push({ kind: "visit", ...visit });
  const cutoff = now - KEEP_DAYS * DAY_MS;
  await chrome.storage.local.set({ visits: visits.filter((x) => x.end > cutoff), outbox: outbox.slice(-5000) });
}

async function openVisit(tab) {
  const { paused } = await settings();
  if (paused || !tab || !tab.url) return;
  current = { start: Date.now(), url: tab.url, title: tab.title || "", tabId: tab.id };
}

/** The active tab of the focused window changed (or its page did): close the old stretch, start a new one. */
async function refresh() {
  await closeVisit();
  const win = await chrome.windows.getLastFocused({ populate: false }).catch(() => null);
  if (!win || !win.focused) return; // Chrome isn't in front
  const [tab] = await chrome.tabs.query({ active: true, windowId: win.id });
  if ((await chrome.idle.queryState(60)) !== "active") return;
  await openVisit(tab);
}

async function noteSearch(url) {
  const s = searchOf(url);
  if (!s || (await settings()).paused) return;
  const { searches = [], outbox = [] } = await chrome.storage.local.get(["searches", "outbox"]);
  const last = searches[searches.length - 1];
  if (last && last.query === s.query && last.engine === s.engine && Date.now() - last.at < 60_000) return; // same search, reloaded
  const rec = { at: Date.now(), ...s };
  searches.push(rec);
  outbox.push({ kind: "search", ...rec });
  const cutoff = Date.now() - KEEP_DAYS * DAY_MS;
  await chrome.storage.local.set({ searches: searches.filter((x) => x.at > cutoff), outbox: outbox.slice(-5000) });
}

/** Hands new records to GTDCraft when it's running; keeps them for next time when it isn't. */
async function flush() {
  if (current && Date.now() - current.start > 60_000) {
    const tab = await chrome.tabs.get(current.tabId).catch(() => null);
    await closeVisit();
    if (tab) await openVisit(tab); // long stretches are sent in pieces
  }
  const { sendToGtdcraft } = await settings();
  const { outbox = [] } = await chrome.storage.local.get("outbox");
  if (!sendToGtdcraft || outbox.length === 0) return;
  try {
    const r = await fetch(GTDCRAFT, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(outbox) });
    if (r.ok) await chrome.storage.local.set({ outbox: [], lastSent: Date.now() });
  } catch {
    /* GTDCraft isn't running: try again later */
  }
}

chrome.tabs.onActivated.addListener(refresh);
chrome.windows.onFocusChanged.addListener(refresh);
chrome.tabs.onUpdated.addListener(async (tabId, info, tab) => {
  if (info.url) noteSearch(info.url);
  if (tab.active && (info.url || info.title) && current && current.tabId === tabId) {
    if (info.url && info.url !== current.url) await refresh();
    else if (info.title) current.title = info.title;
  }
});
chrome.idle.setDetectionInterval(60);
chrome.idle.onStateChanged.addListener((state) => (state === "active" ? refresh() : closeVisit()));
chrome.alarms.create("flush", { periodInMinutes: 2 });
chrome.alarms.onAlarm.addListener((a) => a.name === "flush" && flush());
chrome.runtime.onStartup.addListener(refresh);
chrome.runtime.onInstalled.addListener(refresh);
chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg === "flush") flush().then(() => reply(true));
  return true;
});
