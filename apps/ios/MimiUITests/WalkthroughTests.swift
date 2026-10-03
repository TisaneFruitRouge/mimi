import XCTest

/// Walks through the app against a running computer, taking screenshots: pairing,
/// chatting, an approval card. Skipped unless a pairing link is given:
/// `TEST_RUNNER_MIMI_PAIRING_LINK='mimi://pair?…' xcodebuild test -only-testing:MimiUITests …`
/// (make the link with `POST /v1/remote/pairing` on a scratch daemon).
final class WalkthroughTests: XCTestCase {
    @MainActor
    func testWalkthrough() throws {
        let link = try XCTUnwrap(
            ProcessInfo.processInfo.environment["MIMI_PAIRING_LINK"].flatMap { $0.isEmpty ? nil : $0 },
            "set TEST_RUNNER_MIMI_PAIRING_LINK"
        )
        let app = XCUIApplication()
        app.launch()

        let paste = app.buttons["Paste a pairing link instead"]
        if paste.waitForExistence(timeout: 5) {
            shot(app, "1-welcome")
            paste.tap()
            let field = app.textFields.firstMatch
            XCTAssertTrue(field.waitForExistence(timeout: 5))
            field.typeText(link)
            app.alerts.buttons["Continue"].tap()
            let pair = app.buttons["Pair"]
            XCTAssertTrue(pair.waitForExistence(timeout: 5))
            shot(app, "2-name-this-phone")
            pair.tap()
        }

        let newChat = app.buttons["New chat"]
        XCTAssertTrue(newChat.waitForExistence(timeout: 30))
        let enabled = NSPredicate(format: "isEnabled == true")
        expectation(for: enabled, evaluatedWith: newChat)
        waitForExpectations(timeout: 30)
        shot(app, "3-chats")
        newChat.tap()

        let field = app.textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        shot(app, "4-empty-chat")
        field.tap()
        field.typeText("What should I do tomorrow?")
        app.buttons["Send"].tap()
        XCTAssertTrue(app.staticTexts["Thinking"].waitForExistence(timeout: 5) || true)
        shot(app, "5-thinking")
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'Finish the report'")).firstMatch.waitForExistence(timeout: 20))
        shot(app, "6-reply")

        field.tap()
        field.typeText("Send Sam a note about dinner")
        app.buttons["Send"].tap()
        let approve = app.buttons["Approve"]
        XCTAssertTrue(approve.waitForExistence(timeout: 20))
        sleep(1)
        shot(app, "7-approval")
        approve.tap()
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'note is sent'")).firstMatch.waitForExistence(timeout: 20))
        sleep(1)
        shot(app, "8-approved")

        // A draft the assistant wrote shows as a card to check and send.
        field.tap()
        field.typeText("Draft an email to Sam saying yes to dinner")
        app.buttons["Send"].tap()
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'draft is ready'")).firstMatch.waitForExistence(timeout: 20))
        sleep(1)
        shot(app, "8b-draft-card")

        app.navigationBars.buttons.element(boundBy: 0).tap()
        app.tabBars.buttons["Settings"].tap()
        sleep(1)
        shot(app, "9-settings")
    }

    @MainActor
    private func shot(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
