import Foundation
import Testing
@testable import Mimi

/// Paris (with its clock change on 25 October 2026), weeks from Monday, English.
private func paris() -> CalendarMath {
    var cal = Calendar(identifier: .gregorian)
    cal.timeZone = TimeZone(identifier: "Europe/Paris")!
    cal.firstWeekday = 2
    return CalendarMath(calendar: cal, locale: Locale(identifier: "en_GB"))
}

private func ms(_ s: String, _ math: CalendarMath = paris()) -> Int64 {
    let f = DateFormatter()
    f.calendar = math.calendar
    f.timeZone = math.calendar.timeZone
    f.locale = Locale(identifier: "en_US_POSIX")
    f.dateFormat = "yyyy-MM-dd HH:mm"
    return math.ms(f.date(from: s)!)
}

private func event(_ id: String, _ start: String, _ end: String, allDay: Bool = false) -> CalendarEvent {
    CalendarEvent(id: id, title: id, start: ms(start), end: ms(end), allDay: allDay)
}

struct CalendarMathTests {
    let math = paris()

    @Test func daysAreCalendarDaysAcrossTheClockChange() {
        let saturday = ms("2026-10-24 00:00")
        let sunday = math.addDays(saturday, 1)
        let monday = math.addDays(saturday, 2)
        #expect(sunday == ms("2026-10-25 00:00"))
        // That Sunday lasts 25 hours.
        #expect(monday - sunday == 25 * 3_600_000)
        #expect(math.startOfDay(ms("2026-10-25 23:30")) == sunday)
        #expect(math.daysBetween(saturday, ms("2026-10-26 08:00")) == 2)
    }

    @Test func weeksStartWhereTheRegionStartsThem() {
        #expect(math.startOfWeek(ms("2026-10-03 17:00")) == ms("2026-09-28 00:00"))
        #expect(math.startOfWeek(ms("2026-09-28 00:00")) == ms("2026-09-28 00:00"))
        var us = Calendar(identifier: .gregorian)
        us.timeZone = math.calendar.timeZone
        us.firstWeekday = 1
        let sunday = CalendarMath(calendar: us, locale: Locale(identifier: "en_US"))
        #expect(sunday.startOfWeek(ms("2026-10-03 17:00")) == ms("2026-09-27 00:00"))
    }

    @Test func allDayEventsShowOnTheirDaysOnly() {
        let e = event("Trip", "2026-10-08 00:00", "2026-10-10 00:00", allDay: true)
        #expect(!math.onDay(e, ms("2026-10-07 00:00")))
        #expect(math.onDay(e, ms("2026-10-08 00:00")))
        #expect(math.onDay(e, ms("2026-10-09 00:00")))
        #expect(!math.onDay(e, ms("2026-10-10 00:00")))
        #expect(math.timeRange(e) == "All day, 2 days")
        let one = event("Birthday", "2026-10-08 00:00", "2026-10-09 00:00", allDay: true)
        #expect(math.timeRange(one) == "All day")
    }

    @Test func wordsForDaysAndTimes() {
        let now = ms("2026-10-03 17:00")
        #expect(math.dayTitle(ms("2026-10-03 09:00"), now: now) == "Today")
        #expect(math.dayTitle(ms("2026-10-04 09:00"), now: now) == "Tomorrow")
        #expect(math.dayTitle(ms("2026-10-02 09:00"), now: now) == "Yesterday")
        #expect(math.dayTitle(ms("2026-10-07 09:00"), now: now) == "Wednesday 7 October")
        #expect(math.dayTitle(ms("2027-01-07 09:00"), now: now).contains("2027"))
        #expect(math.when(ms("2026-10-04 09:00"), now: now) == "Tomorrow at \(math.clock(ms("2026-10-04 09:00")))")
        #expect(math.when(ms("2026-10-06 19:30"), now: now) == "Tuesday at 19:30")
        #expect(math.timeRange(event("A", "2026-10-03 09:00", "2026-10-03 09:30")) == "\(math.clock(ms("2026-10-03 09:00"))) – \(math.clock(ms("2026-10-03 09:30")))")
        #expect(math.timeRange(event("B", "2026-10-03 22:00", "2026-10-04 01:00")) == "Sat 22:00 – Sun \(math.clock(ms("2026-10-04 01:00")))")
        // Ending at midnight is still the same day.
        #expect(math.timeRange(event("C", "2026-10-03 22:00", "2026-10-04 00:00")) == "22:00 – \(math.clock(ms("2026-10-04 00:00")))")
        #expect(math.monthTitle(now) == "October 2026")
    }

