// The Focus Live Activity, started and ended by the Rust app through two C functions (build.rs compiles this file
// into a static library linked into the gtdcraft binary on iOS). The widget extension draws it.
import ActivityKit
import Foundation
import WidgetKit

/// The widgets show the new state now (after a sync or a clock change), not at their next scheduled refresh.
@_cdecl("gtd_widgets_reload")
public func gtd_widgets_reload() {
    WidgetCenter.shared.reloadAllTimelines()
}

private var current: Activity<GTDFocusAttributes>?

/// Starts (or replaces) the Focus Live Activity. `end` ≤ 0 means an open-ended clock-in.
@_cdecl("gtd_live_start")
public func gtd_live_start(_ title: UnsafePointer<CChar>, _ start: Double, _ end: Double, _ pomodoro: Bool) {
    guard ActivityAuthorizationInfo().areActivitiesEnabled else { return }
    let state = GTDFocusAttributes.ContentState(
        title: String(cString: title),
        start: Date(timeIntervalSince1970: start),
        end: end > 0 ? Date(timeIntervalSince1970: end) : nil,
        pomodoro: pomodoro)
    let old = current
    Task {
        if let old { await old.end(nil, dismissalPolicy: .immediate) }
    }
    do {
        current = try Activity.request(attributes: GTDFocusAttributes(), content: .init(state: state, staleDate: state.end))
        NSLog("GTDCraft Live Activity started: %@", current?.id ?? "?")
    } catch {
        NSLog("GTDCraft Live Activity not started: %@", String(describing: error))
    }
}

/// Ends the Focus Live Activity (the Pomodoro or clock-in stopped).
@_cdecl("gtd_live_end")
public func gtd_live_end() {
    let old = current
    current = nil
    Task {
        if let old { await old.end(nil, dismissalPolicy: .immediate) }
        for a in Activity<GTDFocusAttributes>.activities { await a.end(nil, dismissalPolicy: .immediate) }
    }
}
