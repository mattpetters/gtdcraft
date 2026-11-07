# GTDCraft rules sheet

How GTDCraft behaves, value by value. Values marked *estimated* are judgement calls rather than settled rules.

## Task status
Status numbers: Blocked 0, Available 1, Next 2, Completed 3, DueSoon 4, Overdue 5, Dropped 6. Checked in this order:
1. completed (own or inherited) → Completed
2. dropped (own or inherited) → Dropped
3. containing project not active and not in the Inbox → Blocked (project on hold)
4. overdue → Overdue (beats defer and sequence)
5. blocked by a future defer date → Blocked (deferred)
6. due soon → DueSoon
   (due on or before the due-soon cutoff: the interval — default 172800 s, two days — rounded to whole days; the start
   of today plus that many days, minus one second; a negative interval counts as none; `due_soon_cutoff_for`)
7. the next task of its project → Next
8. otherwise blocked → Blocked, else Available

Sequential projects make only the topmost active action available; parallel projects' topmost available action is
"first available"; in single-actions lists every action is first available. Actions with an on-hold tag are blocked
(*estimated*). A task inherits the earliest due date and latest defer date from its parents.

## Due soon
- Due-soon interval 2 days (172800 s), granularity days.
- The interval is divided by 86400 and rounded to N whole days; due soon ends at start of today + N days − 1 s. The
  result is cached until the start of the next day. So due soon = due on or before 23:59:59 tomorrow (N = 2).
- Overdue = due date before now.

## Default times
Due 17:00, Planned 09:00, Defer 00:00, floating time zone on. Review every `@1w` (7 days).
New action groups and new projects are Parallel.

## Repeat
- New repetition: every 1 Day, Regularly, based on Due.
- Units: minutes, hours, days, weeks, months, years.
- Regularly: next occurrence keeps each date's calendar step. Catch up automatically skips past occurrences
  so the next one is always in the future.
- From Completion: next based-on date = completion + interval, rounded (hours round at the 30th minute;
  days and longer keep the old time of day); other dates keep their offsets.
- Completing **or dropping** a repeating item creates the next occurrence.
- End after N: stops after N completions/drops.
- Not built: "Only on" weekdays / days of month, End on date.

## Projects & review
- Statuses: Active, On Hold, Completed (marks unfinished actions complete), Dropped.
- Mark Reviewed: last reviewed = now, next review = today + review interval, then the next project is shown.
- Review lists projects whose next review is today or earlier, oldest first.

## Forecast
Items show on a day for their due, planned or defer date. Tile summary dots: red overdue, yellow due soon,
grey other due items; none on Future. Calendar events are not shown (no calendar access).

## Status circle
Yellow = due soon, red = overdue, orange + small flag = flagged, three dots = repeating, check = completed,
dash = dropped. Purple title = Next / first available.

## Keys
⌘1 Inbox, ⌘2 Projects, ⌘3 Tags, ⌘4 Forecast, ⌘5 Flagged, ⌘6 Nearby, ⌘7 Review, ⌘8 Changed, ⌘9 Completed;
⌘N new action, ⇧⌘N new project, Return new item below / end editing, Space complete, ⌥Space drop, F or ⇧⌘L flag,
E or ⌘Return edit title, ⌃⌥Space Quick Entry (in-app only here), ↑/↓ select.

## Layout (at 0.72 points per pixel)
Toolbar 52, rail 82, middle column 241, inspector 290, row 31, second line 17. Colours: background #222121,
rail #292726, selection #40526e, status blue #4680f9, flag orange #f0ac3d.

## Agent connection
`gtdcraft --mcp` serves MCP over stdio (JSON-RPC 2.0, one message per line) on the same database file
(`~/Library/Application Support/GTDCraft/gtdcraft.json`, or `--db` / `GTDCRAFT_DB`). The window reloads the file
within a second when an agent changes it.

## Main window rules
| Rule | In GTDCraft |
|---|---|
| Complete toggles: takes the selected tasks; if **all** are already completed it clears their completion date, otherwise it completes each; beeps when nothing is selected | Space / circle click toggles (single selection) |
| Dropping a repeating task asks first | Dropping a repeating task asks: Skip Occurrence / Drop Completely / Cancel |
| Mark Reviewed marks the selected project, rearranges the list and selects the next project | Review "Mark Reviewed" moves to the next project |
| After a task completes, completed items are hidden only if automatic hiding is on (off by default), after a 0.25 s delay | Setting "Hide completed items automatically" (off by default), 0.25 s delay |
| Cleanup pushes partial edits then cleans up every window | Clean Up button / leaving a perspective clears resolved rows |
| Quick Entry save commits editing, then fills in pending generated dates and adds the items | Quick Entry Save |
| Quick Entry shortcut ⌃⌥Space | ⌃⌥Space (inside the app only) |
| Quick Entry items don't go straight to the Inbox | Quick Entry has a project picker |
| Bare Delete key does nothing in the outline | Only ⌘Delete deletes |
| Rows aren't auto-expanded | Groups open unless collapsed by you (*estimated*) |
| Status circle diameter = 23 × base font ÷ 17; another metric = 12 × base font ÷ 17; constants 6 and 2 | Circle radius = 23 × 13.5 ÷ 17 ÷ 2 ≈ 9.1 pt (which field takes 23/17 is *inferred* from the size matching the screenshots) |
| "No Items Selected" in the empty inspector | Same text |

## Themes
- Format: `themes/theme.schema.json`. A theme = `name`, `mode` (dark/light), a named `palette`, and `ui` slots that
  point at palette names or `#rrggbb` (nvim-colorscheme style). Only `bg`, `fg`, `accent` are required; other slots
  are derived from them.
- Built in: Focus Dark (the default look), Focus Light (*estimated*),
  Kanagawa Paper Ink and Canvas (palettes from mattpetters/emacs-kanagawa-paper-canvas-ink).
- Your own: `~/Library/Application Support/GTDCraft/themes/*.json` (Settings › Appearance › Open Themes Folder).
  A file with the same name as a built-in replaces it.
- Perspective colours come from nine hue slots (red … violet). Each is pushed toward white (dark themes) or black
  (light themes) until it has at least 3:1 WCAG contrast with the background and the sidebar.
- "Match macOS light/dark" picks a light and a dark theme.

## Reminders
Defaults: date/time reminders on, automatic due-date reminders on, automatic defer-date reminders off, automatic
planned-date reminders off. GTDCraft checks each second while open and posts each time once when the clock passes it
(macOS Notification Center via osascript). Not built: custom notifications, snooze (15 minutes).

## Perspectives
Built-ins can be hidden, renamed, recoloured and reordered; custom ones filter by availability
(first available / available / remaining / completed / dropped / all), flagged, tag, project and due-within-N-days,
grouped by none / project / tag.

## Nearby
Tags can carry a place (the tag inspector's Location: Here, Search, Address or Pin — Apple Maps search on Mac and
iPhone, OpenStreetMap Nominatim elsewhere; see Round 45). Nearby lists available actions per place, nearest first from
"My location". Map and location: Round 40 (Apple MapKit and Core Location on Mac and iPhone; OSM tiles from
tile.openstreetmap.org, cached 30 days in `~/Library/Caches/GTDCraft/tiles`, elsewhere).

## Reminders, places and perspectives (round 3)
| Rule | In GTDCraft |
|---|---|
| Four automatic reminder kinds: defer → effective defer date; latest time to start → effective due date minus the estimate; planned → effective planned date; due → effective due date. Each is gated by its setting and only made when the date is after now | Reminders use inherited due/defer dates; new "latest time to start" option (off by default) |
| Snooze = now + 15 minutes | Not built: macOS banners from osascript can't carry a Snooze button |
| A place's radius defaults to 1 km when unset or ≤ 0 | `Place::radius()` |
| Place sizes small / medium / large = 200 m / 1 km / 10 km | Tag inspector "Area" picker; radius circles on the map; Nearby says "you're here" inside the radius |
| Built-in perspectives can't be deleted; only custom ones are deleted | Built-ins can only be hidden; Remove appears on custom perspectives only |
| Note fields accept whatever the system text view reads from the pasteboard | Link cards in notes are an addition on top |

## Round 4 additions
- Selected rows size to field strip + note (at least one line, "Add Note") so nothing spills out.
- Due soon / overdue titles take the status colour, dates sit in a tinted pill (italic when inherited),
  flagged items get a small flag notch on the circle and a filled (own) or outlined (inherited) flag.
- Action groups: Group Type (Parallel/Sequential) and "Complete with last action" (default off).
- Custom perspectives: nested All / Any / None rule tree; Structure (Flexible / Organized by project, tag, folder,
  due, flagged) and initial order.
- View options (eye): Available / Remaining / Everything (+ First Available outside the Inbox), Forecast
  planned/defer toggles, List / Board.
- Keyboard: every Mac shortcut, remappable in Settings › Keyboard.
- Board (kanban): columns per project / tag / day, or To Do / Waiting / Done; dropping changes the task.
- AI perspective drafting: Claude (claude-opus-5-5, structured output, server-side fallbacks), any
  OpenAI-compatible server, or any ACP agent (initialize → session/new → session/prompt).
- Linear: issue links on tasks and projects; GraphQL at api.linear.app with a personal key; status changes via
  issueUpdate(stateId).
- Time: org-mode style clock in/out (one clock at a time, sub-actions roll up, zero-length clocks dropped),
  Pomodoro 25/5/15 every 4 (*estimated* classic defaults), Stats perspective.
- App icon: an elephant holding a check — the Craft apps already use raven, cat, unicorn, rhino, fox,
  zebra and dragon.

