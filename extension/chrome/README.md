# GTDCraft Web Activity (Chrome)

See where your browsing time goes, so you notice when you're off task (or what you do while your agents work):

- **Sites and time** on the active tab, minus idle time, each marked *on task*, *neutral* or *distraction* from lists
  you edit (Settings).
- **Searches** on Google, Bing, DuckDuckGo, YouTube, Perplexity and GitHub.
- **YouTube videos** watched, with when and for how long.
- **Interests over time**: the top words from your searches and the videos you watched, week by week.

Everything stays on your computer (`chrome.storage.local`, kept for a year). If GTDCraft is running, new records are
also handed to it on `http://127.0.0.1:8766` every couple of minutes: they appear in GTDCraft's **Stats** perspective
("Web activity") and agents can read a summary with the `gtd_web_activity` MCP tool. Turn that off in Settings.

## Install

1. Open `chrome://extensions` and switch on **Developer mode**.
2. **Load unpacked** → choose this folder (`extension/chrome`).
3. Pin "GTDCraft Web Activity" and click it for today's summary; "History and interests" opens the dashboard.

## Develop

`npm test` runs the tests for the parsing, categories, summaries and interests in `lib.js` (Node 20+).
