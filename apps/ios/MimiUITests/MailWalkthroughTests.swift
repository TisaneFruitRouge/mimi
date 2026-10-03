import XCTest

/// Walks through Mail against a running computer with a mailbox, taking screenshots:
/// the mailboxes, a sorted view, a conversation in each display mode, a reply drafted by
/// the assistant and sent, a smart folder, a draft card in the chat, and Privacy. Skipped
/// unless a pairing link is given (see `WalkthroughTests`); the computer needs the fake
/// mailbox (`MIMI_FAKE_MAIL_SEED=1 cargo test -p mimi-core fake_mail_server -- --ignored`)
/// and a model that sorts it and calls `mail_compose` when asked to draft an email.
final class MailWalkthroughTests: XCTestCase {
    @MainActor
    func testMail() throws {
        guard let link = ProcessInfo.processInfo.environment["MIMI_PAIRING_LINK"], !link.isEmpty else {
            throw XCTSkip("set TEST_RUNNER_MIMI_PAIRING_LINK")
        }
        let app = XCUIApplication()
        app.launch()
        pairIfNeeded(app, link: link)

        let mailTab = app.tabBars.buttons["Mail"]
        // Reaching the computer the first time can take a while through a relay.
        XCTAssertTrue(mailTab.waitForExistence(timeout: 180))
        select(mailTab)

        // It opens on a view with the mailboxes one step back.
        XCTAssertTrue(row(app, "Dinner on Thursday?").waitForExistence(timeout: 60))
        sleep(1)
        shot(app, "mail-1-needs-reply")

        app.navigationBars.buttons.element(boundBy: 0).tap()
        XCTAssertTrue(app.staticTexts["Smart folders"].waitForExistence(timeout: 10))
        sleep(1)
        shot(app, "mail-2-mailboxes")

        open(row(app, "Inbox"), until: row(app, "Weekend plans"))
        sleep(1)
        shot(app, "mail-3-inbox")

        // A newsletter as it was sent, then the conversation with a reply in each mode.
        let mode = app.segmentedControls["mail-mode"]
        open(row(app, "This week's baskets"), until: mode)
        mode.buttons["Original"].tap()
        // The first email takes a moment to draw while the web content starts.
        sleep(8)
        shot(app, "mail-4-original-newsletter")
        app.navigationBars.buttons.element(boundBy: 0).tap()

        XCTAssertTrue(row(app, "Weekend plans").waitForExistence(timeout: 30))
        open(row(app, "Weekend plans"), until: mode)
        sleep(3)
        shot(app, "mail-5-original")
        mode.buttons["Formatted"].tap()
        sleep(2)
        shot(app, "mail-6-formatted")
        mode.buttons["Text"].tap()
        sleep(1)
        shot(app, "mail-7-text")
        mode.buttons["Original"].tap()
        app.navigationBars.buttons.element(boundBy: 0).tap()

        // The suspicious one.
        reveal(app, row(app, "Quick favour"))
        open(row(app, "Quick favour"), until: app.staticTexts["This email looks suspicious"])
        sleep(1)
        shot(app, "mail-8-suspicious")
        app.navigationBars.buttons.element(boundBy: 0).tap()

        // A reply drafted by the assistant, then sent.
        for _ in 0..<3 where !row(app, "Dinner on Thursday?").isHittable { app.swipeDown() }
        XCTAssertTrue(row(app, "Dinner on Thursday?").waitForExistence(timeout: 10))
        let more = app.navigationBars.buttons["More"]
        open(row(app, "Dinner on Thursday?"), until: more)
        sleep(2)
        shot(app, "mail-9-conversation")
        more.tap()
        app.buttons["Summarize"].tap()
        XCTAssertTrue(app.staticTexts["Written by your model"].waitForExistence(timeout: 20))
        more.tap()
        let draft = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Draft a reply with'")).firstMatch
        XCTAssertTrue(draft.waitForExistence(timeout: 5))
        sleep(1) // let the menu finish opening
        draft.tap()
        let body = app.textViews["compose-body"].exists ? app.textViews["compose-body"] : app.textFields["compose-body"]
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'Chez Léon at 19:30 sounds lovely'")).firstMatch
            .waitForExistence(timeout: 20) || body.waitForExistence(timeout: 1))
        sleep(1)
        shot(app, "mail-10-reply-drafted")
        app.buttons["compose-send"].tap()
        XCTAssertTrue(app.staticTexts["Sent to sam@example.com"].waitForExistence(timeout: 20))
        shot(app, "mail-11-sent")
        sleep(3)
        app.navigationBars.buttons.element(boundBy: 0).tap()

