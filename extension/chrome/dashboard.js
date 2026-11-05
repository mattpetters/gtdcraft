import { summarize, interests, hm, dayKey, DAY_MS } from "./lib.js";

const $ = (id) => document.getElementById(id);
const li = (text, sub) => {
  const el = document.createElement("li");
  el.textContent = text;
  if (sub) {
    const s = document.createElement("span");
    s.textContent = sub;
    el.append(s);
  }
  return el;
};

async function draw() {
  const days = Number($("range").value);
  const { visits = [], searches = [] } = await chrome.storage.local.get(["visits", "searches"]);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const from = today.getTime() - (days - 1) * DAY_MS;
  const to = today.getTime() + DAY_MS;
  // per-day stacked bars
  const per = [];
  for (let t = from; t < to; t += DAY_MS) per.push({ t, s: summarize(visits, searches, t, t + DAY_MS) });
  const max = Math.max(1, ...per.map((p) => p.s.total));
  $("days").replaceChildren(
    ...per.map(({ t, s }) => {
      const col = document.createElement("div");
      col.className = "day";
      col.title = `${dayKey(t)}: ${hm(s.total)} (${Math.round(s.focusShare * 100)}% on task)`;
      const h = (x) => `${(100 * x) / max}%`;
      col.innerHTML = `<i class="distraction" style="height:${h(s.byCategory.distraction)}"></i><i class="neutral" style="height:${h(s.byCategory.neutral)}"></i><i class="focus" style="height:${h(s.byCategory.focus)}"></i>`;
      return col;
    }),
  );
  const all = summarize(visits, searches, from, to);
  $("sites").replaceChildren(...all.domains.slice(0, 20).map((d) => li(d.domain, hm(d.seconds))));
  $("videos").replaceChildren(...all.videos.slice(-30).reverse().map((v) => li(v.title || v.id, `${dayKey(v.first)} · ${hm(v.seconds)}`)));
  $("searches").replaceChildren(...all.searches.slice(-60).reverse().map((x) => li(x.query, `${x.engine} · ${dayKey(x.at)}`)));
  const weeks = interests(all.searches, visits.filter((v) => v.end > from));
  $("interests").replaceChildren(
    ...weeks.map((w) => {
      const row = document.createElement("div");
      row.className = "week";
      const label = document.createElement("b");
      label.textContent = "Week of " + w.week;
      row.append(label, ...w.top.map((k) => Object.assign(document.createElement("span"), { textContent: k.word, title: `${k.count}×`, className: "chip" })));
      return row;
    }),
  );
}

$("range").onchange = draw;
draw();