## Round 4 — flags, rules, indenting
| Rule | In GTDCraft |
|---|---|
| Effective flag = own flag OR parent's effective flag; on change it is pushed to every child | `Store::effective_flagged`; inherited flags draw as orange outlines, own flags filled |
| Perspective rule vocabulary: availability, within duration, has duration, has tag with status, untagged, has any / all of tags, project status, in single-actions list, no project, within focus, matching search, is project or group, has due / defer / planned date, date before / after / on, yesterday / today / tomorrow, in the next / in the past (amount + unit), repeats, is leaf, status, aggregate (All / Any / None), disabled | `rules::Rule` covers each of these kinds (Tags any/all, TagStatus, ProjectStatus, SingleActions, ProjectOrGroup, Leaf, DateOn, DateBefore/After, Off …) |
| A turned-off rule is not evaluated as a filter | `Rule::Off` is skipped inside All / Any / None; "Turn Off / Turn On" in the editor's ⋯ menu |
| Indent / Outdent / Ungroup validate first and beep when not allowed; indent keeps the selection | `Store::indent / outdent / ungroup` return false (no change) when not allowed; selection kept |
| Outdent is only possible when the item's parent isn't the outline root | `Store::outdent` requires a parent action |

## Round 5 fixes
- ⌘N / "+" crash: status lookups used last frame's snapshot after a task was added. Each panel now re-derives
  the snapshot, and lookups (`Derived::st`, `task_at`) can't panic on a task added mid-frame.
- Typing a new title no longer shifts the rows below (the editor sits in its own child layout).
- Selected-row field strip is clickable: Project, Tags, Duration, Defer, Planned, Due, Repeat open an editor
  under the field; the quick-edit keys (P, T, ⇧T, ⇧D, ⇧P, D, R) open the same editors.
- Dates accept natural text everywhere (row editors, inspector, Quick Entry, MCP): today, tomorrow 5pm, fri,
  next week, +3d, 2w, in 3 days, nov 15, 11/15, 2026-11-15 17:00 (`dates.rs`, tested).
- Icon: fine-line engraved elephant on kraft tan (unused by the 18 Craft apps; FilmCraft already uses violet
  and an owl). Generated by `assets/make_icon.py`.

## Round 6 — typed dates
| Rule | In GTDCraft |
|---|---|
| Modifier encoding: next = −1, last = +1, this/none = 0 (tomorrow = day 1 next, yesterday = day 1 last, next month = month 1 next, last month = month 1 last) | `parse_day` modifier |
| Multiplier = −1 when the modifier is last, else +1 | "last week/month/year" go back one unit, "next …" forward |
| Units: hour, day, week (×7 days), month, year; amount = number × multiplier | `offset`; hours ("+3h", "3 hours", "in 3 hours") add to now (`parse_moment`) |
| Weekday: diff = target − current (Sunday = 1). No modifier: same day stays today, earlier day → diff + 7. next: diff + 7. last: diff − 7 | `weekday_offset` ("mon" on a Monday = today; "next fri" on Mon Nov 2 = Nov 13; "last fri" = Oct 30) |
| Year for a month without a year compares months only: same month → this year (next +1, last −1); later month → this year (last −1); earlier month → next year (last → this year) | `month_year` ("nov 1" on Nov 2 stays this year; "oct 5" → next year) |
| Special time names: noon = hour 12, tonight = today hour 23, now = current time | `parse_time` noon, `parse_moment` tonight / now |
| Vocabulary: today/tod, tomorrow/tom, yesterday/yes, tonight, now, noon, this/next/last week/month/year, h/d/w/m/y, + / − | `parse_day`, `parse_moment` |

Not done yet: "this/next/last week" should mean the start of the week for start dates and the end of the week for due
dates (*estimated*). GTDCraft still treats them as ±7 days.

## Round 7 fixes
- Sidebar perspectives scroll when the window is too short to show them all; picking one with a shortcut
  (or resizing the window) scrolls it back into view. `--size WxH` opens the window at a given size.

