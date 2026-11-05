import { test } from "node:test";
import assert from "node:assert/strict";
import { domainOf, searchOf, youtubeId, cleanTitle, categorize, summarize, interests, keywords, hm } from "../lib.js";

test("domains, searches and videos are read from URLs", () => {
  assert.equal(domainOf("https://www.github.com/x"), "github.com");
  assert.equal(domainOf("chrome://extensions"), "");
  assert.deepEqual(searchOf("https://www.google.com/search?q=rust+egui+ios&hl=en"), { engine: "Google", query: "rust egui ios" });
  assert.deepEqual(searchOf("https://www.youtube.com/results?search_query=lofi+beats"), { engine: "YouTube", query: "lofi beats" });
  assert.deepEqual(searchOf("https://duckduckgo.com/?q=gtd+weekly+review"), { engine: "DuckDuckGo", query: "gtd weekly review" });
  assert.equal(searchOf("https://www.google.com/maps"), null);
  assert.equal(youtubeId("https://www.youtube.com/watch?v=abc123&t=4"), "abc123");
  assert.equal(youtubeId("https://youtu.be/xyz"), "xyz");
  assert.equal(youtubeId("https://www.youtube.com/shorts/s1"), "s1");
  assert.equal(youtubeId("https://www.youtube.com/"), null);
  assert.equal(cleanTitle("(3) Lofi beats - YouTube"), "Lofi beats");
});

test("categories match domains and subdomains", () => {
  assert.equal(categorize("youtube.com"), "distraction");
  assert.equal(categorize("old.reddit.com"), "distraction");
  assert.equal(categorize("github.com"), "focus");
  assert.equal(categorize("example.org"), "neutral");
  assert.equal(categorize("example.org", { focus: ["example.org"], distraction: [] }), "focus");
});

test("summaries clip visits to the range and total by site, category and video", () => {
  const t0 = Date.UTC(2026, 10, 2, 15, 0, 0);
  const visits = [
    { start: t0, end: t0 + 30 * 60e3, url: "https://github.com/a", domain: "github.com", title: "repo", category: "focus" },
    { start: t0 + 30 * 60e3, end: t0 + 40 * 60e3, url: "https://www.youtube.com/watch?v=v1", domain: "youtube.com", title: "Talk - YouTube", category: "distraction" },
    { start: t0 - 60 * 60e3, end: t0 + 5 * 60e3, url: "https://example.org", domain: "example.org", title: "", category: "neutral" },
  ];
  const s = summarize(visits, [{ at: t0 + 1000, engine: "Google", query: "q" }], t0, t0 + 3600e3);
  assert.equal(s.byCategory.focus, 1800);
  assert.equal(s.byCategory.distraction, 600);
  assert.equal(s.byCategory.neutral, 300); // only the part inside the range
  assert.equal(s.domains[0].domain, "github.com");
  assert.deepEqual(s.videos.map((v) => [v.id, v.title, v.seconds]), [["v1", "Talk", 600]]);
  assert.equal(s.searches.length, 1);
  assert.ok(Math.abs(s.focusShare - 1800 / 2700) < 1e-9);
});

test("interests are top keywords per week", () => {
  const mon = new Date(2026, 10, 2, 12).getTime(); // a Monday
  const searches = [
    { at: mon, engine: "Google", query: "rust egui ios build" },
    { at: mon + 3600e3, engine: "Google", query: "egui wgpu metal" },
    { at: mon - 7 * 86400e3, engine: "Google", query: "sourdough starter" },
  ];
  const visits = [{ start: mon, end: mon + 60e3, url: "https://www.youtube.com/watch?v=v1", domain: "youtube.com", title: "Egui in 10 minutes - YouTube" }];
  const w = interests(searches, visits);
  assert.equal(w.length, 2);
  assert.equal(w[0].top[0].word, "egui"); // 3 mentions this week
  assert.equal(w[0].top[0].count, 3);
  assert.equal(w[1].top[0].word, "sourdough");
  assert.deepEqual(keywords("How to build the BEST Rust app?"), ["build", "rust", "app"]);
  assert.equal(hm(3900), "1h 05m");
});
