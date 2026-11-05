# GTDCraft roadmap

What's still to do, in the order it's planned. Each item says where the reference is and what "done" means.
Behaviour rules go in [MECHANICS.md](MECHANICS.md); this file is the plan.

Status: ☐ to do · ◐ in progress · ☑ done

## Open

### 1. iPhone, the rest of the layout ◐
References: pictures 13–18, 25–29, 38 (portrait), 30–37 (landscape); the person's attachments 11–14.
- ☑ Bottom bar: commands over favourite-perspective tabs, scrolling to the open one (v0.8.0–0.8.1).
- ☑ Slim top bar: ▯ (perspectives page, with ☆/★) · eye · ⓘ when something is selected · ⋯ (Clean Up,
  Go Back, Sync, Settings); Search from the bottom bar opens a field with Cancel (v0.9.0). Still to add to ⋯: Select,
  Share, Help.
- ☑ Scrolling: the large title scrolls with the list and shows small in the top bar; the bottom
  bar shrinks to a round perspective button and a + capsule while scrolling down, and comes back scrolling up or at
  the top (Round 46).
- ☑ Landscape: rail · sidebar with ▯ ☰ Edit and a floating + · the list with its own top
  bar (eye, title, i ⋯, ✓ while editing) and floating capsules (⚡ Search, + Quick Entry); the inspector as a page
  (Round 46). Still to match: the sidebar's ⋯, Edit as a sidebar edit mode, the inspector
  as a sheet rather than a narrow page, the clock / Pomodoro pill (not shown sideways).
- ☑ Quick Entry as a full-screen sheet with Cancel · eye · Save at the top and one field per line.
- ☑ The bar floats over the list (content scrolls under it; the list ends with room to scroll past it).
- ◐ Inspector as a pushed page with a round ‹ back; its rows are the Mac inspector's.
- ☐ Transitions: ask the person for the video they offered before tuning animations.

### 2. Notes: `file://` links and local paths; an Attachments section ◐
- ☑ Notes recognise `file://…` and local paths (`~/…`, `/Users/…`, `/Volumes/…`) and show them as file/folder rows
  that open in Finder.
- ☑ **Attachments** inspector section on tasks and projects: Add File… / Folder… (Finder picker on the Mac), paste a
  path or `file://` link, × to remove; separate from links in notes; synced with the record.
- ☐ iPhone/iPad: the Files picker (UIDocumentPickerViewController) — today only pasting a path works there.

### 3. Nearby on Apple's maps and location ◐
- ☑ The map is Apple's (MapKit snapshots, dark) on Mac and iPhone, with GTDCraft's pins on top; OSM elsewhere.
- ☑ Core Location supplies "here" on Mac and iPhone (When In Use).
- ☑ iPhone location reminders through iOS (UNLocationNotificationTrigger, nearest 20 places) — they fire closed.
- ☑ Tag locations search Apple Maps (MKLocalSearch, near you) and list the results to pick from; Nominatim only off
  Apple or when Apple Maps fails. The iPhone-style Location card: address with ✕, Change Location (Here with its
  address, Anywhere, Search, Address, Pin on the Nearby map), Notification (none / arriving / leaving → the geofence's
  entry/exit) and Distance (~650 ft / ~¼ mi / ~6¼ mi). Round 45.
- ☐ Contact (an address from Contacts) in Change Location. ☐ Pin by touch-and-hold on its own map sheet (today Pin
  goes to Nearby). ☐ "Use the search term itself" (the nearest result, updated as you move).
- Linux GeoClue: GitHub issue #2. ☐ Mac region monitoring (optional; deprecated API there).

### 4. Connections that stay connected on iPhone ☑
- ☑ The Linear key, GitHub token and AI settings sync between devices (newest wins) through your sync server; iPhone
  Settings has a Connections page. (Keychain storage would keep them out of the database file — later, optional.)

### 5. Widgets, Live Activity, Lock Screen ◐
- ☑ Sync server endpoints: `GET /v1/widget` (focus task, Up Next, due today, time tracked), `POST /v1/inbox`
  (create an inbox item with an optional Reference). Both Bearer-token protected, unit-tested.
- ☑ Home Screen widgets (iPhone/iPad): Focus / Up Next (medium, large), Due Today (small, medium), Time Tracked
  (small, medium). Each uses `AppIntentConfiguration` — the user enters the sync server URL and token in the widget's
  edit screen, since a free Apple account cannot use App Groups for shared data between the app and its extensions.
- ☑ Lock Screen control: Quick Entry (iOS 18+, `ControlWidget`) opens GTDCraft. On iOS 17 it's absent gracefully.
- ☑ `gtdcraft:///add?name=…&note=…&ref_url=…&ref_title=…` URL: the app creates an inbox item when opened with this
  link (from a widget, Shortcut, or the Share extension).
- ☑ Live Activity for the Focus timer / clock-in (Lock Screen, Dynamic Island): a Swift static library
  (ios/LiveActivity) linked into the Rust app by build.rs starts and ends it; the widget extension draws it.
- ☐ Mac desktop widgets: WidgetKit on macOS requires a sandboxed app bundle built through Xcode. GTDCraft's Mac build
  is unsandboxed (`make-app.sh` / the Rust binary directly), so macOS widgets are not feasible with the current setup.
  Would need an Xcode project for the Mac target too.

### 6. Share: make or update a task from a message ◐
- ☑ Share extension (iPhone/iPad): shared text or URL → new inbox item with a Reference. The extension has its own
  server URL and token fields (stored in its own UserDefaults, entered on first use) and POSTs to `POST /v1/inbox`.
  Accepts text and web URLs.
- ☐ Mac Share extension: needs an Xcode-built Mac app bundle (same sandboxing constraint as Mac widgets).
- ☐ "Add to existing task": currently only creates new inbox items; adding a reference to an existing task needs a task
  picker UI in the extension.

### 7. Smaller
- ☑ Folders inside folders ("Work : Clients", like tags).
- ☑ GitHub: a repo reference lists its open issues and PRs to link any of them.
- Linux is tracked in GitHub issues (this effort is Apple platforms): #1 the build compiles, #2 GeoClue location,
  #3 notify-send verified.

## Needs the person
- On the Mac: Settings › General › **Remove Sample Items** (16 sample items synced from the phone).
- Try the reminder buttons (Complete / Snooze) on a real reminder, and the Calendar prompt on the Mac.
- A free Apple account's iPhone install expires after 7 days: `./make-ios-device.sh` renews it.

## Done (recent)
v0.8.1 favourite perspectives in the iPhone bar · v0.8.0 iPhone bottom bar, iPhone app icon · v0.7.x real folders,
touch reordering, perspectives/sections sync · v0.6.0 reminder Complete/Snooze · v0.5.0 UserNotifications,
References (GitHub, email, Messages) · Settings reordered · iPhone Settings page, iOS Calendar ·
QR pairing for sync · delete projects/folders · empty new databases · Quick Entry as its own floating window.