## Round 8 — perspective sidebar and menu bar
| Rule / value | In GTDCraft |
|---|---|
| The perspective sidebar scrolls vertically with no scroll bar | rail `ScrollArea` with `ScrollBarVisibility::AlwaysHidden` |
| The selected perspective is brought into view with the smallest scroll | `scroll_to_rect(.., None)` until the item is visible, again after a resize |
| Sidebar width with titles 82; without titles 64; tab min height with title 64; top inset 16 | `RAIL_W` 82, row pitch 64, top 16 |
| Selection corner radius: 12 on macOS 27+, 16 before | `RAIL_SEL_RADIUS` 12 |
| Other sidebar metrics: selection outer padding 4, content top/bottom padding 6, custom icon 28, min tap target 44, title line height 12 | recorded in mechanics.json |
| Menu bar: GTDCraft, File, Edit, Organize, View, Perspectives, Window, Help, with these key equivalents: New Action ⌘N, New Project ⇧⌘N, New Tag ⌃⌘N, Quick Open ⌘O, Close ⌘W, Sync Now ⌘S; Undo ⌘Z, Redo ⇧⌘Z, Duplicate ⌘D, Mark Reviewed ⇧⌘R, Set Flag ⇧⌘L, Find ⌘F; Clean Up ⌘K, Add Inside ⇧⌘], Add Outside ⇧⌘[, Move ⌃⌘arrows, Indent ⌘], Outdent ⌘[, Group ⌥⌘G, Ungroup ⌥⌘U; Go Back ⌥⌘[, Go Forward ⌥⌘], Show Sidebar ⌥⌘S, Show Inspector ⌥⌘I, Show View Options ⇧⌘V, Enter Full Screen ⌃⌘F; Show Perspectives List ⌃⌘P; Minimize ⌘M | `menu.rs` builds these menus (no Format menu: notes are Markdown). Items with ⌘/⌃ shortcuts replay the key, so custom shortcuts drive the menus too |

Also in File: New Database… ⌥⌘N, Open Database… ⇧⌘O, Open Recent, Save Database As… ⇧⌘S,
Export… ⇧⌘E (.json / .jsonc / .jsonl by extension), Sync Now ⌘S (save, git commit, push when a remote exists),
Keep Version History (git; commits 30 s after the last save, on quit and when switching databases), Version History…
(restore any version, undoable), Show Database in Finder. The open database is remembered and `--mcp` follows it.
Inspector sections can be reordered (⋯ menu on each section, or Settings › Appearance); integrations such as Linear
are sections with a logo. Settings › Agent finds installed ACP agents (official ACP registry commands; Hermes from its docs).

## Round 9 — typed paths; and fixes
| Rule / value | In GTDCraft |
|---|---|
| Hierarchy separator for typed names is `" : "` (space, colon, space) | `project_path` / `tag_path` show "Folder : Project", "Parent : Child"; typing accepts any spacing around ":" |
| Creating from a typed path walks the components one level at a time: an existing child of that name is reused, a missing one is inserted under the previous level (folders for projects, parent tags for tags) | `Store::project_for_path`, `Store::tag_for_path`; the Project and Tags field editors offer "New project/tag “…”" and Enter creates it when nothing matches; MCP tags use the same path rule |

Fixes this round:
- Missing glyphs (⌃ ⌥ ↑ ↓ ⋯ ↺) now render: macOS's Apple Symbols font is the last fallback in every font family.
- 34 popular themes built in (Tokyo Night ×4, Catppuccin ×4, Everforest, Gruvbox, Rosé Pine ×3, Kanagawa ×3, Nord,
  Dracula, One Dark, Nightfox, Carbonfox, Solarized, GitHub Dark, Ayu Mirage, Melange, Oxocarbon, Vesper, Poimandres,
  Night Owl, Monokai Pro), with colours taken unchanged from each scheme's Ghostty file in mbadolato/iTerm2-Color-Schemes
  (`themes/make_popular.py` regenerates them).
- Drag-and-drop ordering: Settings › Perspectives, Settings › Appearance (inspector sections) and the inspector's
  "Rearrange Sections…" mode use grips you drag; projects drag onto a folder (or a project in it) to file them, or onto
  "No Folder" to take them out.
- Multiple selection: ⌘-click toggles a row, ⇧-click selects a range in on-screen order; complete, drop, flag, delete and
  the row's field editors apply to every selected action.
- Nested tags: a parent tag's list includes its children's actions; the sidebar indents children.

## Round 10 — fixes and new pieces
- Tags list/board crash-free: an action with several tags appears once per tag; each copy's widgets are salted with its
  header (list) or column (board), so egui IDs no longer clash.
- Board columns are a per-perspective choice (View Options › Columns): usual, To Do / Waiting / Done, by project, by
  tag, or the stages of a status set. A status set is any tag with child tags (e.g. Status : Backlog, To Do, In Progress,
  Waiting for Review — "Add a Status set" creates that one). Dragging a card to a stage removes the action's other stages
  of that set and adds the new one; its other tags stay. In Projects, a filtered single project keeps its own column
  choice. Tag rules match child tags ("Status" matches "Status : In Progress"), so a custom perspective like
  "Chores, In Progress" is just two tag rules.
- Pomodoro pill shows the action's name (cut to fit before the search field); hover shows "Project : Action".
- Focus window (⌃⌘M, Window › Focus Window): the action in focus and its project, the Pomodoro or clock timer, Stop /
  Done, time until the next due or planned time today, and Up Next (⌃⌘U adds; drag to reorder; click to bring forward;
  Done completes and moves on). It can float above other apps. Calendar events are not read yet.

## Round 11 — Forecast calendar events
| Rule / value | In GTDCraft |
|---|---|
| Event filter: not cancelled (EventKit status 3) and ends after the start of the day | `calendar.rs` drops cancelled events |
| An event belongs to a day if `startDate >= S AND startDate <= E`, or `endDate >= S AND endDate <= E`, or `startDate < S AND endDate > E`; E = end of day − 60 s | the fetch asks EventKit for [start of day, start of day + 86400 − 60 s] |
| All-day events are left out unless the include-all-day setting is on | all-day events are skipped (they have no time to count down to) |
| Events are prefetched for 35 days | GTDCraft reads only today, every 5 minutes (only today feeds the countdown) |
| Hidden calendars and declined / pending / delegated events are settings | not yet: all calendars are read |
| Forecast shows the event timeline by default | Focus window: "Include Calendar events" is off until you turn it on (it needs macOS permission) |

Calendar events are read through JavaScript for Automation and EventKit (`/usr/bin/osascript -l JavaScript`). Reading never
prompts; "Allow Calendar Access" asks macOS once. The app bundle carries NSCalendarsFullAccessUsageDescription.
The countdown shows "In <event> until <time>" during an event and the sooner of the next event and the next due /
planned action today.

## Round 12 — Calendar throughout the app
- Calendar access now runs inside GTDCraft through EventKit (`calendar.rs`: authorizationStatusForEntityType,
  requestFullAccessToEventsWithCompletion, predicateForEventsWithStartDate:endDate:calendars:). The earlier
  osascript helper couldn't show the macOS prompt, and a click while a read was running was dropped; now the request
  is queued and handled by the app. `Info.plist` (with NSCalendarsFullAccessUsageDescription) is embedded in the
  executable by `build.rs` (`-sectcreate __TEXT __info_plist`) and copied into the ad-hoc-signed GTDCraft.app.
- The Round 11 rules apply: 35 days read ahead, cancelled (status 3) dropped and the day-overlap rule with end of
  day − 60 s, all-day events only when "Include all-day events" is on, hidden calendars chosen in Settings › General ›
  Calendar. Shown by default once allowed.
- Forecast: each day lists its events first (calendar colour bar, time, title, calendar; location on hover), then its
  actions; days further out than the week can be opened from the calendar grid, whose cells show a dot per calendar
  colour. The Forecast sidebar offers "Allow Calendar Access" (or Privacy Settings when it's off).
- Focus window: "In <event> until …" and the countdown to the sooner of the next event or timed action later today.
- Agents: `gtd_calendar_events` (from, days ≤ 35) returns the same events.

## Round 13 — event hours and calendar exclusion
| Rule / value | In GTDCraft |
|---|---|
| An event fills hours floor(start) .. floor(start) + max(ceil(end) − floor(start), 1), where start = hour + minute/60 (0 if it began on an earlier day) and end = hour + minute/60 (24 if it runs past the day); all-day events fill 0–24 | `calendar::hours` / `calendar::busy`; Forecast draws each day's 24-hour timeline (busy hours filled, now marked on today) |
| Visible calendars = all event calendars except hidden ones (by calendar identifier) and automatically excluded ones | Settings › General › Calendar hides calendars by identifier |
| Automatically excluded: natural-language-suggested, suggested-event and Facebook birthday calendars, each when its setting is on; each test runs only if the calendar responds to EventKit's selector | `calendar::auto_excluded` asks `isNaturalLanguageSuggestedEventCalendar`, `isSuggestedEventCalendar`, `isFacebookBirthdayCalendar` via respondsToSelector:, skipping those calendars and their events |
| Fetch window for a day: start of day … start of next day − 60 s, visible calendars only | `calendar::events` (whole 35-day range ends at its last day's −60 s) |

## Round 14 — shortcut menus on rows
Right-click or Control-click an action for its shortcut menu: Flag/Unflag, Complete, Drop, Inspect; Duplicate, Add
to Up Next, Start Pomodoro; Type (Parallel / Sequential / Complete with Last Action, on action groups); Organize (Add
Inside, Add Action, Group, Ungroup, Indent, Outdent, Move Up/Down, Convert to Project); Schedule (Due Today, Due
Tomorrow, Defer until Tomorrow, Plan for Today, Plan for Tomorrow, using the default due/defer/planned times); Delete.
Right-clicking a row inside a multiple selection keeps the selection, so the command applies to all of it. Project
rows have Add Inside, Flag, Mark Reviewed, status changes and Inspect.

## Round 15 — Calendar access that works however GTDCraft is opened
macOS asks the *responsible* app for Calendar permission. GTDCraft can be started by another app (a launcher, a
terminal) that doesn't declare Calendar use (no NSCalendarsUsageDescription), so macOS refused silently and no prompt appeared. EventKit work
now runs in "GTDCraft Calendar" (`src/bin/gtdcraft-calendar.rs`, bundle id local.gtdcraft.calendar, LSUIElement), a
small helper app installed into ~/Library/Application Support/GTDCraft and started with `open -W -n -g`, so it is its
own responsible app: it shows the prompt and keeps the permission whether GTDCraft runs from a launcher, the .app,
a terminal or as an agent's `--mcp` server. It is reinstalled only when `HELPER_VERSION` changes, so rebuilding GTDCraft
doesn't reset the permission. Answers come back as a JSON file. Until access is granted GTDCraft re-checks every 15 s.

## Round 16 — sync, iOS, web activity
- Sync (`src/sync.rs`): per-record newest-`modified` wins; deletes are tombstones (`Store::deleted`) that win over
  older edits; each device mints IDs from `device_tag × 2^32` (20-bit tag, IDs stay under 2^53); settings stay per
  device. `gtdcraft --sync-server` merges and returns the result; devices sync every 60 s, 5 s after a save, and on
  Sync Now.
- iOS: eframe draws with wgpu/Metal; menus, file dialogs, the Calendar helper and git stay on the Mac. iOS 27 UIKit
  refuses apps built with its SDK that don't adopt UIScene (TN3187, `_UIApplicationEvaluateRuntimeIssueForNoSceneLifecycleAdoption`
  checks `dyld_program_sdk_at_least`); until winit adopts scenes the iOS binary records SDK 18.5. Under 700 pt wide the
  app shows one pane (the list, or the inspector as a page) with a Settings button.
- Web activity: the Chrome extension posts visits (time on the active tab, minus idle) and searches to
  127.0.0.1:8766; GTDCraft keeps them in web-activity.jsonl and summarizes them in Stats and `gtd_web_activity`.
  Every line in Stats (site, search, video) is a link: clicking opens it in the browser. A video's 320×180 picture
  comes from `i.ytimg.com/vi/<id>/mqdefault.jpg`, fetched once into `thumbs/<id>.jpg` and read from disk after; a
  zero-byte file records "YouTube had no picture", so a deleted or private video isn't asked for again.

## Round 17 — sync rules
| Rule | Value in GTDCraft |
|---|---|
| Same-time edits | The copy already there wins; an incoming record replaces it only if the existing one was modified strictly earlier. On the server the stored copy is the existing one; on a device it's the local copy (`merge_ties` in `src/sync.rs`) |
| Tag links | Kept inside each task's record, so they follow the task's rule above |
| Sync on launch | Yes (first sync due at once) |
| Sync on quit | Yes, one blocking sync of up to 5 s as the app closes (`sync_on_quit`) |
| Delay integration while active | Pulled changes wait while a title, note or field is being edited, then fold in (`user_active`) |

## Round 18 — Calendar permission reset; version label
- Why the dialog never showed: tccd logged `AUTHREQ_PROMPTING … service=kTCCServiceCalendar` and "staged prompting
  request", then waited. macOS shows the Calendar dialog only once the asking app is in front, and since macOS 14 an
  app can't bring itself forward unless the active app yields to it. Now GTDCraft calls
  `yieldActivationToApplicationWithBundleIdentifier:` (local.gtdcraft.calendar) before asking. The helper then becomes a
  regular app, calls `finishLaunching` and `activate`. If the dialog still waits, clicking the helper's Dock icon brings
  it forward.
- Settings › General › Calendar › "Reset Calendar Permission and Ask Again" does four things: stops helpers stuck on a
  request, runs `tccutil reset Calendar local.gtdcraft.calendar` (no admin needed), removes the helper so it is
  reinstalled and re-signed, then asks again. Helper version 4.
- Version: `Cargo.toml` is the one place it lives. build.rs stamps it into the embedded Info.plist, make-app.sh and
  make-ios.sh do the same for the app bundles, and build.rs also stamps the git short SHA. The bottom-right corner
  shows `v<version> · <sha>`; clicking it opens the commit on GitHub. Features bump the minor version, fixes the patch.

## Round 19 — Calendar permission and version numbers
| Rule | Value in GTDCraft |
|---|---|
| What counts as access | Only full access: `authorizationStatusForEntityType:EKEntityTypeEvent == 3`. Write-only or denied count as off |
| When it asks | On its own, as soon as access is needed and isn't full: `requestFullAccessToEventsWithCompletion:`. GTDCraft asks once per launch while access is undecided (`cal_auto_asked`); the button and Reset still ask on demand |
| After the answer | Both event caches are dropped and events refetched; the same happens on EKEventStoreChangedNotification. GTDCraft reads events right after the answer |
| Version numbers | Two: the marketing version (CFBundleShortVersionString; GTDCraft: semver from Cargo.toml) and the build version (CFBundleVersion; GTDCraft: git commit count). Both are stamped into the embedded Info.plist and the app bundles. The corner label shows `v<semver> · <sha>`; its tooltip shows the build |

## Round 20 — the Calendar helper runs AppKit's event loop while asking
Sampling a stuck helper showed it spinning `NSRunLoop runUntilDate:` and never pulling AppKit events, so activation
never completed and the dialog stayed staged. While the dialog is up it now runs
`nextEventMatchingMask:untilDate:inMode:dequeue:` / `sendEvent:` (helper version 5); `lsappinfo` shows it checking in
0.12 s after launch as a Foreground app. The request (`requestFullAccessToEventsWithCompletion:`) is answered on a
completion block, then events are read.

## Round 21 — Quick Entry
- **Window**: transparent, floating above document windows, movable by its background. GTDCraft: the card is an egui
  window dragged by its background, drawn over the app.
- **Frame memory**: both size and place are kept; a restored origin at the zero point means centred. GTDCraft:
  `settings.quick_size` and `settings.quick_pos` (none = centred), written when a drag or resize ends.
- **Kept on screen**: if the window doesn't fit inside the screen it is centred; if it still doesn't fit it is
  resized and centred again. GTDCraft: a saved size is clamped to the app window less 24 pt, and a saved place that
  would leave the card off screen falls back to centred.
- **Minimum width** (inline layout): the width of the project placeholder, the tags placeholder and the
  widest date twice, plus four icon widths plus 99 pt. The widest date is the widest short date (no time) among
  today, yesterday, tomorrow and 2022-12-20. GTDCraft: `quick_min_width` measures "Project Tags D D" in its 12.5 pt
  field font with its 12 pt field glyphs. GTDCraft has only the inline layout.
- **Cancel**: hides the window and discards the edits, so a cancelled entry is thrown away, not kept for next time.
- **Save**: commits editing, fills in pending generated dates, then saves.
- **Unsaved**: any new item that isn't empty (an empty new item doesn't count).
- **Toggle**: hides it if visible, else shows it, adding an empty item at the top if there's none.
- *Not settled:* the minimum height (150 pt) is *estimated*; button title colours follow the theme.

## Round 22 — Quick Entry is its own window
Quick Entry is now a separate macOS window rather than a card inside the main window (see round 21): it floats above
other windows (egui `WindowLevel::AlwaysOnTop`, window layer 3), moves anywhere on screen independently of the main
window, has native edge resizing and a hidden title bar, and stays up when another app is in front. Dragging its
background starts a native window drag (`ViewportCommand::StartDrag`); widgets on top keep their own presses. Its
screen frame (`settings.quick_pos`, `settings.quick_size`, in screen points) is saved whenever it changes, restored on
the next open, and replaced by centred-on-screen if the saved place doesn't fit the main monitor. Escape, ⌘W / close
and Cancel close it. `--screenshot` runs (one window) still draw the card inside the app, centred.

## Round 23 — the Quick Entry window
- **Level**: 8 (NSModalPanelWindowLevel), above floating panels (3). GTDCraft sets its AppKit window to level 8
  (`quick_window_level`); a test run shows window layer 8 while another app is in front.
- **Key and main**: it can become key and main, so it takes the keyboard like a normal window. GTDCraft opens it
  active (`with_active(true)`) with the title focused.
- **Appearance**: light/dark follows the app's effective appearance. GTDCraft's window shares the app's egui context
  and theme, so it follows too.
- **Show**: prepare (add an empty item if needed) → update the minimum width → keep on screen → make key and order
  front (only this window; the main window isn't brought forward).
- **Hide**: ordered out — hidden, not closed, so its frame stays. GTDCraft closes its window and keeps the frame in
  settings, which reopens it in the same place.
- **Toggle / visible**: both go by whether the window is visible.
- *Not yet:* a global Quick Entry shortcut. (Save has no fade: see round 25.)

## Round 24 — version and build number (for the iPhone/iPad build)
These set how every GTDCraft build (Mac, Simulator, iPhone) labels itself.
- **Build number**: the main bundle's CFBundleVersion. GTDCraft's CFBundleVersion is the commit count on every
  platform: `make-app.sh`, `make-ios.sh`, and now `make-ios-device.sh` (`CURRENT_PROJECT_VERSION`, was a fixed "1");
  `build.rs` stamps the same number into the binary.
- **Marketing version**: the bundle's CFBundleShortVersionString. GTDCraft's CFBundleShortVersionString is
  Cargo.toml's version everywhere; the iPhone build's Info.plist now reads `$(MARKETING_VERSION)` /
  `$(CURRENT_PROJECT_VERSION)` and the script passes them (an unsigned Xcode build shows 0.4.0 / 15, matching the
  Mac app's "v0.4.0").

## Round 25 — Save in Quick Entry
- Save doesn't animate: the window closes at once. If saving fails, the window stays open with the error.
- GTDCraft: Save closes Quick Entry immediately (no fade). Saving an item only adds it to the in-memory database,
  which can't fail. (A later failed write to disk is only logged to stderr — "gtdcraft: save failed" — not shown in
  a dialog.)

## Round 26 — pairing a phone with a QR code; Stats on phones
- Settings › General › Sync on the Mac shows **Pair a phone**: a QR code of `gtdcraft:///setup-sync?server=<url>&token=<secret>` (was `gtdcraft://pair`; see round 27)
  (percent-encoded; `localhost` is swapped for the Mac's Bonjour name). The iPhone's Camera offers "Open in
  GTDCraft"; iOS hands the link to the app delegate's `application:openURL:options:` (winit sets no delegate, so
  GTDCraft installs one), which fills Server and Token and syncs at once. **Copy Link** / pasting the link into the
  Server field does the same. On a Mac running the launchd server (`local.gtdcraft.syncserver`), **Use This Mac's
  Server** fills both fields. iOS declares the `gtdcraft` URL scheme and NSLocalNetworkUsageDescription.
- Stats below 560 pt wide (phones): tiles two per row, the donut's legend under the ring (it
  needs 400 pt beside it), Time by project and Time by tag stacked, and date labels at least 38 pt apart.

## Round 27 — setup-sync links; empty new databases; deleting projects
- **Setup-sync link**: the QR link is `gtdcraft:///setup-sync?server=…&token=…`; opening it shows "Replace GTDCraft
  Settings?" ("You have clicked on a GTDCraft settings link. Replace your GTDCraft synchronization settings?") with
  Cancel / Replace Settings, and only Replace Settings fills Server and Token and syncs. Cancel shows no further error.
- **New databases start empty** (`Store::load_or_new`); the sample is optional: Settings › General › Sample Items ›
  Add Sample Items (only while the database is empty). **Remove Sample Items** (or `gtdcraft --remove-sample`)
  deletes sample actions matched by name and project, then sample projects and tags with nothing of yours left in
  them, with tombstones so the delete reaches every device. Sync now drops Pomodoros whose task was deleted.
- **Delete projects and folders**: project rows (outline and sidebar) have Delete next to Complete / Drop / On Hold;
  folder rows have "Delete Folder and Its Projects". Deletes take the project's actions too and sync as tombstones
  (Drop keeps them; Delete is for projects that shouldn't exist). Edit › Undo brings them back.
- **iPhone safe areas**: the strips under the status bar, the home indicator and the notch sides are painted in the
  toolbar, page and rail colours, so the app fills the screen (no black bars).

## Round 28 — Delete
- Delete removes the selection without asking; Undo is the way back. There is no "delete project?" or "delete
  folder?" confirmation.
- GTDCraft: Delete on a project (row menu, sidebar menu, or ⌘⌫ with the project selected) goes through the same
  Delete command as actions: no confirmation, its actions go with it, Undo restores it, and the **previous row** is
  selected afterwards. Folder rows: "Delete Folder and Its Projects".

## Round 29 — iPhone/iPad Settings and Calendar
- Settings on iPhone/iPad is a full page over the app, not a floating window: a large "Settings" title, one grouped
  block of 44 pt rows with › (General, Calendar, Sync, Appearance, Perspectives, Notifications, Keyboard), and a blue
  ✓ that closes it; a row opens its page with ‹ back and the page name centred. It covers the whole screen. The Agent
  page (MCP and ACP agents run on a Mac) and Mac-only lines (database path, Open Themes Folder, the Calendar helper
  and Dock hints, macOS wording) don't appear there.
- Calendar on iPhone/iPad asks EventKit directly (`ek_access` / `ek_request` / `ek_events` / `ek_calendars`); the Mac
  keeps the GTDCraft Calendar helper. iOS shows its own prompt with the Info.plist text; if access is off,
  "Open Settings" opens GTDCraft's page in the Settings app (`app-settings:`).
- Reminders aren't delivered as iOS notifications yet (the Mac uses Notification Center through osascript); the
  Notifications page says so on iPhone/iPad.

## Round 30 — the Settings pages and their order
- GTDCraft's pages, in this order on the Mac and iPhone (`SETTINGS_PAGES`): General (sample items; the database
  path on the Mac), Organization (review interval, hiding completed items), Appearance, Perspectives, Dates & Times
  (due soon, default times), Notifications, Calendar, Sync, Keyboard, and Agent on the Mac only. The iPhone home
  page ends with "N projects; M actions · GTDCraft <version>". No Layout, Automation or Reset pages yet.

## Round 31 — notifications through UserNotifications; References (GitHub, email, Messages); v0.5.0
- **Notifications** now go through Apple's UserNotifications framework (`objc2-user-notifications`): they arrive as
  GTDCraft (`local.gtdcraft`) in Notification Center, with GTDCraft's icon, not as Script Editor (the old osascript
  path, kept only for the bare `target/release` binary, which has no app bundle for UserNotifications to use). A
  delegate shows banners while GTDCraft is in front too. Permission is asked once (alert + sound). Checked on this
  Mac: a reminder due 15 s ahead was delivered and recorded by Notification Center under `local.gtdcraft`.
