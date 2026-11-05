import { DEFAULT_LISTS } from "./lib.js";

const $ = (id) => document.getElementById(id);
const { lists = DEFAULT_LISTS, sendToGtdcraft = true } = await chrome.storage.local.get(["lists", "sendToGtdcraft"]);
$("focus").value = lists.focus.join("\n");
$("distraction").value = lists.distraction.join("\n");
$("send").checked = sendToGtdcraft;
const lines = (id) => $(id).value.split("\n").map((s) => s.trim().replace(/^www\./, "")).filter(Boolean);
$("save").onclick = async () => {
  await chrome.storage.local.set({ lists: { focus: lines("focus"), distraction: lines("distraction") }, sendToGtdcraft: $("send").checked });
  $("msg").textContent = "Saved.";
};
$("clear").onclick = async () => {
  if (!confirm("Delete all recorded sites, searches and videos?")) return;
  await chrome.storage.local.remove(["visits", "searches", "outbox"]);
  $("msg").textContent = "Deleted.";
};
