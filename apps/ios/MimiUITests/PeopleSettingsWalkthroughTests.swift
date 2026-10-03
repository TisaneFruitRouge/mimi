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
        let peopleTab = app.tabBars.buttons["People"]
        XCTAssertTrue(peopleTab.waitForExistence(timeout: 30))
        peopleTab.tap()
        XCTAssertTrue(app.staticTexts["Léa Martin"].waitForExistence(timeout: 20))
        sleep(1)
        shot("p1-people")

        // Possible duplicates.
        let dupes = app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'possible duplicate'")).firstMatch
        if dupes.waitForExistence(timeout: 5) {
            dupes.tap()
            XCTAssertTrue(app.buttons["Same person"].firstMatch.waitForExistence(timeout: 10))
            sleep(1)
            shot("p2-duplicates")
            back()
        }

        // A person.
        app.staticTexts["Léa Martin"].tap()
        XCTAssertTrue(app.staticTexts["How to reach them"].waitForExistence(timeout: 10)
            || app.staticTexts["HOW TO REACH THEM"].waitForExistence(timeout: 2))
        sleep(2)
        shot("p3-person")
        app.swipeUp()
        sleep(1)
        shot("p4-person-more")
        // Handle actions.
        app.swipeDown()
        app.swipeDown()
        let phone = app.staticTexts["+41 79 123 45 67"]
        if phone.waitForExistence(timeout: 3) {
            phone.tap()
            sleep(1)
            shot("p5-handle-menu")
            dismissMenu()
        }
        back()

        // Merge two people: choose them, then the preview.
        app.buttons["More"].tap()
        XCTAssertTrue(app.buttons["Select to merge"].waitForExistence(timeout: 5))
        app.buttons["Select to merge"].tap()
        sleep(1)
        let tom = app.staticTexts["Tom Becker"]
        if !tom.isHittable { app.swipeUp() }
        tom.tap()
        app.staticTexts["Thomas Becker"].tap()
        sleep(1)
        shot("p6-select-to-merge")
        app.buttons["Merge 2 contacts"].tap()
        let merge = app.navigationBars.buttons["Merge"]
        XCTAssertTrue(merge.waitForExistence(timeout: 10))
        sleep(2)
        shot("p7-merge-sheet")
        merge.tap()
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'is one contact now'")).firstMatch.waitForExistence(timeout: 10))
        sleep(1)
        shot("p8-merged")
        // Undo it, so the walkthrough can run again.
        app.buttons["Undo"].tap()
        sleep(2)
        shot("p9-merge-undone")
        back()

        // Add someone.
        app.buttons["Add someone"].tap()
        XCTAssertTrue(app.navigationBars["Add someone"].waitForExistence(timeout: 5))
        sleep(1)
        shot("p10-add-someone")
        app.navigationBars.buttons["Cancel"].tap()

        // Settings.
        app.tabBars.buttons["Settings"].tap()
        XCTAssertTrue(app.staticTexts["General"].waitForExistence(timeout: 10))
        sleep(1)
        shot("s1-settings")

        visit("General", "s2-general")
        visit("Personality", "s3-personality", scroll: true)
        visit("Connections", "s4-connections", scroll: true) {
            let telegram = self.app.staticTexts["Telegram"].firstMatch
            if telegram.waitForExistence(timeout: 5) {
                telegram.tap()
                sleep(1)
                self.shot("s4c-telegram-sheet")
                let connect = self.app.buttons["Connect Telegram"]
                if connect.waitForExistence(timeout: 3) {
                    connect.tap()
                    sleep(1)
                    self.shot("s4d-telegram-connect")
                }
                self.app.buttons["Close"].firstMatch.tap()
                sleep(1)
            }
            let email = self.app.staticTexts["Email"].firstMatch
            if email.waitForExistence(timeout: 3) {
                email.tap()
                let connect = self.app.buttons["Connect Email"]
                if connect.waitForExistence(timeout: 3) {
                    connect.tap()
                    let field = self.app.textFields["you@example.com"]
                    if field.waitForExistence(timeout: 3) {
                        field.tap()
                        field.typeText("me@fastmail.com")
                        sleep(3)
                        self.shot("s4e-email-connect")
                    }
                }
                self.app.buttons["Close"].firstMatch.tap()
                sleep(1)
            }
        }
        visit("Models", "s5-models", scroll: true) {
            let change = self.app.buttons["Change model"]
            if change.waitForExistence(timeout: 3) {
                change.tap()
                sleep(1)
                self.shot("s5c-choose-model")
                self.back()
            }
            self.app.swipeUp()
            self.app.swipeUp()
            let add = self.app.buttons["Add a model source"]
            if add.waitForExistence(timeout: 3) {
                add.tap()
                sleep(1)
                self.shot("s5d-add-source")
                self.app.navigationBars.buttons["Cancel"].tap()
            }
        }
        visit("Memory", "s6-memory", scroll: true) {
            let note = self.app.staticTexts["Mornings"]
            if !note.isHittable { self.app.swipeDown() }
            if note.waitForExistence(timeout: 3) {
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
        XCTAssertTrue(cell.waitForExistence(timeout: 10), "no \(row) row")
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

    @MainActor
    private func back() {
        let button = app.navigationBars.buttons.element(boundBy: 0)
        if button.waitForExistence(timeout: 3) { button.tap() }
    }

    @MainActor
    private func dismissMenu() {
        // A context menu closes with a tap outside it.
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.05)).tap()
    }

    @MainActor
    private func pairIfNeeded(_ link: String) throws {
        let paste = app.buttons["Paste a pairing link instead"]
        guard paste.waitForExistence(timeout: 5) else { return }
        paste.tap()
        let field = app.textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.typeText(link)
        app.alerts.buttons["Continue"].tap()
        let pair = app.buttons["Pair"]
        XCTAssertTrue(pair.waitForExistence(timeout: 5))
        pair.tap()
    }

    @MainActor
    private func shot(_ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