- **iPhone/iPad reminders** are scheduled with iOS ahead of time (the next 7 days, at most 64 — iOS's limit, as
  `UNTimeIntervalNotificationTrigger`s), rescheduled whenever the set changes, so they arrive while GTDCraft is
  closed. Same rules as the Mac (`alarms()`: due, latest start = due − estimate, defer, planned).
- **References** (`refs.rs`): tasks and projects carry a list of links, shown in an inspector section
  "References" (orderable like the others). A link's kind comes from its URL — GitHub repo / issue / pull request,
  Gmail or Mail (`message:`) email, Messages (`sms:` / `imessage:`), or any web page — so another service is one
  more case. GitHub links get their title and state from the REST API (open, closed, merged, draft; repos their
  description); a token is optional (Settings › Agent › GitHub, or GITHUB_TOKEN) and needed only for private repos.
  Clicking a reference opens it in its app; × removes it; `owner/repo#12` and `github.com/…` are accepted. They sync
  with the task or project.
- Version 0.5.0 (Cargo.toml; the iPhone build takes it from there).

## Round 32 — reminder buttons (Complete, Snooze) and snoozing; v0.6.0
- Reminders have two actions: "complete" (title "Complete", needs the device unlocked) and "snooze" (title "Snooze").
- Snooze: the next whole minute after now, plus the snooze delay (15 minutes by default), as a snoozed reminder.
- GTDCraft: reminders for an action carry the category "task-alarm" with **Complete** (identifier "complete",
  needs the device unlocked) and **Snooze** ("snooze"). Snooze sends the same reminder again at the next whole
  minute plus 15 minutes (`places::snooze_delay`), handled in the notification delegate so it works even if
  GTDCraft isn't in front; Complete completes the action; tapping the reminder selects it and opens its inspector.
  Checked on this Mac: the delivered reminder is recorded with category `task-alarm` and its action in the
  identifier.

