// GTDCraft Web Activity: the pure parts (URL parsing, categories, summaries, interests), shared by the service
// worker, the popup and the dashboard, and tested with `node --test test/`.

export const DAY_MS = 86400000;

/** Host without "www." ("" for chrome:// and other non-web pages). */
export function domainOf(url) {
  try {
    const u = new URL(url);
    if (!/^https?:$/.test(u.protocol)) return "";
    return u.hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
}

const ENGINES = [
  { name: "Google", host: /(^|\.)google\.[a-z.]+$/, path: /^\/search/, param: "q" },
  { name: "Bing", host: /(^|\.)bing\.com$/, path: /^\/search/, param: "q" },
  { name: "DuckDuckGo", host: /(^|\.)duckduckgo\.com$/, path: /^\/$/, param: "q" },
  { name: "YouTube", host: /(^|\.)youtube\.com$/, path: /^\/results/, param: "search_query" },
  { name: "Perplexity", host: /(^|\.)perplexity\.ai$/, path: /^\/search/, param: "q" },
  { name: "GitHub", host: /^github\.com$/, path: /^\/search/, param: "q" },
];

/** A search in this URL: { engine, query } or null. */
export function searchOf(url) {
  let u;
  try {
    u = new URL(url);
  } catch {
    return null;
  }
  const host = u.hostname.replace(/^www\./, "");
  for (const e of ENGINES) {
    if (e.host.test(host) && e.path.test(u.pathname)) {
      const q = (u.searchParams.get(e.param) || "").trim();
      if (q) return { engine: e.name, query: q };
    }
  }
  return null;
}

/** A YouTube video id for watch / shorts / youtu.be URLs, else null. */
export function youtubeId(url) {
  try {
    const u = new URL(url);
    const host = u.hostname.replace(/^(www|m)\./, "");
    if (host === "youtube.com" && u.pathname === "/watch") return u.searchParams.get("v");
    if (host === "youtube.com" && u.pathname.startsWith("/shorts/")) return u.pathname.split("/")[2] || null;
    if (host === "youtu.be") return u.pathname.slice(1) || null;
  } catch {}
  return null;
}

/** "Lofi beats (2026) - YouTube" -> "Lofi beats (2026)"; drops a leading "(3) " notification count. */
export function cleanTitle(title) {
  return (title || "").replace(/^\(\d+\)\s*/, "").replace(/\s+-\s+YouTube$/, "").trim();
}

export const DEFAULT_LISTS = {
  distraction: ["youtube.com", "reddit.com", "x.com", "twitter.com", "facebook.com", "instagram.com", "tiktok.com", "news.ycombinator.com", "twitch.tv", "netflix.com"],
  focus: ["github.com", "linear.app", "docs.google.com", "notion.so", "stackoverflow.com", "developer.apple.com", "docs.rs", "crates.io", "claude.ai", "chatgpt.com", "localhost"],
};

/** "focus" | "distraction" | "neutral": a domain matches a list entry or any subdomain of it. */
export function categorize(domain, lists = DEFAULT_LISTS) {
  const hit = (list) => (list || []).some((d) => domain === d || domain.endsWith("." + d));
  if (hit(lists.focus)) return "focus";
  if (hit(lists.distraction)) return "distraction";
  return "neutral";
}

/** Local day key "YYYY-MM-DD" for a timestamp. */
export function dayKey(ms) {
  const d = new Date(ms);
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/**
 * Totals for visits in [from, to): seconds per domain and per category, the searches and the YouTube videos.
 * A visit is { start, end, url, domain, title, category } (ms timestamps); searches are { at, engine, query }.
 */
export function summarize(visits, searches, from, to) {
  const byDomain = {};
  const byCategory = { focus: 0, distraction: 0, neutral: 0 };
  const videos = {};
  for (const v of visits) {
    const s = Math.max(v.start, from);
    const e = Math.min(v.end, to);
    if (e <= s || !v.domain) continue;
    const secs = (e - s) / 1000;
    byDomain[v.domain] = (byDomain[v.domain] || 0) + secs;
    byCategory[v.category || "neutral"] += secs;
    const id = youtubeId(v.url);
    if (id) {
      const x = (videos[id] ||= { id, title: cleanTitle(v.title), seconds: 0, first: s });
      x.seconds += secs;
      if (v.title) x.title = cleanTitle(v.title);
      x.first = Math.min(x.first, s);
    }
  }
  const domains = Object.entries(byDomain).sort((a, b) => b[1] - a[1]).map(([domain, seconds]) => ({ domain, seconds }));
  const total = byCategory.focus + byCategory.distraction + byCategory.neutral;
  return {
    total,
    byCategory,
    focusShare: total ? byCategory.focus / total : 0,
    distractionShare: total ? byCategory.distraction / total : 0,
    domains,
    searches: searches.filter((x) => x.at >= from && x.at < to),
    videos: Object.values(videos).sort((a, b) => a.first - b.first),
  };
}

const STOP = new Set("a an and are as at be by for from how i in is it its me my of on or the this to vs what when where which who why with you your can do does best new 2024 2025 2026 youtube video official full".split(" "));

/** Words worth keeping from a search or a video title. */
export function keywords(text) {
  return (text || "")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s+#.-]/gu, " ")
    .split(/\s+/)
    .map((w) => w.replace(/^[.-]+|[.-]+$/g, ""))
    .filter((w) => w.length > 2 && !STOP.has(w) && !/^\d+$/.test(w));
}

/** Monday of the week containing ms, as a day key. */
export function weekKey(ms) {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
  return dayKey(d.getTime());
}

/** Interests over time: per week, the top keywords from searches and the titles of videos watched. */
export function interests(searches, visits, topN = 8) {
  const weeks = {};
  const add = (ms, text, w) => {
    const k = weekKey(ms);
    const m = (weeks[k] ||= {});
    for (const word of new Set(keywords(text))) m[word] = (m[word] || 0) + w;
  };
  for (const s of searches) add(s.at, s.query, 1);
  const seen = new Set();
  for (const v of visits) {
    const id = youtubeId(v.url);
    if (id && !seen.has(id + weekKey(v.start))) {
      seen.add(id + weekKey(v.start));
      add(v.start, cleanTitle(v.title), 1);
    }
  }
  return Object.keys(weeks)
    .sort()
    .reverse()
    .map((week) => ({ week, top: Object.entries(weeks[week]).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, topN).map(([word, count]) => ({ word, count })) }));
}

/** "1h 05m", "12m", "40s". */
export function hm(seconds) {
  const s = Math.round(seconds);
  if (s < 60) return `${s}s`;
  const m = Math.round(s / 60);
  return m >= 60 ? `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m` : `${m}m`;
}