        // A new message from scratch.
        let compose = app.buttons["New message"].firstMatch
        XCTAssertTrue(compose.waitForExistence(timeout: 10))
        compose.tap()
        let to = app.textFields["compose-to"].exists ? app.textFields["compose-to"] : app.textViews["compose-to"]
        XCTAssertTrue(to.waitForExistence(timeout: 10))
        to.typeText("priya@work.example")
        let message = app.textViews["compose-body"].exists ? app.textViews["compose-body"] : app.textFields["compose-body"]
        message.tap()
        message.typeText("Hi Priya,\n\nComments on the budget coming tomorrow morning.")
        sleep(1)
        shot(app, "mail-12-compose")
        app.buttons["compose-send"].tap()
        XCTAssertTrue(app.staticTexts["Sent to priya@work.example"].waitForExistence(timeout: 20))

        // A smart folder.
        app.navigationBars.buttons.element(boundBy: 0).tap()
        let folder = row(app, "Bills & orders")
        if folder.waitForExistence(timeout: 10) {
            folder.tap()
            sleep(2)
            shot(app, "mail-13-folder")
            app.navigationBars.buttons["Folder options"].tap()
            app.buttons["Edit"].tap()
            sleep(1)
            shot(app, "mail-14-folder-editor")
            app.buttons["Cancel"].tap()
            app.navigationBars.buttons.element(boundBy: 0).tap()
        }

        // A draft in the chat.
        select(app.tabBars.buttons["Chats"])
        let newChat = app.buttons["New chat"]
        XCTAssertTrue(newChat.waitForExistence(timeout: 10))
        newChat.tap()
        let field = app.textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        field.typeText("Draft an email to Sam saying yes to dinner")
        app.buttons["Send"].tap()
        XCTAssertTrue(app.staticTexts["Draft email · not sent"].waitForExistence(timeout: 30))
        app.scrollViews.firstMatch.swipeDown()
        sleep(2)
        shot(app, "mail-15-chat-draft")
        app.buttons["Edit"].tap()
        sleep(1)
        shot(app, "mail-16-chat-draft-edit")
        app.buttons["compose-send"].tap()
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH 'Sent “Dinner on Thursday”'")).firstMatch
            .waitForExistence(timeout: 20))
        sleep(1)
        shot(app, "mail-17-chat-draft-sent")

        // Privacy.
        app.navigationBars.buttons.element(boundBy: 0).tap()
        select(app.tabBars.buttons["Settings"])
        app.buttons["Email sorting"].tap()
        sleep(2)
        shot(app, "mail-18-privacy")
    }

    /// Taps a tab until it's the one shown (taps can get lost while the app is busy).
    @MainActor
    private func select(_ tab: XCUIElement) {
        for _ in 0..<4 where !tab.isSelected {
            tab.tap()
            let selected = XCTNSPredicateExpectation(predicate: NSPredicate(format: "isSelected == true"), object: tab)
            _ = XCTWaiter.wait(for: [selected], timeout: 8)
        }
        XCTAssertTrue(tab.isSelected)
    }

    /// Scrolls the list until the element is on screen.
    @MainActor
    private func reveal(_ app: XCUIApplication, _ element: XCUIElement) {
        for _ in 0..<6 where !(element.exists && element.isHittable) {
            app.swipeUp()
        }
    }

    /// Taps something until what it opens is there (taps can get lost while the app is busy).
    @MainActor
    private func open(_ element: XCUIElement, until shown: XCUIElement) {
        for _ in 0..<4 where !shown.exists {
            if element.exists { element.tap() }
            _ = shown.waitForExistence(timeout: 10)
        }
        XCTAssertTrue(shown.exists)
    }

    /// A row (conversation, mailbox, folder) by the text it shows.
    @MainActor
    private func row(_ app: XCUIApplication, _ text: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
    }

    @MainActor
    private func pairIfNeeded(_ app: XCUIApplication, link: String) {
        let paste = app.buttons["Paste a pairing link instead"]
        guard paste.waitForExistence(timeout: 5) else { return }
        let field = app.textFields.firstMatch
        // The machine may be slow: tap again if the alert didn't come.
        for _ in 0..<3 where !field.exists {
            paste.tap()
            _ = field.waitForExistence(timeout: 10)
        }
        XCTAssertTrue(field.exists)
        field.tap()
        field.typeText(link)
        app.alerts.buttons["Continue"].tap()
        let pair = app.buttons["Pair"]
        XCTAssertTrue(pair.waitForExistence(timeout: 15))
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