## Round 33 — folders of their own; reordering by touch; perspectives and sections sync; v0.7.0
- **Folders** are records (`Store::folders`: id, name, modified) besides the folder name each project carries, so a
  folder can be empty. They sync like tags (by id, newest wins, deletes as tombstones). Projects sidebar: right-click
  a folder for New Folder, Rename Folder…, Delete Folder and Its Projects; right-click the space below the list for
  New Project and New Folder. Organize › New Folder makes an empty folder and asks for its name (it used to make a
  project inside a folder). Renaming moves every project in it.
- **Reordering by touch** (Settings › Perspectives, Rearrange Sections): the drag remembers where the finger was,
  because a lifted finger ends the pointer in the same frame as the release (it used to drop back where it began);
  iPhone/iPad rows also get ↑ ↓ buttons.
- **Perspectives and the inspector's section order sync** between devices: `Settings::layout_modified` is stamped on
  save when either changed on this device, and the newer layout wins in the merge. Everything else in Settings
  stays per device (sync server and token, API keys, places, themes).

## Round 34 — adding a folder; v0.7.1
- Organize › Add Folder beeps when a folder can't be added here; shortcut menus offer "Add Folder" unless the clicked
  rows rule it out.
- A new folder goes into the selected item's folder right after the selected project; with nothing to go after, it
  goes at the top.
- GTDCraft: folders keep the order they were placed in (it used to sort them by name); a new folder goes right after
  the folder of the selected project or action — or after the folder whose menu you used — and at the top otherwise.
  GTDCraft folders don't nest, so "into that folder" becomes "next to it".

## Round 35 — iPhone portrait bottom bar; iPhone app icon; v0.8.0
- iPhone held upright: no perspective rail. A rounded bar sits at the bottom (`tab_bar`, 132 pt): a row of commands
  — Quick Open (⚡ over a card), Search, New Action (+), Quick Entry (inbox with +) — over four perspective tabs,
  the first four shown in the sidebar, each with its icon, name and (Inbox) badge; the current one on a lighter
  pill. iPad and landscape keep the rail. The version label moves out of the way (Settings shows
  it). Still to match: the slim top bar (sidebar · eye · ⋯), the bar floating over the list, and a full-screen Quick
  Entry sheet.
- iPhone app icon: an asset catalog (ios/Assets.xcassets, AppIcon 1024 pt, no alpha), cut from the Mac icon's tile so
  it fills the square iOS rounds; the simulator build uses the same picture.
- `GTDCRAFT_NO_NOTIFY` (environment): no notification setup, for simulator and screenshot runs.

## Round 36 — favourite perspectives fill the iPhone bar; v0.8.1
- Perspectives have a favourite flag (Settings › Perspectives ☆/★). Default favourites: Inbox, Projects,
  Tags, Forecast, Flagged, Nearby, Review (`PERSPECTIVE_FAVORITES`). The iPhone bottom bar's tabs are the favourites
  in sidebar order, four across with their names; more scroll sideways, and opening a perspective scrolls its tab
  into view. Favourites sync with the rest of the perspectives.

## Round 37 — iPhone portrait: top bar, perspectives page, Quick Entry sheet; v0.9.0
- Top bar (`toolbar_phone`): round 40 pt buttons — ▯ opens the perspectives page, the eye opens View Options; at the
  right ⓘ (only when something is selected) and ⋯ with Clean Up, Go Back, Sync and Settings. Search, from the bottom bar, takes the middle as a field with Cancel; otherwise the clock / Pomodoro pill.
- Perspectives page (`perspectives_page_ui`): ‹ back, a large "Perspectives" title, every shown
  perspective with its icon and ☆/★ (a tap opens it; the star makes it a favourite for the bottom bar).
- Quick Entry: a full-screen sheet with Cancel · eye · Save at the top and Project, Tags, Due and
  Repeat one per line, at phone size (×1.15; chips ×1.32), instead of the Mac's card.

## Round 38 — Clean Up, Expand/Collapse All, processing the Inbox; v0.9.1
- Clean Up cleans the visible view now and marks the others to be cleaned when they're next shown; it does nothing
  while automatic cleanup is turned off.
- After a completion, edits are pushed, filtering is refreshed, and every window is cleaned up.
- Inbox processing: an Inbox item needs a project to leave the Inbox — an Inbox item with a project stays in the
  Inbox until Clean Up files it.
- Expand All / Collapse All validate first (else beep), then expand or collapse every row, recursively.
- GTDCraft: an Inbox item given a project stays in the Inbox (`inbox_kept`, `track_inbox`) until Clean Up — by hand
  (⌘K, the iPhone ⋯ menu) or automatically when completed items are tidied away. The iPhone ⋯ menu gains Expand
  All and Collapse All (every action with sub-actions, every project and folder heading).

## Round 42 — connections sync; nested folders; a repo's open issues and PRs; Linux notifications
- **Connections sync** like the layout: `Settings::connections_modified` is stamped on save when the Linear key, GitHub
  token or AI provider/key/model/server changed here; the newer set wins in the merge. They travel with the database
  to your own sync server (stored there as on each device). Device-specific settings stay put (sync server, token,
  places, the agent command). iPhone/iPad Settings gains a **Connections** page for them.
- **Nested folders** are named like nested tags: "Work : Clients" is Clients inside Work (`FOLDER_SEP`). The sidebar
  lists them as a tree (`folder_names`: each folder, then the ones inside it; parents that only exist as a prefix are
  listed), indented, showing their own name; a folder's menu has New Folder Inside; renaming or deleting a folder
  takes what's inside along (`in_folder`).
- **GitHub**: a repo reference has **Issues & PRs** — its open issues and pull requests (`GET /repos/{o}/{r}/issues
  ?state=open&per_page=30`, drafts marked), each with + to link it.
- **Linux** notifications go through `notify-send` (libnotify). (The Linux build itself needs the Apple-only crates
  gated to Apple targets first — see ROADMAP.)

## Round 43 — folder status and names
- A folder is Active or Dropped, nothing else. Dropping a folder drops what's inside it, including its subfolders.
- New folders are named "Untitled Folder".
- GTDCraft: folders have Drop Folder / Mark Folder Active (sidebar folder menu); a dropped folder shows "(dropped)",
  its projects leave the active list, and every action in them — at any depth — is Dropped. New folders are named
  "Untitled Folder" (was "New Folder").

## Round 41 — iOS extensions: widgets, Share, and sync server endpoints

### What each extension does
- **GTDCraftWidgets** (`ios/Widgets/GTDCraftWidgets.swift`): a WidgetBundle with four members:
  - *Focus / Up Next* (medium, large): the current focus task (Pomodoro or clocked-in or first in Up Next), its project,
    and the next items in the queue.
  - *Due Today* (small, medium): a count badge and a list of actions due today.
  - *Time Tracked* (small, medium): time tracked today and this week (from clock entries).
  - *Quick Entry* (iOS 18+ Lock Screen `ControlWidget`): a button that opens GTDCraft. Gated behind `@available(iOS 18.0, *)`;
    absent gracefully on iOS 17.
