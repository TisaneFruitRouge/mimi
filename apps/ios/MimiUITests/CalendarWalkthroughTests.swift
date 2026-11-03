import XCTest

/// Walks through Calendar, reminders and Settings › Reminders & notifications against a
/// running computer, taking screenshots. Skipped unless a pairing link is given:
/// `TEST_RUNNER_MIMI_PAIRING_LINK='mimi://pair?…' xcodebuild test -only-testing:MimiUITests/CalendarWalkthroughTests …`
/// The computer needs a writable calendar with a few events today, and a person with an
/// email address in People named Sam (for the guest suggestion).
final class CalendarWalkthroughTests: XCTestCase {
    @MainActor
    func testCalendar() throws {
        guard let link = ProcessInfo.processInfo.environment["MIMI_PAIRING_LINK"].flatMap({ $0.isEmpty ? nil : $0 }) else {
            throw XCTSkip("set TEST_RUNNER_MIMI_PAIRING_LINK")
        }
        let app = XCUIApplication()
        app.launch()
        pairIfNeeded(app, link: link)

        let calendarTab = app.tabBars.buttons["Calendar"].firstMatch
        XCTAssertTrue(calendarTab.waitForExistence(timeout: 30))
        calendarTab.tap()

        // The day, with its events.
        let event = app.buttons.matching(identifier: "calendar-event").firstMatch
        XCTAssertTrue(event.waitForExistence(timeout: 30))
        sleep(2)
        shot(app, "cal-1-day")

        // An event's details.
        let hittable = app.buttons.matching(identifier: "calendar-event").allElementsBoundByIndex.first { $0.isHittable } ?? event
        hittable.tap()
        let ask = app.buttons.containing(NSPredicate(format: "label BEGINSWITH 'Ask '")).firstMatch
        XCTAssertTrue(ask.waitForExistence(timeout: 10))
        sleep(1)
        shot(app, "cal-2-event")
        app.buttons["Close"].firstMatch.tap()

        // The design review has notes, people and a reminder menu: open it full height.
        sleep(1)
        let review = app.buttons.containing(NSPredicate(format: "label BEGINSWITH 'Design review'")).firstMatch
        if review.waitForExistence(timeout: 3) && review.isHittable {
            review.tap()
            XCTAssertTrue(ask.waitForExistence(timeout: 10))
            app.swipeUp()
            sleep(1)
            shot(app, "cal-3-event-people")
            app.buttons["Close"].firstMatch.tap()
            sleep(1)
        }

        // The list of what's coming.
        app.buttons["View options"].firstMatch.tap()
        app.buttons["List"].firstMatch.tap()
        sleep(2)
        shot(app, "cal-4-list")
        app.buttons["View options"].firstMatch.tap()
        app.buttons["Day"].firstMatch.tap()
        sleep(1)

        // A new event, with a guest from People.
        app.buttons["Add"].firstMatch.tap()
        app.buttons["New event"].firstMatch.tap()
        let title = app.textFields["event-title"]
        XCTAssertTrue(title.waitForExistence(timeout: 10))
        title.tap()
        title.typeText("Coffee with Sam")
        let guests = app.textFields["guest-field"]
        if !guests.isHittable { app.swipeUp() }
        guests.tap()
        guests.typeText("Sam")
        let suggestion = app.buttons.containing(NSPredicate(format: "label BEGINSWITH 'Invite Sam'")).firstMatch
        XCTAssertTrue(suggestion.waitForExistence(timeout: 10))
        shot(app, "cal-5-guest-suggestion")
        suggestion.tap()
        sleep(1)
        shot(app, "cal-6-new-event")
        app.buttons["event-save"].firstMatch.tap()
        XCTAssertTrue(app.staticTexts["Nothing has been emailed to your guests yet."].waitForExistence(timeout: 20))
        sleep(1)
        shot(app, "cal-7-saved-with-guests")
        app.buttons["Done"].firstMatch.tap()
        sleep(1)

        // The next day, by swiping.
        let from = app.coordinate(withNormalizedOffset: CGVector(dx: 0.85, dy: 0.62))
        let to = app.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.62))
        from.press(forDuration: 0.05, thenDragTo: to, withVelocity: .fast, thenHoldForDuration: 0)
        sleep(2)
        shot(app, "cal-8-next-day")
        let today = app.buttons["Today"].firstMatch
        XCTAssertTrue(today.waitForExistence(timeout: 5), "swiping should move to another day")
        today.tap()
        sleep(1)

        // Reminders and routines.
        app.buttons["Reminders and routines"].firstMatch.tap()
        XCTAssertTrue(app.navigationBars["Reminders"].waitForExistence(timeout: 10))
        sleep(1)
        shot(app, "cal-9-reminders")
        app.buttons["New reminder or routine"].firstMatch.tap()
        let what = app.textFields["schedule-title"]
        XCTAssertTrue(what.waitForExistence(timeout: 10))
        what.tap()
        what.typeText("Buy flowers")
        sleep(1)
        shot(app, "cal-10-new-reminder")
        app.buttons["schedule-save"].firstMatch.tap()
        let row = app.staticTexts["Buy flowers"].firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 15))
        sleep(1)
        shot(app, "cal-11-reminder-added")
        row.swipeLeft()
        app.buttons["Delete"].firstMatch.tap()
        let undo = app.buttons["Undo"]
        XCTAssertTrue(undo.waitForExistence(timeout: 5))
        shot(app, "cal-12-undo")
        undo.tap()
        XCTAssertTrue(row.waitForExistence(timeout: 5))

        // A routine's editor, with its rule.
        app.staticTexts["Morning briefing"].firstMatch.tap()
        XCTAssertTrue(app.navigationBars["Change routine"].waitForExistence(timeout: 10))
        sleep(1)
        shot(app, "cal-13-routine")
        app.buttons["Cancel"].firstMatch.tap()

        // Settings › Reminders & notifications.
        app.tabBars.buttons["Settings"].firstMatch.tap()
        app.buttons["Reminders & notifications"].firstMatch.tap()
        XCTAssertTrue(app.switches["computer-notifications"].waitForExistence(timeout: 10))
        sleep(2)
        shot(app, "cal-14-notifications")
    }

    @MainActor
    private func pairIfNeeded(_ app: XCUIApplication, link: String) {
        let paste = app.buttons["Paste a pairing link instead"]
        guard paste.waitForExistence(timeout: 5) else { return }
        paste.tap()
        let field = app.textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        field.typeText(link)
        app.alerts.buttons["Continue"].tap()
        let pair = app.buttons["Pair"]
        XCTAssertTrue(pair.waitForExistence(timeout: 5))
        pair.tap()
    }

    @MainActor
    private func shot(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
