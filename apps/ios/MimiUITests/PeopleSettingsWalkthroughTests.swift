import XCTest

/// Walks through People and every Settings page against a running computer, taking
/// screenshots. Skipped unless a pairing link is given (see `WalkthroughTests`); the
/// computer needs a few people (two of them the same person under different names, "Tom
/// Becker" and "Thomas Becker", and two "Sam Carter"s), a profile and some memory notes.
final class PeopleSettingsWalkthroughTests: XCTestCase {
    private var app: XCUIApplication!

    @MainActor
    func testPeopleAndSettings() throws {
        continueAfterFailure = false
        guard let link = ProcessInfo.processInfo.environment["MIMI_PAIRING_LINK"], !link.isEmpty else {
            throw XCTSkip("set TEST_RUNNER_MIMI_PAIRING_LINK")
        }
        app = XCUIApplication()
        app.launch()
        try pairIfNeeded(link)

        // People.
        tap(app.tabBars.buttons["People"], until: app.staticTexts["Léa Martin"])
        sleep(1)
        shot("p1-people")

        // Possible duplicates.
        let dupes = app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'possible duplicate'")).firstMatch
        if dupes.waitForExistence(timeout: 20) {
            dupes.tap()
            XCTAssertTrue(app.buttons["Same person"].firstMatch.waitForExistence(timeout: 30))
            sleep(1)
            shot("p2-duplicates")
            back()
        }

        // A person.
        tap(app.staticTexts["Léa Martin"])
        XCTAssertTrue(app.staticTexts["How to reach them"].waitForExistence(timeout: 30)
            || app.staticTexts["HOW TO REACH THEM"].waitForExistence(timeout: 6))
        sleep(2)
        shot("p3-person")
        app.swipeUp()
        sleep(1)
        shot("p4-person-more")
        // Handle actions.
        app.swipeDown()
        app.swipeDown()
        // The row is one button labelled with everything it shows.
        let phone = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "Phone, +41 79 123 45 67")).firstMatch
        if phone.waitForExistence(timeout: 10) {
            phone.tap()
            sleep(1)
            shot("p5-handle-menu")
            dismissMenu()
        }
        back()

        // Merge two people: choose them, then the preview.
        tap(app.buttons["More"])
        XCTAssertTrue(app.buttons["Select to merge"].waitForExistence(timeout: 20))
        app.buttons["Select to merge"].tap()
        sleep(1)
        // Rows further down only exist once the list scrolls to them.
        let tom = app.staticTexts["Tom Becker"]
        for _ in 0..<4 where !(tom.waitForExistence(timeout: 3) && tom.isHittable) {
            app.swipeUp()
        }
        tap(tom)
        tap(app.staticTexts["Thomas Becker"])
        sleep(1)
        shot("p6-select-to-merge")
        tap(app.buttons["Merge 2 contacts"])
        let merge = app.navigationBars.buttons["Merge"]
        XCTAssertTrue(merge.waitForExistence(timeout: 30))
        sleep(2)
        shot("p7-merge-sheet")
        merge.tap()
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'is one contact now'")).firstMatch.waitForExistence(timeout: 30))
        sleep(1)
        shot("p8-merged")
        // Undo it, so the walkthrough can run again.
        tap(app.buttons["Undo"])
        sleep(2)
        shot("p9-merge-undone")
        back()

        // Add someone.
        tap(app.buttons["Add someone"])
        XCTAssertTrue(app.navigationBars["Add someone"].waitForExistence(timeout: 20))
        sleep(1)
        shot("p10-add-someone")
        tap(app.navigationBars.buttons["Cancel"])

        // Settings.
        tap(app.tabBars.buttons["Settings"], until: app.staticTexts["General"])
        sleep(1)
        shot("s1-settings")

        visit("General", "s2-general")
        visit("Personality", "s3-personality", scroll: true)
        visit("Connections", "s4-connections", scroll: true) {
            let telegram = self.app.staticTexts["Telegram"].firstMatch
            if telegram.waitForExistence(timeout: 20) {
                telegram.tap()
                sleep(1)
                self.shot("s4c-telegram-sheet")
                let connect = self.app.buttons["Connect Telegram"]
                if connect.waitForExistence(timeout: 10) {
                    connect.tap()
                    sleep(1)
                    self.shot("s4d-telegram-connect")
                    self.back()
                }
                self.tap(self.app.buttons["Close"].firstMatch)
                sleep(1)
            }
            let email = self.app.staticTexts["Email"].firstMatch
            if email.waitForExistence(timeout: 10) {
                email.tap()
                let connect = self.app.buttons["Connect Email"]
                if connect.waitForExistence(timeout: 10) {
                    connect.tap()
                    let field = self.app.textFields["you@example.com"]
                    if field.waitForExistence(timeout: 10) {
                        field.tap()
                        field.typeText("me@fastmail.com")
                        sleep(3)
                        self.shot("s4e-email-connect")
                    }
                    self.back()
                }
                self.tap(self.app.buttons["Close"].firstMatch)
                sleep(1)
            }
        }
        visit("Models", "s5-models", scroll: true) {
            let change = self.app.buttons["Change model"]
            if change.waitForExistence(timeout: 10) {
                change.tap()
                sleep(1)
                self.shot("s5c-choose-model")
                self.back()
            }
            self.app.swipeUp()
            self.app.swipeUp()
            let add = self.app.buttons["Add a model source"]
            if add.waitForExistence(timeout: 10) {
                add.tap()
                sleep(1)
                self.shot("s5d-add-source")
                self.tap(self.app.navigationBars.buttons["Cancel"])
            }
        }
        visit("Memory", "s6-memory", scroll: true) {
            let note = self.app.staticTexts["Mornings"]
            if !note.isHittable { self.app.swipeDown() }
            if note.waitForExistence(timeout: 10) {
                note.tap()
                sleep(1)
                self.shot("s6c-memory-note")
                self.back()
            }
        }
        visit("Permissions", "s7-permissions", scroll: true)
        visit("Privacy", "s8-privacy", scroll: true)
        visit("This phone", "s9-this-phone")
    }

    // MARK: Helpers

    @MainActor
    private func visit(_ row: String, _ name: String, scroll: Bool = false, then extra: (() -> Void)? = nil) {
        let cell = app.staticTexts[row].firstMatch
        if !cell.isHittable { app.swipeUp() }
        XCTAssertTrue(cell.waitForExistence(timeout: 30), "no \(row) row")
        cell.tap()
        sleep(2)
        shot(name)
        if scroll {
            app.swipeUp()
            sleep(1)
            shot("\(name)-more")
            app.swipeDown()
            app.swipeDown()
        }
        extra?()
        back()
        sleep(1)
    }

    /// Waits for the element (screens take a while to settle on a busy machine), then taps it.
    @MainActor
    private func tap(_ element: XCUIElement, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(element.waitForExistence(timeout: 30), "\(element) never appeared", file: file, line: line)
        element.tap()
    }

    /// Taps until `result` shows: a busy simulator sometimes swallows a tap.
    @MainActor
    private func tap(_ element: XCUIElement, until result: XCUIElement, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(element.waitForExistence(timeout: 30), "\(element) never appeared", file: file, line: line)
        for _ in 0..<3 {
            element.tap()
            if result.waitForExistence(timeout: 20) { return }
        }
        XCTFail("\(result) never appeared", file: file, line: line)
    }

    @MainActor
    private func back() {
        let button = app.navigationBars.buttons["BackButton"].firstMatch
        guard button.waitForExistence(timeout: 10) else {
            let first = app.navigationBars.buttons.element(boundBy: 0)
            if first.waitForExistence(timeout: 10) { first.tap() }
            return
        }
        // A busy simulator sometimes swallows the tap: check the page went, else tap again.
        let page = app.navigationBars[app.navigationBars.firstMatch.identifier]
        for _ in 0..<3 {
            if button.exists && button.isHittable { button.tap() }
            if page.waitForNonExistence(timeout: 10) { return }
        }
    }

    @MainActor
    private func dismissMenu() {
        // A menu closes with a tap outside it (not on the status bar, which doesn't count).
        let copy = app.buttons["Copy"].firstMatch
        for _ in 0..<3 {
            let region = app.otherElements["PopoverDismissRegion"].firstMatch
            if region.exists {
                region.tap()
            } else {
                app.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.6)).tap()
            }
            if copy.waitForNonExistence(timeout: 5) { return }
        }
    }

    @MainActor
    private func pairIfNeeded(_ link: String) throws {
        let paste = app.buttons["Paste a pairing link instead"]
        guard paste.waitForExistence(timeout: 20) else { return }
        paste.tap()
        let field = app.textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 20))
        // On a busy machine the alert's field may not have focus yet.
        field.tap()
        field.typeText(link)
        app.alerts.buttons["Continue"].tap()
        let pair = app.buttons["Pair"]
        XCTAssertTrue(pair.waitForExistence(timeout: 20))
        // On a busy machine the sheet can still be settling (keyboard going away) and
        // swallow the tap: tap again while it still says "Pair" and isn't connecting.
        let people = app.tabBars.buttons["People"]
        for _ in 0..<3 {
            if pair.exists && pair.isHittable { pair.tap() }
            if people.waitForExistence(timeout: 30) { return }
        }
    }

    @MainActor
    private func shot(_ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
