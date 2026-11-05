// The Focus Live Activity's data, shared by the app (which starts it, ios/LiveActivity) and the widget extension
// (which draws it on the Lock Screen and in the Dynamic Island).
import ActivityKit
import Foundation

public struct GTDFocusAttributes: ActivityAttributes {
    public struct ContentState: Codable, Hashable {
        /// The action being worked on.
        public var title: String
        /// When the Pomodoro or clock-in began.
        public var start: Date
        /// When a Pomodoro ends (nil for an open-ended clock-in).
        public var end: Date?
        public var pomodoro: Bool

        public init(title: String, start: Date, end: Date?, pomodoro: Bool) {
            self.title = title
            self.start = start
            self.end = end
            self.pomodoro = pomodoro
        }
    }

    public init() {}
}
