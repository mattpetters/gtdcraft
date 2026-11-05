// GTDCraft WidgetKit extensions — Home Screen, Lock Screen, and controls.
// Data flows through the sync server (GET /v1/widget) because a free Apple account
// can't use App Groups for shared file access between the app and its extensions.
// The user configures the server URL and token in the widget's edit screen.

import WidgetKit
import SwiftUI
import AppIntents

// MARK: - Network

/// The JSON the sync server's GET /v1/widget returns.
struct WidgetData: Codable {
    var focus: FocusItem?
    var up_next: [NameItem]
    var due_today: [NameItem]
    var due_today_count: Int
    var time_today_min: Int
    var time_week_min: Int
    var active_pomodoro: Bool

    struct FocusItem: Codable {
        var name: String
        var project: String
    }
    struct NameItem: Codable {
        var name: String
    }

    static let empty = WidgetData(focus: nil, up_next: [], due_today: [], due_today_count: 0, time_today_min: 0, time_week_min: 0, active_pomodoro: false)
}

/// Fetches the widget summary from the sync server.
/// The app's own sync settings, which it writes to the shared App Group — used when the widget's edit screen is left
/// empty (the person: "widget should reuse server connection from app").
func appConnection() -> (String, String) {
    let d = appGroupDefaults()
    return (d?.string(forKey: "sync_url") ?? "", d?.string(forKey: "sync_token") ?? "")
}

/// The App Group shared with the app (GTDAppGroup in Info.plist, from GTD_APP_GROUP in ios/Config).
func appGroupDefaults() -> UserDefaults? {
    guard let group = Bundle.main.object(forInfoDictionaryKey: "GTDAppGroup") as? String, !group.isEmpty else { return nil }
    return UserDefaults(suiteName: group)
}

/// The app's database on the server (one copy per database); empty for databases from before identities.
func sharedDB() -> String {
    appGroupDefaults()?.string(forKey: "sync_db") ?? ""
}

func connection(_ server: String, _ token: String) -> (String, String) {
    let s = server.trimmingCharacters(in: .whitespaces), t = token.trimmingCharacters(in: .whitespaces)
    return s.isEmpty || t.isEmpty ? appConnection() : (s, t)
}

func fetchWidgetData(server: String, token: String) async -> WidgetData {
    let (server, token) = connection(server, token)
    guard !server.isEmpty, !token.isEmpty,
          let url = URL(string: "\(server.trimmingCharacters(in: .init(charactersIn: "/")))/v1/widget?db=\(sharedDB())") else {
        return .empty
    }
    var req = URLRequest(url: url)
    req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
    req.timeoutInterval = 10
    do {
        let (data, response) = try await URLSession.shared.data(for: req)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else { return .empty }
        return try JSONDecoder().decode(WidgetData.self, from: data)
    } catch {
        return .empty
    }
}

// MARK: - Configuration intent

struct GTDCraftWidgetIntent: WidgetConfigurationIntent {
    static var title: LocalizedStringResource = "GTDCraft Server"
    static var description: IntentDescription = "Connect to your GTDCraft sync server."

    @Parameter(title: "Server URL", default: "")
    var serverURL: String

    @Parameter(title: "Token", default: "")
    var token: String
}

// MARK: - Shared timeline

struct GTDCraftEntry: TimelineEntry {
    let date: Date
    let data: WidgetData
    let configured: Bool
}

struct GTDCraftProvider: AppIntentTimelineProvider {
    func placeholder(in context: Context) -> GTDCraftEntry {
        GTDCraftEntry(date: .now, data: .empty, configured: true)
    }

    func snapshot(for configuration: GTDCraftWidgetIntent, in context: Context) async -> GTDCraftEntry {
        let d = await fetchWidgetData(server: configuration.serverURL, token: configuration.token)
        return GTDCraftEntry(date: .now, data: d, configured: !connection(configuration.serverURL, configuration.token).0.isEmpty)
    }