    @Test func overlappingEventsSitSideBySide() {
        let day = ms("2026-10-05 00:00")
        let placed = math.layoutDay([
            event("a", "2026-10-05 09:00", "2026-10-05 10:00"),
            event("b", "2026-10-05 09:30", "2026-10-05 11:00"),
            event("c", "2026-10-05 10:00", "2026-10-05 10:30"),
            event("d", "2026-10-05 14:00", "2026-10-05 15:00"),
            event("all", "2026-10-05 00:00", "2026-10-06 00:00", allDay: true),
        ], day: day)
        let byId = Dictionary(uniqueKeysWithValues: placed.map { ($0.event.id, $0) })
        #expect(byId["all"] == nil)
        #expect(byId["a"]?.column == 0 && byId["b"]?.column == 1)
        // "c" starts when "a" ends: it takes a's column again.
        #expect(byId["c"]?.column == 0)
        #expect(byId["a"]?.columns == 2 && byId["c"]?.columns == 2)
        #expect(byId["d"]?.column == 0 && byId["d"]?.columns == 1)
        #expect(byId["a"]?.top == 540 && byId["a"]?.bottom == 600)
    }

    @Test func shortAndLateEventsStayReadableAndInsideTheDay() {
        let day = ms("2026-10-05 00:00")
        let placed = math.layoutDay([
            event("short", "2026-10-05 12:00", "2026-10-05 12:05"),
            event("late", "2026-10-05 23:00", "2026-10-06 02:00"),
            event("early", "2026-10-04 22:00", "2026-10-05 01:00"),
        ], day: day)
        let byId = Dictionary(uniqueKeysWithValues: placed.map { ($0.event.id, $0) })
        #expect(byId["short"]?.bottom == 720 + CalendarMath.minMinutes)
        let lateBottom = byId["late"]?.bottom ?? -1
        #expect(lateBottom - 24 * 60 == 0)
        #expect(byId["early"]?.top == 0 && byId["early"]?.bottom == 60)
    }

    @Test func remindersAMomentApartDontHideEachOther() {
        let day = ms("2026-10-05 00:00")
        let tops = math.chipTops([ms("2026-10-05 09:00"), ms("2026-10-05 09:05"), ms("2026-10-05 23:59")], day: day, chipMinutes: 24)
        #expect(tops[0] == 540 - 12)
        #expect(tops[1] == tops[0] + 25)
        #expect(tops[2] == 24 * 60 - 24)
    }
}

struct ScheduleTests {
    let math = paris()

    private func roundTrip(_ s: Schedule) throws -> Schedule {
        try JSONDecoder().decode(Schedule.self, from: JSONEncoder().encode(s))
    }

