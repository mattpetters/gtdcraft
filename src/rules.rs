//! Custom perspective rules, edited as nested "All / Any / None of the following" groups holding rules such as
//! "Availability: Remaining" or "Contained within project or folder".
use crate::model::*;
use chrono::{Duration, NaiveTime};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Rule {
    All { rules: Vec<Rule> },
    Any { rules: Vec<Rule> },
    None { rules: Vec<Rule> },
    /// first | available | remaining | completed | dropped
    Availability { value: String },
    /// flagged | due_soon | overdue | due_soon_or_flagged
    Status { value: String },
    /// None = untagged
    Tag { id: Option<Id> },
    /// None = in the Inbox
    Project { id: Option<Id> },
    Folder { name: String },
    /// field: due | defer | planned | added | modified | completed; days > 0 = within the next N days,
    /// days < 0 = within the last N days, 0 = today
    DateWithin { field: String, days: i32 },
    HasDate { field: String, has: bool },
    /// max_min None = unestimated
    Duration { max_min: Option<u32> },
    Text { contains: String },
    Repeats { value: bool },
    HasNote { value: bool },
    /// actionHasAnyOfTags / actionHasAllOfTags
    Tags { ids: Vec<Id>, all: bool },
    /// actionHasTagWithStatus: active | on_hold
    TagStatus { status: String },
    /// actionHasProjectWithStatus: active | on_hold | completed | dropped
    ProjectStatus { status: String },
    /// actionIsInSingleActionsList
    SingleActions,
    /// actionIsProjectOrGroup (has sub-actions)
    ProjectOrGroup,
    /// actionIsLeaf (no sub-actions)
    Leaf,
    /// actionDateIsYesterday / Today / Tomorrow: day = -1, 0, 1
    DateOn { field: String, day: i32 },
    /// actionDateIsBeforeDateSpec / actionDateIsAfterDateSpec
    DateBefore { field: String, date: chrono::NaiveDate },
    DateAfter { field: String, date: chrono::NaiveDate },
    /// disabledRule: a turned-off rule is skipped (DisabledFilterRulePredicate)
    Off { rule: Box<Rule> },
}

pub const LEAF_KINDS: [(&str, &str); 20] = [
    ("availability", "Availability"),
    ("status", "Status"),
    ("tag", "Has tag"),
    ("project", "Contained within project"),
    ("folder", "Contained within folder"),
    ("date_within", "Date within"),
    ("has_date", "Has a date"),
    ("duration", "Estimated duration"),
    ("text", "Name or note contains"),
    ("repeats", "Is repeating"),
    ("has_note", "Has a note"),
    ("tags", "Has any / all of tags"),
    ("tag_status", "Has a tag with status"),
    ("project_status", "Has a project with status"),
    ("single_actions", "Is in a single actions list"),
    ("project_or_group", "Is a project or action group"),
    ("leaf", "Is an action (no sub-actions)"),
    ("date_on", "Date is yesterday / today / tomorrow"),
    ("date_before", "Date is before / after a date"),
    ("group", "Group (All / Any / None)"),
];