- **GTDCraftShare** (`ios/ShareExtension/ShareViewController.swift`): a Share extension receiving text or URLs from other
  apps. Shows a compose sheet with a task name, the shared URL as a Reference, and server/token fields (saved in the
  extension's own `UserDefaults` on first use). Submits via `POST /v1/inbox`.

### How data flows
A free Apple developer account was first thought unable to use the App Groups capability, so the main app and its
extensions cannot share files or `UserDefaults` through a shared container. Data flows through the sync server instead:
1. The Rust sync server gains two endpoints:
   - `GET /v1/widget` — returns a JSON summary computed from the stored database: focus task, Up Next (5), due today
     (count + 10 names), time tracked today and this week (minutes), active Pomodoro flag.
   - `POST /v1/inbox` — creates a new task in the inbox with optional `name`, `note`, `ref_url`, `ref_title`. Returns
     `{"ok": true, "id": <new_id>}`.
   Both are Bearer-token protected (same token as `POST /v1/sync`).
2. Each widget is configured through `AppIntentConfiguration` (iOS 17+): the user enters the sync server URL and token
   in the widget's edit screen. The timeline provider fetches `GET /v1/widget` every 15 minutes.
3. The Share extension stores server URL and token in its own `UserDefaults`, entered on first use (a text field in
   the share sheet).

### URL scheme
- `gtdcraft:///add?name=…&note=…&ref_url=…&ref_title=…` — the app's `take_opened_link` handler creates an inbox item.
  Used by the Quick Entry Lock Screen control (opens the app) and available to Shortcuts.
- `gtdcraft:///setup-sync?server=…&token=…` — existing pairing link, unchanged.

### Limits of a free Apple account
- **No App Groups**: the app and its extensions cannot share a file container or `UserDefaults`. All data exchange goes
  through the network (the sync server). This means the sync server must be running for widgets and the Share extension
  to work. A paid account ($99/year) would enable App Groups, letting the Rust app write a JSON snapshot that extensions
  read directly from disk — no server needed.
- **No Keychain Sharing**: each target gets an isolated keychain, so credentials can't be shared between the app and
  extensions through the keychain either.
- **No Push Notifications**: widgets can't be refreshed on demand via push. They update at most every 15 minutes
  (WidgetKit's budget). A paid account enables push-based timeline reloads.
- **7-day expiry**: builds installed on a physical device expire after 7 days; `./make-ios-device.sh` renews them.
- **Live Activity**: requires ActivityKit called from the main app process. Since the app is a Rust binary, calling
  ActivityKit needs either (a) building the Rust code as a staticlib linked into a Swift app, or (b) a small Swift
  static library exposing C functions called from Rust via `extern "C"`. Both restructure the build. Deferred.
- **Mac widgets**: WidgetKit on macOS requires a sandboxed app bundle. GTDCraft's Mac build is unsandboxed, so Mac
  widgets need an Xcode project for the Mac target. Deferred.

## Round 39 — file links in notes and Attachments inspector section
- Notes already detect web links (`is_url`) on their own line and render them as rich link cards. New: `is_file_link`
  detects `file://…` URLs, `~/…` paths, and absolute paths with known prefixes (`/Users/`, `/Volumes/`, `/Applications/`,
  `/Library/`, `/System/`, `/home/`, `/tmp/`, `/var/`, `/etc/`, `/opt/`, `/usr/`). Trailing punctuation (`.`, `,`, `;`)
  is stripped. Detected file links render as a clickable row: folder/paperclip icon (directory check via `Path::is_dir`
  on the expanded path), filename, and parent path. `~/…` paths show the unexpanded parent. `expand_path` handles
  `file://` (percent-decoding `%20`) and `~` expansion (via `$HOME`). `open_file` uses `open` on macOS, `refs::open`
  with a `file://` URL on iOS.
- Attachments: a new inspector section (key `"attachments"`) for tasks and projects. Each record gains
  `attachments: Vec<String>` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`; changes set `modified`.
  Section added to `INSPECTOR_SECTIONS` (between Linear and References), `TASK_SECTIONS` (13 entries) and
  `PROJECT_SECTIONS` (8 entries). The section shows each attachment as a row (icon + name + parent, click opens, ×
  removes), "Add File or Folder…" / "Folder…" buttons using `rfd::FileDialog` (macOS only, `cfg(not(target_os = "ios"))`),
  and a paste field for a path or `file://` URL on all platforms. `normalize_attachment` converts `file://` URLs to
  paths (with `%20` decoding). On iOS, `rfd` is unavailable; only the paste field is offered (the iOS Files picker via
  `UIDocumentPickerViewController` is still to do).

## Round 40 — Nearby on Apple's maps and location
- **Map**: on macOS and iOS the Nearby map uses Apple MapKit (`MKMapSnapshotter`) instead of OpenStreetMap tiles.
  A snapshot is taken for the visible region (dark appearance on macOS via `NSAppearanceNameDarkAqua`, muted emphasis
  on iOS), uploaded as an egui texture, cached by quantised region key (~100 m granularity). GTDCraft's own pins and
  radii are drawn on top using the existing Web Mercator projection. OSM tiles remain the fallback on Linux/Windows
  and if snapshotting fails. The Apple Maps snapshot includes its own legal attribution text; the OSM credit line is
  hidden when Apple Maps is active.
- **Location**: Core Location (`CLLocationManager`) provides the current location ("here") on macOS and iOS.
  Authorization is requested once at launch (`requestWhenInUseAuthorization`); location is polled with
  `requestLocation` and the delegate updates a global `(lat, lon)`. The app's `settings.here` is updated each
  frame when the location changes by more than ~100 m. `GTDCRAFT_NO_NOTIFY` suppresses location (screenshot mode).
  Info.plist keys: `NSLocationWhenInUseUsageDescription` (iOS and Mac), `NSLocationUsageDescription` (Mac).
  Linux: no GeoClue integration yet; location is set by search as before.
- **Location notifications (iOS)**: `schedule_location` registers `UNLocationNotificationTrigger` with
  `CLCircularRegion` (notify on entry, repeating) for the nearest 20 places from "here". iOS monitors at most ~20
  regions concurrently. Each notification uses the existing `task-alarm` category (Complete / Snooze buttons).
  Notifications are identified by `gtdcraft-loc-<task_id>` so they replace on re-schedule.
- **Region monitoring on Mac**: not implemented (CLLocationManager region monitoring is deprecated on macOS;
  the Mac app fires time-based reminders only).
- **Limits**: snapshot cache is in-memory only (one pending snapshot at a time, debounced by cache-key quantisation).
  iOS region limit is 20 (iOS system limit); places beyond that distance get no geofence notification.

## Round 44 — the Focus Live Activity; v0.10.0
- Starting a Pomodoro or clocking in on iPhone starts a Live Activity (Lock Screen and Dynamic Island): the action's
  name and a timer — counting down to the Pomodoro's end, or up from the clock-in — that the system keeps ticking.
  Stopping ends it. The app is the Rust binary, so ActivityKit is reached through a small Swift static library
  (ios/LiveActivity/GTDLive.swift, `gtd_live_start` / `gtd_live_end`) that build.rs compiles and links on iOS; its
  attributes type (ios/Shared/GTDFocusAttributes.swift) is shared with the widget extension, which draws it
  (`FocusLiveActivity`). Info.plist: NSSupportsLiveActivities. Checked on the iPhone simulator: the request is
  accepted ("GTDCraft Live Activity started: …"); the picture itself is drawn by the extension on a device.
- `--press` / `--type` now work outside screenshot mode (the frame counter runs every frame).

## Round 47 — iPhone keyboard, Report a Bug, Apple platforms first
- **Keyboard** (iPhone/iPad): UIKit's `UIKeyboardWillChangeFrameNotification` (`UIKeyboardFrameEndUserInfoKey`)
  gives the keyboard's height; the content area ends above it (a spacer panel), the bottom bar steps aside while
  typing, Settings pages end above it, and the field being edited is scrolled into view (centred) when the keyboard
  would cover it — iOS's usual behaviour.
- **Report a Bug** (Help › Report a Bug… on the Mac; ⋯ › Report a Bug on iPhone): opens GitHub's new-issue page for
  GTDCraft with the `bug_report` issue form (.github/ISSUE_TEMPLATE/bug_report.yml) and its Environment field filled
  in from URL parameters — version, build, commit, OS and device, the screen you were on. No token or library: your
  browser's (or the GitHub app's) sign-in is used. The same text goes in `body` for when the form isn't on the default
  branch yet.
- Linux work moved to GitHub issues #1–#3 (build, GeoClue location, notify-send); this effort is Apple platforms.

## Round 45 — tag locations: Apple Maps search and the iPhone location editor
- **Search is Apple's** on Mac and iPhone/iPad: `MKLocalSearch` with the typed query, its region a 0.5° box around the
  device's location (`cl::current()`), else the saved "My location" (`places::search_bias`). Every result is listed —
  name in bold, address under it — and the person picks one; before, Nominatim's first hit was taken. Addresses are
  built from the result's placemark as the iPhone card shows them: "number street" / "city state postcode" / "country"
  (`places::address_lines`). Nominatim (8 results, `display_name` as the address) stays the search on other platforms,
  and is tried when Apple Maps errors or finds nothing. Both completion handlers run on the main queue: the app calls
  them from a background thread while eframe's event loop keeps the main queue running. `MKMapItem.placemark` and
  `CLGeocoder` are deprecated in macOS/iOS 26, but their replacements (`MKMapItem.address`,
  `MKReverseGeocodingRequest`) don't exist on iOS 17, the oldest GTDCraft supports.
- **The Location card** (tag inspector, Mac and iPhone): the place's name (when it's a business, not the tag's name or
  the street again), its address on several lines (coordinates when there's none), and a red ✕ that clears it.
  "Change Location" ("Add Location" when there's none) opens Here, Anywhere, Search, Address, Pin (no Contact
  option):
  - Here — the Core Location fix (else "My location"), then `CLGeocoder reverseGeocodeLocation` fills the address.
  - Anywhere — clears the place.
  - Search — a query field and the results list. Address — the same with `resultTypes = .address`.
  - Pin — goes to Nearby with the tag selected; clicking (tapping) the map drops it there, and the address is looked up.
  A new spot keeps the tag's Notification and Distance (`Place::relocated`).
- **Notification**: None / When Arriving / When Leaving (`Place.notify`, serialized `"none" | "arriving" |
  "leaving"`, synced with the tag). Places saved before this round have no field and load as When Arriving, which is
  what they did before. On iPhone/iPad the geofence's `CLCircularRegion` gets `notifyOnEntry` / `notifyOnExit`
  from it, and None places get no geofence. The pending requests are now handed to iOS only when the alarms, the places
  or "here" change, instead of every second (which re-armed every geofence). The Mac shows "Location notifications
  arrive on iPhone and iPad."
- **Distance**: small / medium / large = 200 m / 500 m / 10 km (medium was 1 km before this round, see round 3),
  labelled "~650 ft" / "~¼ mi" / "~6¼ mi" (miles where the locale doesn't use metric, `NSLocale.usesMetricSystem`) or
  "~200 m" / "~500 m" / "~10 km". Older places at 1000 m (or
  unset, the 1 km default) show as medium (`Place::tier`, nearest by ratio) until a size is picked.
- `Place.address` is new (empty = not looked up; not written when empty).
- Checks: `gtdcraft --find-place coffee --near 45.52,-122.68` runs the search and a reverse lookup once from the
  command line and prints the results (it spins the main run loop for MapKit's answer); the ignored test
  `mapkit_search_finds_coffee_in_portland` runs it. Screenshots: `--view tags --select office` shows a tag's inspector,
  `--place-search coffee` adds the results list.

## Round 46 — iPhone: the bar shrinks on scroll; landscape
- Layout by the safe area's size (`phone_layout`): iOS, upright and narrower than 700 pt → portrait (bottom bar);
  iOS, sideways and shorter than 600 pt → landscape; anything else (iPad, Mac) → the Mac panels as before.
- Portrait, scrolling: the large title and count line scroll with the list (they're the list's first row
  now); once the list has scrolled 44 pt the title fades in small, centred in the top bar. The bottom bar follows
  iOS's tab bar "minimize on scroll down" as the video shows: scrolling down past 44 pt shrinks it to a round
  button with the perspective's icon at the left and a capsule with + and Quick Entry at the right; scrolling up
  (6 pt or more) or reaching the top brings it back, and so does tapping the round button (`bar_collapsed`). The
  two forms cross-fade and slide over 0.25 s (egui `animate_bool_with_time`).
- Landscape: no window toolbar. The rail at the left as on iPad; the sidebar (about 4/9 of
  what the rail leaves, 241–320 pt) heads itself with ▯ (hide it), ☰ (the perspectives page) and Edit (the
  inspector for the selected project or tag; dimmed otherwise, as in Forecast) and has a floating round + where
  there's something to add (projects, tags). The list has its own top bar — ▯ only while the sidebar is hidden (or
  the perspective has none, then the perspectives page), the eye, the title in the middle, i and ⋯ in one capsule
  (i dimmed with nothing selected), a blue ✓ while an action's name is being typed — and floating capsules at its
  bottom corners: ⚡ Quick Open and Search at the left, + and Quick Entry at the right. No inspector column: the
  inspector opens as a page with ‹ in the list's place. Quick Entry is the full-screen sheet, as upright.
- `--landscape` (test hook, iPhone): turns the app sideways on launch through `UIWindowScene
  requestGeometryUpdate` (landscape right), since `simctl` can't rotate the simulator; `--scroll N` now also
  scrolls the list, so screenshots can show the shrunk bar.

## Round 48 — the Report a Bug link
- `gtdcraft:///report-bug` (exact path, a trailing / allowed) opens Report a Bug on iPhone/iPad
  (`sync::is_bug_link`), so a Shortcut or note can link to it. Reporting goes through the GitHub issue form
  (Round 47); there's no mail address.

## Round 49 — Attach File… in the row's shortcut menu (the person: "where is add attachment… a path to a folder")
- Attach File… is offered only when every clicked row is a project or task, and acts on the window's selection.
- GTDCraft: task and project rows (list and sidebar) have **Attach File…** and **Attach Folder…** (egui's picker can't
  offer files and folders in one panel); the chosen paths go on the item's Attachments, deduplicated, and the item is
  selected so the inspector shows them. On iPhone/iPad the items open the inspector, where Attachments takes a pasted
  path. The inspector's own buttons are now "Add File…" and "Add Folder…" (the first only ever picked files).

## Round 50 — attaching to several selected rows
- GTDCraft: Attach File…/Folder… on a row inside the selection attaches to every selected row (the clicked one first);
  on a row outside it, that row alone becomes the selection. GTDCraft attaches a link to the file, not a copy, so the
  database doesn't grow and there's no need to confirm attaching to several items (`attach_to`, tested). Paths an
  item already has aren't added twice; the item's modified time moves only when something was added.

## Round 51 — dates on iPhone, the inspector's width, the bar's spring, Forecast countdowns
- **Typed dates are kept when editing ends** (the person: Return didn't submit, tapping out reverted). Return, Tab and
  clicking away all save; Cancel doesn't. GTDCraft: a date field that was typed in saves what it reads as a date
  whenever focus leaves it (`date_dirty`); a soft keyboard's Return arriving as a newline ends editing too.
  Unreadable text goes back to the saved date.
- **iPhone/iPad date fields open iOS's date picker** in a sheet: title
  "Defer/Planned/Due Date", Done, `UIDatePicker` (date and time, inline style) and a momentary segmented control
  Today · +1d · +1w · +1m · Clear (`datepick.rs`). Changes apply as they're made, so swiping the sheet down keeps
  them. The quick buttons' rule is shared with the inspector's own row (`datepick::quick`: Today keeps the time, the
  others add to the date or to today at the default time). Not yet: a typed-date field at the top of the sheet.
- **Inspector width**: the inspector capped its content at the Mac panel's width (`INSPECTOR_W - 36`), leaving the right
  third of an iPhone page empty; the page now uses the screen's width.
- **The bar's shrink and grow**: one shape — the bar's rounded block splits into the round
  perspective button and the + capsule — rather than two bars cross-fading and sliding. 0.42 s, eased with a spring
  that overshoots about 4% in the direction it's moving (`liquid`, tested: exact ends, no jumps, 1–8% overshoot);
  contents fade over the first/last 60%.
- **Forecast countdown** (the person: "time until planned … then how long left in the time block, without setting due
  dates"): a planned action's row shows "in 2h 10m", then "in 12:34" in the last hour, ticking; from its planned time
  until its duration has passed, "12:34 left of 45m" in an accent pill. Planned date + duration is the block — no due
  date or calendar event needed. Nothing more than a day ahead, after the block, or past the planned time without a
  duration (`block_countdown`, tested).

## Round 52 — the person's Artcraft list (UX bugs logged in GTDCraft itself)
- **Copy as List / Copy as List with Context** (the person: "multi selected menu should offer copy tasks as list…
  with context"). A selected child of a selected parent goes once, under it; only visible rows are copied.
  GTDCraft: "Copy as List" = "- name" lines, sub-actions tab-indented, notes indented beneath;
  "with Context" adds @project (top items), @tags(…), @defer/@planned/@due(YYYY-MM-DD HH:MM), @estimate(Nm),
  @flagged, @done(…) (`copy_list`, tested). In the row menu (whole selection when the row is in it) and Select mode's
  bar; Mac pasteboard via `pbcopy`, iOS via UIPasteboard; a "Copied N items" toast.
- **Select mode on iPhone** ("multi select not working": there's no ⌘ on a phone): ⋯ › Select; taps pick rows; a bar
  in the tab bar's place shows "N Selected" · Done over Copy, Copy+Context, Flag, Complete, Delete (each acts on all
  picked rows, as the Mac's menu commands do).
- **The row menu on iPhone** ("organize menu buggy as hell on mobile": its submenus open on hover): touch and hold a
  row for a sheet from the bottom listing every command, with Type, Organize and Schedule under headings instead of
  submenus, 44 pt rows, scrolling when taller than 72% of the screen. One list (`row_menu_items`) feeds both it and the
  Mac popup (which keeps its submenus and separators).
- **New items stay in the list on iPhone** ("should not always open inspector on new item create, jarring"): New
  Action and Add Inside close the inspector page and edit the name in its row; the row scrolls to the middle when the
  editor opens ("did not scroll new item into view"). Tapping a selected row's note edits it **in the row** (saved when
  focus leaves or another row is tapped) rather than switching to the inspector.
- **Planned reminder → Clock In** ("notification for planned item – would you like to clock in?"): reminders gain a
  **Clock In** button (UNNotificationAction "clockin", opens the app) beside Complete and Snooze; the planned-time
  reminder reads "Planned for now (45 min) · Clock in?". Clock In selects the action and starts its clock (and Live
  Activity) unless it's already running.
- **Tags in iPhone portrait** ("no way to display tags list"): a sideways-scrolling row of chips under the title —
  All, every tag in sidebar order, Untagged — filtering like the sidebar.
- **The eye on iPhone** ("doesn't seem to do anything"): View Options was placed under the Mac toolbar's eye, which
  the iPhone bar never set, so it opened off-screen; it now opens under the iPhone's eye, kept on screen and narrowed
  to fit.

## Round 53 — finding a tag from typed text ("agent:review")
- The person asked for the addressed Artcraft items to be tagged `agent:review` (meant as nested). Done through
  GTDCraft's MCP `gtd_update_task` (each item's existing tags plus the new one), which made the tag `agent` › `review`.
- Lookup rule: a primary key first; else the input is normalised and split on ":"; a tag whose whole hierarchical name
  (components joined with " : ") equals it wins at once; otherwise tags whose own name equals the last component,
  ignoring case, are counted, and one is used only when there's exactly one; failing that, the path can be created.
- GTDCraft's `Store::tag_by_name` now follows it: whole path first (case and spaces around ":" ignored), then the
  single tag with that own name — none when several share it (it used to take the first, at random when two nested
  tags had the same name); `tag_for_path` still creates the path when nothing matches. Tested.

## Round 54 — the person's second Artcraft list (16 bugs)
- **Planned dates wouldn't save** from the row's field strip on iPhone: its typed popover saved only on Return (which
  the iPhone keyboard didn't deliver) and threw the text away on a tap elsewhere. On iPhone/iPad the strip's
  Defer/Planned/Due now open iOS's date sheet (`datepick`); everywhere, the popover keeps a typed date or duration
  that reads when you tap away, as ending editing does elsewhere (Round 51), and a soft keyboard's newline counts as
  Return.
- **Typing on iPhone** (autocorrect not applied, hold-backspace not deleting by word, no Select All / Copy / Paste on
  the server URL): winit 0.30.13's iOS view adopts only UIKeyInput (`platform_impl/ios/view.rs` 335–353: each
  `insertText:` becomes key presses, `deleteBackward` a plain Backspace, no IME), so iOS's text system never reaches
  egui. `nativetext::Edit` (every text field) is egui's TextEdit on the Mac; on iPhone/iPad, while a field is being
  edited, a real UITextField (one line) or UITextView (several, unscrolled, no insets) sits exactly over it — iOS's
  own autocorrect, predictions, edit menu, cursor and delete acceleration — and its text is copied back every frame.
  Return ends editing and reaches egui as Enter (`raw_input_hook`); a tap elsewhere or the field leaving the screen
  removes it. URL fields (sync server, links, paths) don't autocorrect or capitalise and get the URL keyboard.
- **Notes grow** instead of spilling out of their row: the in-row note editor's height is its wrapped text's.
- **Keyboard over the tag/project picker**: with the keyboard up, the field popover sits on top of it (bottom-pivoted,
  its height from the last frame) and its list shortens to fit.
- **Most recently used first** in the project and tag pickers until you type: by the newest change to an action in
  the project or with the tag (`Store::by_recent_use`, tested) — derived, so it syncs.
- **Projects and tags lists in portrait**: chips for Projects too, and ☰ opens the sidebar's vertical list as a page
  (‹ back); picking one shows its items.
- **Undo / Redo buttons** in the iPhone top bar once there's something to undo.
- **Notifications**: Settings › Notifications on iPhone shows whether iOS allows them (`getNotificationSettings…`),
  with Open iOS Settings when it doesn't, and drops the stale "don't arrive yet" text. New: "Also remind me before
  planned and due times" (5 min – 1 hour), adding "Planned in 15 min" / "Due in 15 min" reminders.
- **Forecast time blocks**: the countdown's phase has its own colour — ahead (dim), starting within 5 minutes (green),
  in it (accent), ending in its last 5 minutes or fifth (orange) (`BlockPhase`, tested). The day timeline shows
  planned blocks with calendar events, a block's start edge green and end edge orange, **across** the day or **down**
  it (View Options › Day timeline: Horizontal / Vertical; the vertical one shows 8 AM–6 PM at least, events left and
  blocks right). **All-day events** are listed but no longer fill the timeline unless "All-day events block time".
- **Attachments on iPhone/iPad**: Add from Files… (UIDocumentPickerViewController, any file, copied in) and Add
  Photo… (UIImagePickerController) save to Documents/Attachments under a name that doesn't clash (`unique_name`,
  tested); tapping one previews it with Quick Look.
- **Widgets and the Share extension use the app's server**: the app writes its sync URL and token to the App Group
  (`GTD_APP_GROUP` in `ios/Config`; free accounts allow it, per Apple's capability table); the widget and Share extension fall back to it when their own fields are empty.

## Round 55 — naming reminders before a time
- Heads-up reminders (Round 54) are labelled by their offset: the notification reads "Before Due: 15 Minutes" /
  "Before Planned: 15 Minutes", the inspector's Notifications list shows "Before Due (15 Minutes): Today, 4:45 PM",
  and Settings offers Off · 5 Minutes · 10 Minutes · 15 Minutes · 30 Minutes · 1 Hour. The interval is written in
  its largest whole unit, capitalised, plural past one (`interval_label`, tested: 5 Minutes, 1 Hour, 90 Minutes,
  2 Hours, 1 Day).

## Round 56 — Send to Agent (the person's spec)
- **Opt-in on the sync server**: `--agents`, or Settings › Sync › Agents on the Mac that runs it (writes
  `agents-enabled` in the server's data folder; no restart). Needs at least one Agent Client Protocol agent installed
  there (`acpfind`: Claude Code, Codex, Gemini CLI, goose, Hermes, Copilot, OpenCode, Kimi…). `GET /v1/agent/agents`
  → {enabled, agents}; devices show Send to Agent… only when it's on and there's an agent.
- **A thread** (`POST /v1/agent/threads {task, agent, request, from?}`): the server copies its database to a staged
  copy and starts the agent (`/bin/sh -c <command>`, login PATH) with ACP `initialize` → `session/new` (cwd = the
  thread's folder; mcpServers = GTDCraft's own `--mcp` on the staged copy, minting in the server's ID range, every
  write logged) → `session/prompt` with the action's context (name, project, tags, dates, estimate, note, references,
  attachments, sub-actions) and the request. The agent keeps its own connectors. Updates drive the thread: tool calls
  count steps and name the current activity; message chunks become its reply.
- **Reads are always allowed; writes are staged.** Each write through the gtd tools (or a plan of them in one
  `gtd_batch` call — "codemode") lands in the staged copy, so later reads see it, and is listed ("Add task “Book a
  slot”"). When the turn ends with changes, the thread waits for **Approve** (the staged copy merges into the real
  database with sync's merge under a lock shared with sync requests; devices get it at their next sync) or **Reject**
  (discarded). Questions: the agent ends its turn with one; the person replies in the thread (`/reply`), and the same
  session continues. Its own tools' permission requests come to the person as buttons (`/permission`). Stop
  (`/cancel` → ACP `session/cancel`), Finish.
- **Two hours** per thread (`THREAD_LIFETIME_SECS`); then it's Timed out and **Continue in a New Thread** starts one
  whose prompt carries a summary of the old (request, last four messages, what happened to its changes). A server
  restart marks running threads timed out. Threads persist in `agent-threads.json`.
- **On devices** (Mac, iPhone, iPad, `agentui.rs`): the row menu's Send to Agent… (agent choice + request); the
  action's row shows "Agent · step N" with a sliding progress bar while it works, "Agent · Waiting for you" /
  "Changes to review" after (tap to open the thread: messages, activity, permission buttons, staged changes with
  Approve / Reject, reply, Stop, Finish, Continue). Polls every 2 s while something runs, else every 30 s.
- **The Agents perspective**: actions with threads, grouped by project; its Show rule applies — Available /
  Remaining show actions whose latest thread is still active, Everything adds the finished ones (archived, not
  cluttering).
- Tested end to end on a scratch server with Claude Code: it staged two sub-actions with `gtd_batch`, the real
  database was unchanged until Approve, then held both under the right parent; a second thread asked its question
  and waited for a reply. (`gtd_batch` now accepts the prefixed names agents see, "mcp__gtdcraft__gtd_add_task".)

## Round 57 — rules for reading a request into fields
- When a sentence becomes an action: a date is **planned** if the task will be done at or on it, **due** if the task
  must be done by or before it, **defer** if the task can't begin until or after it. Durations ("for 40 minutes") are
  estimates, never dates. Every tag mentioned is kept. Project candidates are listed most likely first.
- GTDCraft gives agents these rules (`agent::FIELD_RULES`): in every Send to Agent prompt and in the MCP server's
  `initialize` instructions, so any agent using the gtd tools picks planned / due / defer consistently, treats
  durations as estimates, keeps every mentioned tag, and files into the most likely project.

## Round 58 — focus on every device at once; one server copy per database
- **Clocks didn't travel**: `clock_in` / `clock_out` changed an action's clock without moving its `modified`, and sync
  keeps the newest copy of each record — so the Mac's older copy won over the phone's clock-in. Both now stamp the
  action (tested: `up_next_and_clocks_travel`).
- **Up Next didn't travel either**: the server kept the queue of whichever device synced last (the Mac's, minute by
  minute), so the widget showed the wrong Up Next. The queue has its own stamp (`queue_modified`, set on save when it
  changed) and the newer one wins. The widget's Up Next now starts after its Focus item instead of repeating it.
- **Within seconds, not minutes**: devices send a change a second after it's made, and every 3 s ask the server for
  the database's revision (`GET /v1/rev?db=…`, its copy's modification time); when it moves they sync at once. The
  server writes its copy only when the shared content changed (`shared_content`: everything but each device's own
  settings), so a sync with nothing new doesn't set the other devices syncing (tested). A clock or Pomodoro started on
  one device appears on the others (`follow_focus`: a Pomodoro clock gets the local countdown from its start + the
  work length; a stopped one ends it), with the iPhone's Live Activity, and the Home Screen widgets reload at once
  (`WidgetCenter.reloadAllTimelines` from the Swift library) after every sync and clock change.
- **Databases have identities** (`db_id`): new databases get one; the server keeps one copy per identity
  (`db_path`: the first copy, `gtdcraft.jsonl`, for the identity it was given or copies without one; others in
  `db-<id>.jsonl`; `GET /v1/databases` lists them). Databases from before get theirs from the server at their next
  sync and the devices adopt it. Each device remembers the database it syncs (`sync-db` beside the app's files);
  opening or starting a **different** database (or an old file without an identity) **pauses** syncing — nothing is
  sent or merged, so an empty or other database can't overwrite the synced one or mix into it — with a banner: Sync
  This Database on Its Own (its own server copy; the device now syncs it), Replace with the Synced Database (a backup
  of what's there, then the synced one fills in), Not Now; Settings › Sync shows which database and whether it's
  paused. A new, empty device joins the server's most recently changed database; the pairing QR link carries the
  identity (`&db=`), so a paired empty phone joins exactly the Mac's. Widgets, Share and Send to Agent use the
  device's database on the server (`?db=`, `"db"`). Tested: `databases_stay_apart`, `same_database_rules`.

## Round 59 — backups before replacing local data with sync data
- Risky sync moves make a backup first, named for the reason and the time ("yyyy-MM-dd HHmmss"), in the backup
  folder (created if missing).
- GTDCraft: the paused-sync banner's Replace with the Synced Database (Round 58) first saves "GTDCraft Before Replacing
  Local Data with Sync Data on 2026-11-03 164507.json" in the app's Backups folder (`backup_name`, tested), then
  starts from the synced database.

## Round 60 — Send to Agent prompt templates (the person's format)
- Settings › Agent › Send to Agent templates: each a name and front-matter-style fields, `key: value` per line (an
  optional `---` above and below; a line without a key continues the field above). Built in: a starter
  "Feature or fix" — task, tag: dev, where, acceptance, followUp. Templates travel with the
  connections (stamped, newest wins).
- The Send to Agent sheet picks the template whose `project:` names the action's project (or a folder above it) or
  whose `tag:` / `tags:` names one of its tags, shows its fields filled with their defaults to edit for this request,
  and can send with only the fields filled. The agent's prompt gets them after the action's details: "From the
  person's template:" then each field, and `followUp:` as "Last, once everything else is done: …"
  (`agent::parse_fields`, `template_matches`, `fields_block`, tested).

## Round 61 — template variables, strict
- GTDCraft's Send to Agent template values can use the action's `{task}`, `{project}`, `{tags}`, `{note}`,
  `{defer}`, `{planned}`, `{due}`, `{estimate}`, `{link}` (`agent::template_env`); they're filled when you send
  (`fill_vars`, tested; `{{` / `}}` for literal braces). An unknown name stops the send with "The variable 'x' is not
  defined (use {task}, …)" in the sheet, instead of reaching the agent as a literal: an undefined variable is an
  error, not an empty value.

## Round 62 — the sync server, checked and hardened
- Health, databases, revision, widget and agent endpoints all answered; the log held only start-up lines. One fault: an
  agent thread (09:14) sat at "Starting…" for two hours — the agent never answered ACP `initialize`, and the runner
  waited out the thread's whole lifetime. Now the handshake (`initialize`, `session/new`) must answer within
  `STARTUP_SECS` (180 s; a first `npx` run downloads the adapter) or the thread fails with "The agent didn't start: no
  answer to its handshake in 3 minutes…". A restart's "timed out" marks are written to `agent-threads.json` at once
  (the file still said "working"). Every refusal or failure the server returns is logged to sync-server.log — time,
  method, path without its query, code, message; never the token.
- Deployed from v0.18.1 plus only these two fixes (built in a separate clean checkout), not from the working tree, which
  holds another agent's unfinished System One changes; the previous server program and its data were backed up first
  in GTDCraft/server-backups/.

## Round 63 — telling the person when sync can't happen
- A sync failure shouldn't pass quietly: the person needs to know changes aren't reaching the server, and a way to
  retry.
- GTDCraft: after two failed syncs in a row (`FAIL_BANNER_AFTER`; one can be a blip — the Mac waking, Wi-Fi changing)
  a "Can't sync" banner shows the reason, that changes stay on this device until the server answers, and Retry / Not
  Now; the next good sync clears it (and re-arms it). The server keeps its own log of what it refused (Round 62).
