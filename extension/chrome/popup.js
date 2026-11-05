import { summarize, hm, dayKey } from "./lib.js";

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

const { visits = [], searches = [], paused = false } = await chrome.storage.local.get(["visits", "searches", "paused"]);
const start = new Date();
start.setHours(0, 0, 0, 0);
const s = summarize(visits, searches, start.getTime(), start.getTime() + 86400000);
$("total").textContent = hm(s.total) + " · " + dayKey(Date.now());
const pct = (x) => (s.total ? (100 * x) / s.total : 0);
$("bar").innerHTML = `<i class="focus" style="width:${pct(s.byCategory.focus)}%"></i><i class="neutral" style="width:${pct(s.byCategory.neutral)}%"></i><i class="distraction" style="width:${pct(s.byCategory.distraction)}%"></i>`;
$("share").textContent = s.total ? `${Math.round(s.focusShare * 100)}% on task, ${Math.round(s.distractionShare * 100)}% distraction` : "Nothing yet today.";
s.domains.slice(0, 8).forEach((d) => $("sites").append(li(d.domain, hm(d.seconds))));
s.searches.slice(-8).reverse().forEach((x) => $("searches").append(li(x.query, x.engine)));
s.videos.slice(-6).reverse().forEach((v) => $("videos").append(li(v.title || v.id, hm(v.seconds))));
const btn = $("pause");
btn.textContent = paused ? "Resume" : "Pause";
btn.onclick = async () => {
  await chrome.storage.local.set({ paused: !paused });
  location.reload();
};
