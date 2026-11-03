import Foundation
import Testing
@testable import Mimi

struct PairingLinkTests {
    let id = String(repeating: "ab", count: 32)

    @Test func readsTheComputersLink() throws {
        let link = try #require(PairingLink(string: "mimi://pair?id=\(id)&code=0123abcd"))
        #expect(link.endpointId == id)
        #expect(link.code == "0123abcd")
        #expect(link.relay == nil)
    }

    @Test func keepsTheUsersOwnRelay() throws {
        let link = try #require(PairingLink(string: "mimi://pair?id=\(id)&code=1&relay=https%3A%2F%2Fr.example%2F"))
        #expect(link.relay == "https://r.example/")
    }

    @Test func refusesAnythingElse() {
        #expect(PairingLink(string: "https://pair?id=\(id)&code=1") == nil)
        #expect(PairingLink(string: "mimi://pair?id=short&code=1") == nil)
        #expect(PairingLink(string: "mimi://pair?id=\(id)") == nil)
        #expect(PairingLink(string: "mimi://pair?id=\(id)&code=1&relay=file%3A%2F%2Fx")?.relay == nil)
    }
}

struct EventTests {
    func decode(_ json: String) throws -> Event {
        try JSONDecoder().decode(Event.self, from: Data(json.utf8))
    }

    @Test func deltasCarryTheirMessage() throws {
        let c = UUID(), m = UUID()
        let event = try decode(#"{"type":"message_delta","conversation_id":"\#(c)","message_id":"\#(m)","content":"Hi","reasoning":""}"#)
        guard case .messageDelta(let conversation, let message, let content, _) = event else {
            Issue.record("wrong event"); return
        }
        #expect(conversation == c && message == m && content == "Hi")
    }

    @Test func unknownEventsJustSaySomethingChanged() throws {
        guard case .changed(let kind) = try decode(#"{"type":"mail_changed"}"#) else {
            Issue.record("wrong event"); return
        }
        #expect(kind == "mail_changed")
        guard case .changed = try decode(#"{"type":"something_new","x":1}"#) else {
            Issue.record("wrong event"); return
        }
    }

    @Test func messagesKeepToolOutputAsItIs() throws {
        let json = #"""
        {"type":"message_updated","message":{"id":"\#(UUID())","conversation_id":"\#(UUID())","role":"assistant",
         "content":"Done.","reasoning":"","status":"complete","model":null,"locality":"cloud","error":null,
         "created_at":1,"mentions":[],"actions":[{"id":"\#(UUID())","tool":"reminder_add","summary":"remind you",
         "arguments":{"when_text":"tomorrow"},"requires_approval":false,"status":"done","result":"reminder set",
         "error":null,"output":{"schedule_revision":7},"call_id":"c","round":0,"content_offset":0}]}}
        """#
        guard case .messageUpdated(let m) = try decode(json) else { Issue.record("wrong event"); return }
        #expect(m.locality == .cloud)
        #expect(m.actions.first?.scheduleRevision == 7)
        #expect(m.actions.first?.arguments["when_text"]?.string == "tomorrow")
    }
}

struct MarkdownTests {
    @Test func splitsBlocks() {
        let blocks = MarkdownText.blocks("""
        # Plan
        First line
        still the same paragraph

        - one
        - two
        1. first
        2. second
        > quoted
        ```
        code here
        ```
        """)
        #expect(blocks == [
            .heading(1, "Plan"),
            .paragraph("First line\nstill the same paragraph"),
            .list(["one", "two"], ordered: false),
            .list(["first", "second"], ordered: true),
            .quote("quoted"),
            .code("code here"),
        ])
    }

    @Test func onlyWebLinksStayLinks() {
        let text = MarkdownText.inline("[a](https://example.org) and [b](file:///etc/passwd)")
        let links = text.runs.compactMap(\.link)
        #expect(links == [URL(string: "https://example.org")!])
    }
}

struct ActionArgumentTests {
    @Test func emailCardsShowTheWholeMessage() {
        let rows = ActionArguments.rows(tool: "mail_send", arguments: .object([
            "to": .array([.string("sam@example.org")]),
            "subject": .string(""),
            "body": .string("Hello"),
            "thread_id": .number(3),
        ]))
        #expect(rows.map(\.label) == ["To", "Subject", "Message"])
        #expect(rows[1].value == "(no subject)")
    }

    @Test func unknownToolsListTheirArguments() {
        let rows = ActionArguments.rows(tool: "dev_send_note", arguments: .object(["note_text": .string("hi")]))
        #expect(rows == [ActionArguments.Row(label: "Note text", value: "hi")])
    }
}