    func timeline(for configuration: GTDCraftWidgetIntent, in context: Context) async -> Timeline<GTDCraftEntry> {
        let d = await fetchWidgetData(server: configuration.serverURL, token: configuration.token)
        let entry = GTDCraftEntry(date: .now, data: d, configured: !connection(configuration.serverURL, configuration.token).0.isEmpty)
        // WidgetKit refreshes at most every 15 minutes; ask for the next update in 15 min.
        let next = Calendar.current.date(byAdding: .minute, value: 15, to: .now)!
        return Timeline(entries: [entry], policy: .after(next))
    }
}

// MARK: - Helper views

func hm(_ mins: Int) -> String {
    mins >= 60 ? "\(mins / 60)h \(mins % 60)m" : "\(mins)m"
}

struct ConfigureHint: View {
    var body: some View {
        VStack(spacing: 4) {
            Image(systemName: "gearshape")
                .font(.title2)
            Text("Open GTDCraft and link it to your sync server")
                .font(.caption)
                .multilineTextAlignment(.center)
        }
        .foregroundStyle(.secondary)
    }
}

// MARK: - Focus / Up Next widget

struct FocusWidgetView: View {
    let entry: GTDCraftEntry

    var body: some View {
        if !entry.configured {
            ConfigureHint()
        } else if let focus = entry.data.focus {
            VStack(alignment: .leading, spacing: 6) {
                Label {
                    Text("Focus")
                        .font(.caption.bold())
                        .foregroundStyle(.secondary)
                } icon: {
                    Image(systemName: entry.data.active_pomodoro ? "timer" : "target")
                        .foregroundStyle(.orange)
                }
                Text(focus.name)
                    .font(.headline)
                    .lineLimit(2)
                if !focus.project.isEmpty {
                    Text(focus.project)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                if !entry.data.up_next.isEmpty {
                    Divider()
                    Text("Up Next")
                        .font(.caption2.bold())
                        .foregroundStyle(.secondary)
                    ForEach(entry.data.up_next.prefix(3), id: \.name) { item in
                        Text("  \(item.name)")
                            .font(.caption)
                            .lineLimit(1)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        } else {
            VStack(spacing: 4) {
                Image(systemName: "checkmark.circle")
                    .font(.title)
                    .foregroundStyle(.green)
                Text("All clear")
                    .font(.caption)
            }
        }
    }
}

struct FocusWidget: Widget {
    let kind = "gtdcraft.widget.focus"

    var body: some WidgetConfiguration {
        AppIntentConfiguration(kind: kind, intent: GTDCraftWidgetIntent.self, provider: GTDCraftProvider()) { entry in
            FocusWidgetView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Focus / Up Next")
        .description("Your current focus task and what's up next.")
        .supportedFamilies([.systemMedium, .systemLarge])
    }
}

// MARK: - Due Today widget

struct DueTodayWidgetView: View {
    let entry: GTDCraftEntry

    var body: some View {
        if !entry.configured {
            ConfigureHint()
        } else {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Image(systemName: "calendar")
                        .foregroundStyle(.red)
                    Text("Due Today")
                        .font(.caption.bold())
                    Spacer()
                    Text("\(entry.data.due_today_count)")
                        .font(.title2.bold())
                        .foregroundStyle(entry.data.due_today_count > 0 ? .red : .secondary)
                }
                if entry.data.due_today.isEmpty {
                    Spacer()
                    Text("Nothing due today")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity)
                    Spacer()
                } else {
                    ForEach(entry.data.due_today.prefix(6), id: \.name) { item in
                        HStack(spacing: 4) {
                            Circle()
                                .fill(.red.opacity(0.6))
                                .frame(width: 6, height: 6)
                            Text(item.name)
                                .font(.caption)
                                .lineLimit(1)
                        }
                    }
                    if entry.data.due_today_count > 6 {
                        Text("+ \(entry.data.due_today_count - 6) more")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

struct DueTodayWidget: Widget {
    let kind = "gtdcraft.widget.duetoday"

    var body: some WidgetConfiguration {
        AppIntentConfiguration(kind: kind, intent: GTDCraftWidgetIntent.self, provider: GTDCraftProvider()) { entry in
            DueTodayWidgetView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Due Today")
        .description("Actions due today.")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}

// MARK: - Stats widget

struct StatsWidgetView: View {
    let entry: GTDCraftEntry

    var body: some View {
        if !entry.configured {
            ConfigureHint()
        } else {
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Image(systemName: "chart.bar")
                        .foregroundStyle(.blue)
                    Text("Time Tracked")
                        .font(.caption.bold())
                }
                HStack(spacing: 16) {
                    VStack {
                        Text(hm(entry.data.time_today_min))
                            .font(.title3.bold().monospacedDigit())
                        Text("Today")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    VStack {
                        Text(hm(entry.data.time_week_min))
                            .font(.title3.bold().monospacedDigit())
                        Text("This Week")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity)
            }
        }
    }
}

struct StatsWidget: Widget {
    let kind = "gtdcraft.widget.stats"

    var body: some WidgetConfiguration {
        AppIntentConfiguration(kind: kind, intent: GTDCraftWidgetIntent.self, provider: GTDCraftProvider()) { entry in
            StatsWidgetView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Time Tracked")
        .description("How much you've worked today and this week.")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}

// MARK: - Quick Entry (Lock Screen control, iOS 18+)

@available(iOS 18.0, *)
struct QuickEntryControl: ControlWidget {
    let kind = "gtdcraft.control.quickentry"

    var body: some ControlWidgetConfiguration {
        StaticControlConfiguration(kind: kind) {
            ControlWidgetButton(action: QuickEntryIntent()) {
                Label("Quick Entry", systemImage: "plus.circle.fill")
            }
        }
        .displayName("GTDCraft Quick Entry")
        .description("Open GTDCraft to add a task.")
    }
}

struct QuickEntryIntent: AppIntent {
    static var title: LocalizedStringResource = "Quick Entry"
    static var description: IntentDescription = "Open GTDCraft to add a new task."
    static var openAppWhenRun = true

    func perform() async throws -> some IntentResult {
        return .result()
    }
}

// MARK: - Focus Live Activity (Lock Screen and Dynamic Island)

struct FocusLiveActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: GTDFocusAttributes.self) { ctx in
            HStack(spacing: 12) {
                Image(systemName: ctx.state.pomodoro ? "timer" : "clock")
                    .font(.title2)
                    .foregroundStyle(ctx.state.pomodoro ? .red : .blue)
                VStack(alignment: .leading, spacing: 2) {
                    Text(ctx.state.title).font(.headline).lineLimit(1)
                    Text(ctx.state.pomodoro ? "Pomodoro" : "Clocked in").font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                timer(ctx.state).font(.title2.monospacedDigit()).multilineTextAlignment(.trailing)
            }
            .padding()
        } dynamicIsland: { ctx in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Image(systemName: ctx.state.pomodoro ? "timer" : "clock")
                }
                DynamicIslandExpandedRegion(.trailing) {
                    timer(ctx.state).monospacedDigit().frame(maxWidth: 70)
                }
                DynamicIslandExpandedRegion(.bottom) {
                    Text(ctx.state.title).lineLimit(1)
                }
            } compactLeading: {
                Image(systemName: ctx.state.pomodoro ? "timer" : "clock")
            } compactTrailing: {
                timer(ctx.state).monospacedDigit().frame(maxWidth: 50)
            } minimal: {
                Image(systemName: ctx.state.pomodoro ? "timer" : "clock")
            }
        }
    }

    /// A Pomodoro counts down to its end; a clock-in counts up from its start (the system keeps it ticking).
    func timer(_ s: GTDFocusAttributes.ContentState) -> Text {
        if let end = s.end {
            return Text(timerInterval: s.start...end, countsDown: true)
        }
        return Text(s.start, style: .timer)
    }
}

// MARK: - Widget bundle

@main
struct GTDCraftWidgetBundle: WidgetBundle {
    var body: some Widget {
        FocusWidget()
        DueTodayWidget()
        StatsWidget()
        FocusLiveActivity()
        if #available(iOS 18.0, *) {
            QuickEntryControl()
        }
    }
}