    @Test func everyRuleSurvivesTheWire() throws {
        let all: [Schedule] = [
            .once(at: "2026-10-04T09:00"),
            .daily(time: "07:30"),
            .weekdays(time: "08:00"),
            .weekly(days: [.mon, .thu], time: "18:00"),
            .monthly(day: 31, time: "09:00"),
            .yearly(month: 2, day: 29, time: "10:00"),
            .interval(minutes: 90),
            .beforeEvent(eventId: "ev:1:2:3", eventTitle: "Dentist", minutesBefore: 30),
        ]
        for s in all { #expect(try roundTrip(s) == s) }
        let json = String(data: try JSONEncoder().encode(Schedule.beforeEvent(eventId: "ev:1", eventTitle: "X", minutesBefore: 5)), encoding: .utf8)!
        #expect(json.contains(#""type":"before_event""#) && json.contains(#""minutes_before":5"#))
    }

    @Test func newerRulesAndDaysDontBreakTheList() throws {
        let json = #"""
        [{"id":"01a10253-16d9-7450-bc1b-930dccaa57f7","kind":"reminder","title":"A","instruction":null,
          "schedule":{"type":"lunar","phase":"full"},"description":"Every full moon","paused":false,
          "next_at":null,"last_at":null,"ended":null,"conversation_id":null,"created_at":1,"something_new":true},
         {"id":"01a10253-16d9-7450-bc1b-930dccaa57f8","kind":"routine","title":"B","instruction":"Do it",
          "schedule":{"type":"weekly","days":["mon","someday"],"time":"07:00"},"description":"Mondays",
          "paused":true,"next_at":5,"last_at":null,"ended":null,"conversation_id":null,"created_at":2}]
        """#
        let items = try JSONDecoder().decode([ScheduleItem].self, from: Data(json.utf8))
        #expect(items[0].schedule == .unknown("lunar"))
        #expect(items[0].finished)
        #expect(items[1].schedule == .weekly(days: [.mon], time: "07:00"))
        #expect(!items[1].finished)
    }

    @Test func theFormReadsAndWritesEveryRule() {
        let now = math.date(ms("2026-10-03 17:12"))
        for s: Schedule in [
            .once(at: "2026-10-04T09:15"),
            .daily(time: "07:30"),
            .weekdays(time: "08:00"),
            .weekly(days: [.mon, .thu], time: "18:00"),
            .monthly(day: 31, time: "09:00"),
            .yearly(month: 12, day: 25, time: "10:00"),
            .interval(minutes: 120),
            .interval(minutes: 45),
            .beforeEvent(eventId: "ev:1", eventTitle: "Dentist", minutesBefore: 60),
        ] {
            #expect(ScheduleDraft(schedule: s, now: now, math: math).schedule(math: math) == s)
        }
        #expect(ScheduleDraft(schedule: .interval(minutes: 120), now: now, math: math).unit == .hours)
    }

    @Test func aNewReminderStartsAtTheNextFullHour() {
        let draft = ScheduleDraft(now: math.date(ms("2026-10-03 17:12")), math: math)
        #expect(draft.schedule(math: math) == .once(at: "2026-10-03T18:00"))
    }

    @Test func incompleteFormsDontSave() {
        var draft = ScheduleDraft(now: Date(), math: math)
        draft.repeatKind = .weekly
        draft.days = []
        #expect(draft.schedule(math: math) == nil)
        draft.repeatKind = .beforeEvent
        #expect(draft.schedule(math: math) == nil)
        draft.repeatKind = .weekly
        draft.days = [.sun, .mon]
        // Always in the week's order, whatever the tapping order.
        #expect(draft.schedule(math: math) == .weekly(days: [.mon, .sun], time: math.hhmm(draft.date)))
    }

    @Test func plainWords() {
        #expect(ScheduleWords.beforeLabel(10) == "10 minutes before")
        #expect(ScheduleWords.beforeLabel(120) == "2 hours before")
        #expect(ScheduleWords.beforeLabel(2880) == "2 days before")
        #expect(ScheduleWords.beforeLabel(0) == "When it starts")
        #expect(ScheduleWords.joinNames(["Sam"]) == "Sam")
        #expect(ScheduleWords.joinNames(["Sam", "Léa"]) == "Sam and Léa")
        #expect(ScheduleWords.joinNames(["Sam", "Léa", "Tom"]) == "Sam, Léa and Tom")
        #expect(ScheduleWords.status(.late) == "Sent late")
    }

    @Test func remindersSayWhenTheyreNext() {
        let now = ms("2026-10-03 17:00")
        let daily = ScheduleItem(title: "Vitamins", schedule: .daily(time: "08:00"), description: "Every day at 08:00", nextAt: ms("2026-10-04 08:00"))
        #expect(ScheduleWords.detail(daily, math: math, now: now) == "Tomorrow at \(math.clock(ms("2026-10-04 08:00"))) · Every day at 08:00")
        let once = ScheduleItem(title: "Bank", schedule: .once(at: "2026-10-03T19:00"), nextAt: ms("2026-10-03 19:00"))
        #expect(ScheduleWords.detail(once, math: math, now: now) == "Today at 19:00")
        let gone = ScheduleItem(title: "Bank", schedule: .once(at: "x"), lastAt: ms("2026-10-02 09:00"))
        #expect(ScheduleWords.detail(gone, math: math, now: now) == "Went off yesterday at \(math.clock(ms("2026-10-02 09:00")))")
        let ended = ScheduleItem(title: "X", schedule: .beforeEvent(eventId: "e", eventTitle: "t", minutesBefore: 5), ended: "The event was cancelled")
        #expect(ScheduleWords.detail(ended, math: math, now: now) == "The event was cancelled")
    }
}

struct CalendarDecodingTests {
    @Test func eventsReadWhatTheComputerSends() throws {
        let json = #"""
        {"events":[{"id":"ev:1","calendar_id":"c:1","calendar":"Work","title":"Design review","start":1,"end":2,
          "all_day":false,"location":"","notes":"https://example.org","organizer":{"name":null,"email":"me@example.org",
          "person_id":null,"person_name":null},"attendees":[{"name":"Sam","email":"me@example.org","person_id":null,
          "person_name":null,"response":"accepted"},{"name":null,"email":"sam@example.org",
          "person_id":"01a10253-16d9-7450-bc1b-930dccaa57f7","person_name":"Sam Carter","response":"maybe_later"}],
          "repeats":true,"mine":true,"future_field":1}],"unavailable":["Google: signed out"]}
        """#
        let found = try JSONDecoder().decode(CalendarEvents.self, from: Data(json.utf8))
        let e = try #require(found.events.first)
        #expect(e.location == nil)
        #expect(e.notes == "https://example.org")
        #expect(e.guests.map(\.email) == ["sam@example.org"])
        #expect(e.guests.first?.displayName == "Sam Carter")
        #expect(e.guests.first?.response == .unknown)
        #expect(e.canInvite)
        // The organizer once, even when listed as an attendee too.
        #expect(e.people.map(\.person.email) == ["me@example.org", "sam@example.org"])
        #expect(e.people.first?.organizer == true)
        #expect(found.unavailable == ["Google: signed out"])
    }

    @Test func newEventsAndChangesLeaveOutWhatIsntSet() throws {
        let change = EventChange(title: "New name")
        let json = String(data: try JSONEncoder().encode(change), encoding: .utf8)!
        #expect(json == #"{"title":"New name"}"#)
        let new = NewCalendarEvent(calendarId: "c", title: "T", start: 1, end: 2, allDay: true, location: nil, notes: nil, guests: ["Sam <sam@example.org>"])
        let body = try JSONSerialization.jsonObject(with: JSONEncoder().encode(new)) as! [String: Any]
        #expect(body["calendar_id"] as? String == "c")
        #expect(body["all_day"] as? Bool == true)
        #expect(body["location"] == nil)
    }

    @Test func offersAndDeliveries() throws {
        let offer = try JSONDecoder().decode(InvitationOffer.self, from: Data(#"""
        {"id":"01a10253-16d9-7450-bc1b-930dccaa57f7","kind":"cancel","event_title":"Dinner","event_when":"on Friday",
         "guests":[{"name":"Sam","email":"sam@example.org"},{"name":null,"email":"lea@example.org"}],"from":null,
         "from_note":null,"sent_at":null}
        """#.utf8))
        #expect(ScheduleWords.offerLabel(offer) == "Tell Sam and lea@example.org it's cancelled")
        #expect(ScheduleWords.sentLabel(offer) == "Told Sam and lea@example.org it's cancelled")
        let d = try JSONDecoder().decode(Delivery.self, from: Data(#"""
        {"id":"01a10253-16d9-7450-bc1b-930dccaa57f7","item_id":"01a10253-16d9-7450-bc1b-930dccaa57f8","kind":"reminder",
         "title":"Oven","due_at":1,"at":2,"status":"teleported","detail":null,"conversation_id":null}
        """#.utf8))
        #expect(d.status == .unknown)
    }
}

struct GuestTests {
    @Test func guestsAreWrittenAsTheComputerReadsThem() {
        #expect(GuestEntry(name: "Sam Carter", email: "sam@example.org").text == "Sam Carter <sam@example.org>")
        #expect(GuestEntry(name: "Evil <x@y.z>, \"q\"", email: "e@example.org").text == "Evil x@y.z q <e@example.org>")
        #expect(GuestEntry(name: nil, email: "lea@example.org").text == "lea@example.org")
    }

    @Test func onlyWholeAddressesCount() {
        #expect(GuestEntry.isEmail("sam@example.org"))
        #expect(GuestEntry.isEmail("sam.c+tag@mail.example.co.uk"))
        #expect(!GuestEntry.isEmail("sam"))
        #expect(!GuestEntry.isEmail("sam@example"))
        #expect(!GuestEntry.isEmail("sam@@example.org"))
        #expect(!GuestEntry.isEmail("sam @example.org"))
        #expect(!GuestEntry.isEmail("sam@example..org"))
        #expect(!GuestEntry.isEmail("<sam@example.org>"))
    }

    @Test func calendarColoursBecomeQuietTints() {
        let green = RGB(hex: "#34c759")!
        #expect(RGB(hex: "34C759") == green)
        #expect(RGB(hex: "#xyz") == nil)
        let tint = green.mixed(0.16, with: .white)
        #expect(tint.r > 0.85 && tint.g > 0.9 && tint.b > 0.85)
    }
}