impl Rule {
    pub fn new_leaf(kind: &str) -> Rule {
        match kind {
            "availability" => Rule::Availability { value: "remaining".into() },
            "status" => Rule::Status { value: "flagged".into() },
            "tag" => Rule::Tag { id: None },
            "project" => Rule::Project { id: None },
            "folder" => Rule::Folder { name: String::new() },
            "date_within" => Rule::DateWithin { field: "due".into(), days: 7 },
            "has_date" => Rule::HasDate { field: "due".into(), has: true },
            "duration" => Rule::Duration { max_min: Some(15) },
            "text" => Rule::Text { contains: String::new() },
            "repeats" => Rule::Repeats { value: true },
            "has_note" => Rule::HasNote { value: true },
            "tags" => Rule::Tags { ids: vec![], all: false },
            "tag_status" => Rule::TagStatus { status: "on_hold".into() },
            "project_status" => Rule::ProjectStatus { status: "active".into() },
            "single_actions" => Rule::SingleActions,
            "project_or_group" => Rule::ProjectOrGroup,
            "leaf" => Rule::Leaf,
            "date_on" => Rule::DateOn { field: "due".into(), day: 0 },
            "date_before" => Rule::DateBefore { field: "due".into(), date: crate::model::now().date() },
            _ => Rule::All { rules: vec![] },
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Rule>> {
        match self {
            Rule::All { rules } | Rule::Any { rules } | Rule::None { rules } => Some(rules),
            _ => None,
        }
    }

    /// The default for a new custom perspective: one "Availability: Remaining" rule.
    pub fn default_root() -> Rule {
        Rule::All { rules: vec![Rule::Availability { value: "remaining".into() }] }
    }

    pub fn matches(&self, s: &Store, d: &Derived, t: &Task, now: Dt) -> bool {
        let on = |r: &&Rule| !matches!(r, Rule::Off { .. });
        match self {
            Rule::All { rules } => rules.iter().filter(on).all(|r| r.matches(s, d, t, now)),
            Rule::Any { rules } => rules.iter().filter(on).count() == 0 || rules.iter().filter(on).any(|r| r.matches(s, d, t, now)),
            Rule::None { rules } => !rules.iter().filter(on).any(|r| r.matches(s, d, t, now)),
            Rule::Off { .. } => true,
            Rule::Tags { ids, all } => {
                if ids.is_empty() {
                    true
                } else if *all {
                    ids.iter().all(|g| t.tags.contains(g))
                } else {
                    ids.iter().any(|g| t.tags.contains(g))
                }
            }
            Rule::TagStatus { status } => t.tags.iter().filter_map(|g| s.tag(*g)).any(|g| g.on_hold == (status == "on_hold")),
            Rule::ProjectStatus { status } => t.project.and_then(|p| s.project(p)).is_some_and(|p| {
                let st = match p.status {
                    ProjectStatus::Active => "active",
                    ProjectStatus::OnHold => "on_hold",
                    ProjectStatus::Completed => "completed",
                    ProjectStatus::Dropped => "dropped",
                };
                st == status
            }),
            Rule::SingleActions => t.project.and_then(|p| s.project(p)).is_some_and(|p| p.kind == ProjectKind::SingleActions),
            Rule::ProjectOrGroup => d.children.get(&Some(t.id)).is_some_and(|c| !c.is_empty()),
            Rule::Leaf => !d.children.get(&Some(t.id)).is_some_and(|c| !c.is_empty()),
            Rule::DateOn { field, day } => field_date(t, d, field).is_some_and(|x| x.date() == now.date() + Duration::days(*day as i64)),
            Rule::DateBefore { field, date } => field_date(t, d, field).is_some_and(|x| x.date() < *date),
            Rule::DateAfter { field, date } => field_date(t, d, field).is_some_and(|x| x.date() > *date),
            Rule::Availability { value } => {
                let st = d.st(t.id);
                match value.as_str() {
                    "first" => st == Status::Next,
                    "available" => st.available(),
                    "completed" => st == Status::Completed,
                    "dropped" => st == Status::Dropped,
                    _ => st.remaining(),
                }
            }
            Rule::Status { value } => {
                let st = d.st(t.id);
                let flagged = s.effective_flagged(t.id).0 || s.effective_flagged(t.id).1;
                match value.as_str() {
                    "due_soon" => st == Status::DueSoon,
                    "overdue" => st == Status::Overdue,
                    "due_soon_or_flagged" => flagged || matches!(st, Status::DueSoon | Status::Overdue),
                    _ => flagged,
                }
            }
            Rule::Tag { id } => match id {
                // a parent tag matches its children too ("Status" matches "Status : In Progress")
                Some(g) => s.tag_family(*g).iter().any(|x| t.tags.contains(x)),
                None => t.tags.is_empty(),
            },
            Rule::Project { id } => t.project == *id,
            Rule::Folder { name } => t.project.and_then(|p| s.project(p)).is_some_and(|p| p.folder.eq_ignore_ascii_case(name)),
            Rule::DateWithin { field, days } => match field_date(t, d, field) {
                None => false,
                Some(x) => {
                    let today = now.date().and_time(NaiveTime::MIN);
                    match days.signum() {
                        0 => x.date() == now.date(),
                        1 => x >= today && x < today + Duration::days(*days as i64 + 1),
                        _ => x <= now && x >= today + Duration::days(*days as i64),
                    }
                }
            },
            Rule::HasDate { field, has } => field_date(t, d, field).is_some() == *has,
            Rule::Duration { max_min } => match (max_min, t.duration_min) {
                (None, m) => m.is_none(),
                (Some(max), Some(m)) => m <= *max,
                (Some(_), None) => false,
            },
            Rule::Text { contains } => {
                let q = contains.to_lowercase();
                q.is_empty() || t.name.to_lowercase().contains(&q) || t.note.to_lowercase().contains(&q)
            }
            Rule::Repeats { value } => t.repeat.is_some() == *value,
            Rule::HasNote { value } => !t.note.trim().is_empty() == *value,
        }
    }

    /// One line, for lists and the AI preview.
    pub fn label(&self, s: &Store) -> String {
        let tag = |id: &Option<Id>| id.and_then(|g| s.tag(g)).map(|g| g.name.clone());
        let proj = |id: &Option<Id>| id.and_then(|p| s.project(p)).map(|p| p.name.clone());
        match self {
            Rule::All { .. } => "All of the following:".into(),
            Rule::Any { .. } => "Any of the following:".into(),
            Rule::None { .. } => "None of the following:".into(),
            Rule::Availability { value } => format!("Availability: {}", cap(value)),
            Rule::Status { value } => format!("Status: {}", cap(&value.replace('_', " "))),
            Rule::Tag { id } => tag(id).map(|n| format!("Has tag: {n}")).unwrap_or_else(|| "Untagged".into()),
            Rule::Project { id } => proj(id).map(|n| format!("Contained within project: {n}")).unwrap_or_else(|| "Is in the Inbox".into()),
            Rule::Folder { name } => format!("Contained within folder: {name}"),
            Rule::DateWithin { field, days } => match days.signum() {
                0 => format!("Has a {field} date of today"),
                1 => format!("Has a {field} date in the next {days} days"),
                _ => format!("Has a {field} date in the past {} days", -days),
            },
            Rule::HasDate { field, has } => if *has { format!("Has a {field} date") } else { format!("Has no {field} date") },
            Rule::Duration { max_min } => max_min.map(|m| format!("Estimated duration ≤ {m} min")).unwrap_or_else(|| "Unestimated".into()),
            Rule::Text { contains } => format!("Name or note contains “{contains}”"),
            Rule::Repeats { value } => if *value { "Is repeating".into() } else { "Is not repeating".into() },
            Rule::HasNote { value } => if *value { "Has a note".into() } else { "Has no note".into() },
            Rule::Tags { ids, all } => {
                let names: Vec<String> = ids.iter().filter_map(|g| s.tag(*g)).map(|g| g.name.clone()).collect();
                format!("Has {} of tags: {}", if *all { "all" } else { "any" }, names.join(", "))
            }
            Rule::TagStatus { status } => format!("Has a tag that is {}", status.replace('_', " ")),
            Rule::ProjectStatus { status } => format!("Has a project that is {}", status.replace('_', " ")),
            Rule::SingleActions => "Is in a single actions list".into(),
            Rule::ProjectOrGroup => "Is a project or action group".into(),
            Rule::Leaf => "Is an action (no sub-actions)".into(),
            Rule::DateOn { field, day } => format!("Has a {field} date of {}", ["yesterday", "today", "tomorrow"][(*day + 1).clamp(0, 2) as usize]),
            Rule::DateBefore { field, date } => format!("Has a {field} date before {date}"),
            Rule::DateAfter { field, date } => format!("Has a {field} date after {date}"),
            Rule::Off { rule } => format!("(off) {}", rule.label(s)),
        }
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn field_date(t: &Task, d: &Derived, field: &str) -> Option<Dt> {
    match field {
        "due" => d.eff_due.get(&t.id).copied(),
        "defer" => d.eff_defer.get(&t.id).copied(),
        "planned" => t.planned,
        "added" => Some(t.added),
        "modified" => Some(t.modified),
        "completed" => t.completed,
        _ => None,
    }
}

/// Converts the earlier flat rule set into a rule tree.
pub fn from_legacy(r: &Rules) -> Rule {
    let mut v = vec![Rule::Availability { value: if r.availability == "all" { "remaining".into() } else { r.availability.clone() } }];
    if r.flagged_only {
        v.push(Rule::Status { value: "flagged".into() });
    }
    if let Some(g) = r.tag {
        v.push(Rule::Tag { id: Some(g) });
    }
    if let Some(p) = r.project {
        v.push(Rule::Project { id: Some(p) });
    }
    if let Some(n) = r.due_within_days {
        v.push(Rule::DateWithin { field: "due".into(), days: n as i32 });
    }
    Rule::All { rules: v }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn nested_rules() {
        let now = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let mut s = Store::default();
        let money = s.add_project("Income", ProjectKind::Parallel);
        s.project_mut(money).unwrap().folder = "Work".into();
        let a = s.add_task("invoice", Some(money), None);
        s.task_mut(a).unwrap().due = Some(now + Duration::days(2));
        let b = s.add_task("call", None, None);
        s.task_mut(b).unwrap().flagged = true;
        let d = s.derive(now);
        let r = Rule::All {
            rules: vec![
                Rule::Availability { value: "remaining".into() },
                Rule::Any { rules: vec![Rule::Folder { name: "work".into() }, Rule::Status { value: "flagged".into() }] },
                Rule::None { rules: vec![Rule::Text { contains: "call".into() }] },
            ],
        };
        assert!(r.matches(&s, &d, s.task(a).unwrap(), now));
        assert!(!r.matches(&s, &d, s.task(b).unwrap(), now));
        assert!(Rule::DateWithin { field: "due".into(), days: 3 }.matches(&s, &d, s.task(a).unwrap(), now));
        assert!(!Rule::DateWithin { field: "due".into(), days: 1 }.matches(&s, &d, s.task(a).unwrap(), now));
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Rule>(&json).unwrap(), r);
        // a turned-off rule is skipped; extra rule kinds
        let off = Rule::All { rules: vec![Rule::Off { rule: Box::new(Rule::Text { contains: "zzz".into() }) }, Rule::Leaf] };
        assert!(off.matches(&s, &d, s.task(a).unwrap(), now));
        assert!(Rule::DateOn { field: "due".into(), day: 2 }.matches(&s, &d, s.task(a).unwrap(), now));
        assert!(Rule::DateBefore { field: "due".into(), date: now.date() + Duration::days(3) }.matches(&s, &d, s.task(a).unwrap(), now));
        assert!(Rule::ProjectStatus { status: "active".into() }.matches(&s, &d, s.task(a).unwrap(), now));
        assert!(!Rule::SingleActions.matches(&s, &d, s.task(a).unwrap(), now));
        let legacy = from_legacy(&Rules { flagged_only: true, ..Rules::default() });
        assert!(legacy.matches(&s, &d, s.task(b).unwrap(), now));
    }
}
