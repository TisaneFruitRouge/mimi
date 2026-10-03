import Foundation
import Testing
@testable import Mimi

struct MailDecodingTests {
    let account = "01a1025a-1eff-773c-aff8-3b3284eee288"

    @Test func readsConversationsAsTheComputerSendsThem() throws {
        let json = #"""
        [{"id":10,"connection_id":"\#(account)","received_on":"shop@example.org","suspicious":false,"folders":[1],
          "subject":"Your order has shipped","participants":[{"name":"Bookshop","email":"orders@bookshop.example"},{"name":null,"email":"x@y.example"}],
          "last_at":1791035246000,"message_count":2,"unread":true,"flagged":false,"category":"important",
          "summary":"Your order is on its way.","snippet":"Hello","automated":true,"last_from_me":false,"something_new":42},
         {"id":11,"connection_id":"\#(account)","received_on":null,"suspicious":true,"folders":[],"subject":"",
          "participants":[],"last_at":1,"message_count":1,"unread":false,"flagged":true,"category":"a_new_one",
          "summary":null,"snippet":"","automated":false,"last_from_me":true}]
        """#
        let threads = try JSONDecoder().decode([MailThread].self, from: Data(json.utf8))
        #expect(threads.count == 2)
        #expect(threads[0].category == .important)
        #expect(threads[0].folders == [1])
        #expect(threads[0].participants[1].shortName == "x")
        // A category this app doesn't know just isn't shown.
        #expect(threads[1].category == nil)
        #expect(MailText.who(threads[0]) == "Bookshop, x")
        #expect(MailText.who(threads[1]) == "Just you")
        #expect(MailText.subject(threads[1].subject) == "(no subject)")
    }

    @Test func readsTheOverview() throws {
        let json = #"""
        {"accounts":[{"connection_id":"\#(account)","email":"me@example.org","name":"me@example.org",
          "addresses":[{"email":"me@example.org","threads":8,"unread":2},{"email":"shop@example.org","threads":1,"unread":1}]}],
         "needs_reply":3,"important":1,"unread":4,"sorting":true,"model_locality":"device","sorter":"jev",
         "sorter_locality":"cloud","jev_connected":true,
         "folders":[{"id":1,"name":"Bills","description":"Bills","icon":"receipt","color":"orange","threads":2,"unread":0,"to_check":5}]}
        """#
        let o = try JSONDecoder().decode(MailOverview.self, from: Data(json.utf8))
        #expect(o.sorter == .jev && o.sorterLocality == .cloud && o.modelLocality == .device)
        #expect(o.folders.first?.toCheck == 5)
        #expect(o.manyAddresses)
        #expect(o.mainAddresses == ["me@example.org"])
        #expect(o.sendingAddresses.map(\.email) == ["me@example.org", "shop@example.org"])
        let rows = MailStore.scopeRows(o)
        #expect(rows.map(\.label) == ["me@example.org", "shop@example.org"])
        #expect(rows[1].scope == MailScope(address: "shop@example.org"))
    }

    @Test func draftsGoOutInTheComputersShape() throws {
        let id = UUID(uuidString: account)!
        let draft = MailDraft(connectionId: id, from: "shop@example.org", to: ["sam@example.com"], subject: "Hi", body: "Hello", replyTo: 3)
        let object = try #require(try JSONSerialization.jsonObject(with: JSONEncoder().encode(draft)) as? [String: Any])
        #expect(object["connection_id"] as? String == account)
        #expect(object["reply_to"] as? Int == 3)
        #expect(object["forward_of"] is NSNull)
        #expect(object["cc"] as? [String] == [])
        let back = try JSONDecoder().decode(MailDraft.self, from: JSONEncoder().encode(draft))
        #expect(back == draft)
    }

    @Test func draftActionsBecomeCards() throws {
        let json = #"""
        {"id":"\#(UUID())","tool":"mail_compose","summary":"write an email","arguments":{},"requires_approval":false,
         "status":"done","result":"drafted","output":{"draft":{"connection_id":null,"from":null,"to":["sam@example.com"],
         "cc":[],"subject":"Dinner","body":"Hi Sam","reply_to":null,"forward_of":null},"status":"Draft shown"}}
        """#
        let action = try JSONDecoder().decode(Action.self, from: Data(json.utf8))
        let draft = try #require(MailDrafts.draft(of: action))
        #expect(draft.to == ["sam@example.com"] && draft.subject == "Dinner")
        var other = action
        other.tool = "mail_send"
        #expect(MailDrafts.draft(of: other) == nil)
        other = action
        other.status = .running
        #expect(MailDrafts.draft(of: other) == nil)
    }

    @Test func queriesAreEncoded() {
        let path = MimiAPI.path("/mail/threads", [
            URLQueryItem(name: "q", value: "a+b & c=d"),
            URLQueryItem(name: "address", value: "me+tag@example.org"),
        ])
        #expect(path == "/mail/threads?q=a%2Bb%20%26%20c%3Dd&address=me%2Btag@example.org")
        #expect(MimiAPI.path("/mail", []) == "/mail")
    }
}

struct MailReplyTests {
    let account = UUID()

    func message(_ id: Int64, from: String, to: [String], cc: [String] = [], me: Bool = false, attachments: [String] = []) throws -> MailMessage {
        let json: [String: Any] = [
            "id": id, "from": ["name": NSNull(), "email": from],
            "to": to.map { ["name": NSNull(), "email": $0] }, "cc": cc.map { ["name": NSNull(), "email": $0] },
            "date": 1_790_000_000_000, "body": "Body \(id)", "seen": true, "from_me": me,
            "attachments": attachments, "suspicious": false, "has_html": NSNull(),
        ]
        return try JSONDecoder().decode(MailMessage.self, from: JSONSerialization.data(withJSONObject: json))
    }

    func detail(subject: String, _ messages: [MailMessage]) throws -> MailThreadDetail {
        let thread: [String: Any] = [
            "id": 7, "connection_id": account.uuidString.lowercased(), "received_on": "alias@example.org",
            "suspicious": false, "folders": [], "subject": subject, "participants": [], "last_at": 1,
            "message_count": messages.count, "unread": false, "flagged": false, "category": NSNull(),
            "summary": NSNull(), "snippet": "", "automated": false, "last_from_me": false,
        ]
        let t = try JSONDecoder().decode(MailThread.self, from: JSONSerialization.data(withJSONObject: thread))
        return MailThreadDetail(thread: t, messages: messages)
    }

    @Test func repliesGoToWhoeverWroteLastFromTheAddressItCameTo() throws {
        let d = try detail(subject: "Dinner?", [
            message(1, from: "sam@example.com", to: ["alias@example.org"]),
            message(2, from: "alias@example.org", to: ["sam@example.com"], me: true),
        ])
        let r = MailText.reply(to: d)
        #expect(r.to == ["sam@example.com"])
        #expect(r.subject == "Re: Dinner?")
        #expect(r.from == "alias@example.org")
        #expect(r.connectionId == account && r.replyTo == 7)
        #expect(MailText.reply(to: try detail(subject: "RE: Dinner?", [])).subject == "RE: Dinner?")
        #expect(MailText.reply(to: try detail(subject: "Réf : x", [])).subject == "Réf : x")
    }

    @Test func replyAllCopiesEveryoneButTheUser() throws {
        let d = try detail(subject: "Plan", [
            message(1, from: "sam@example.com", to: ["me@example.org", "bo@example.net"], cc: ["Sam@example.com", "lea@example.com"]),
        ])
        let all = try #require(MailText.replyAll(to: d, mine: ["me@example.org"]))
        #expect(all.to == ["sam@example.com"])
        #expect(all.cc == ["bo@example.net", "lea@example.com"])
        let alone = try detail(subject: "Plan", [message(1, from: "sam@example.com", to: ["me@example.org"])])
        #expect(MailText.replyAll(to: alone, mine: ["me@example.org"]) == nil)
    }

    @Test func forwardsQuoteTheMessageAndTakeItsAttachments() throws {
        let d = try detail(subject: "Sheet", [message(4, from: "priya@work.example", to: ["me@example.org"], attachments: ["a.xlsx"])])
        let f = try #require(MailText.forward(d))
        #expect(f.subject == "Fwd: Sheet")
        #expect(f.to.isEmpty && f.replyTo == nil)
        #expect(f.forwardOf == 4)
        #expect(f.body.contains("---------- Forwarded message ----------\nFrom: priya@work.example"))
        #expect(f.body.hasSuffix("Body 4"))
        #expect(MailText.forward(try detail(subject: "Fw: x", [message(1, from: "a@b.c", to: [])]))?.subject == "Fw: x")
        #expect(MailText.forward(try detail(subject: "x", [message(1, from: "a@b.c", to: [])]))?.forwardOf == nil)
    }
}

struct MailTextTests {
    @Test func addressListsAreSplitAndChecked() {
        #expect(MailText.splitAddresses("sam@example.com, Bo <bo@example.net>;x@y.z\n ") == ["sam@example.com", "Bo <bo@example.net>", "x@y.z"])
        #expect(MailText.bare("Bo <Bo@Example.net>") == "bo@example.net")
        #expect(MailText.looksLikeAddress("Bo <bo@example.net>"))
        #expect(!MailText.looksLikeAddress("sam"))
        #expect(!MailText.looksLikeAddress("sam@localhost"))
        #expect(!MailText.looksLikeAddress("a@b@c.d"))
        #expect(!MailText.looksLikeAddress("sam @example.com"))
    }

    @Test func quotedHistoryIsFolded() {
        let (main, quoted) = MailText.splitQuoted("Sounds great!\n\nOn Fri, 25 Sep 2026, you wrote:\n> Hike?")
        #expect(main == "Sounds great!")
        #expect(quoted == "On Fri, 25 Sep 2026, you wrote:\n> Hike?")
        #expect(MailText.splitQuoted("Oui !\nLe 3 oct. 2026, Sam a écrit :\n> Ça va ?").1?.hasPrefix("Le 3") == true)
        #expect(MailText.splitQuoted("> all quoted\n> still").1 == nil)
        #expect(MailText.splitQuoted("a > b\nc").1 == nil)
        #expect(MailText.splitQuoted("Hi\n> quoted\nmine again").1 == nil)
    }

    @Test func programsAreOnlySaved() {
        for name in ["setup.EXE", "run.sh", "x.command", "Mimi.app", "profile.mobileconfig", "README"] {
            #expect(MailText.isProgram(name), "\(name)")
        }
        for name in ["budget.pdf", "photo.JPG", "notes.txt", "sheet.xlsx", "invite.ics"] {
            #expect(!MailText.isProgram(name), "\(name)")
        }
    }

    @Test func fileNamesStayInTheirFolder() {
        #expect(MailText.safeFileName("../../.bashrc") == "_.._.bashrc")
        #expect(MailText.safeFileName(".hidden") == "hidden")
        #expect(MailText.safeFileName("a/b\\c\u{0}.pdf") == "a_b_c_.pdf")
        #expect(MailText.safeFileName("  ") == "attachment")
        let long = String(repeating: "x", count: 200) + ".pdf"
        #expect(MailText.safeFileName(long).count == 104 && MailText.safeFileName(long).hasSuffix(".pdf"))
    }

    @Test func snippetsLoseTheirDividers() {
        #expect(MailText.tidySnippet("──────── [Green Grocer] # This week ==== Hello… ok") == "[Green Grocer] # This week Hello… ok")
        #expect(MailText.tidySnippet("Hi!! See you...") == "Hi!! See you")
    }

    @Test func labelsAreOneShortLine() {
        #expect(MailText.label("  Q3\n budget  review ") == "Q3 budget review")
        #expect(MailText.label("") == "(no subject)")
        #expect(MailText.label(String(repeating: "a", count: 80)).count == 61)
    }
}

struct MailHTMLTests {
    @Test func theEmailPageAllowsNothingToLoadOrRun() {
        let doc = MailHTML.document("<p>Hi</p>")
        #expect(doc.contains("http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; style-src 'unsafe-inline'\""))
        #expect(doc.contains("<meta name=\"referrer\" content=\"no-referrer\">"))
        #expect(doc.contains("<div class=\"mimi-mail\"><p>Hi</p></div>"))
        // The policy comes before the email, so nothing in it can come first.
        #expect(doc.range(of: "Content-Security-Policy")!.lowerBound < doc.range(of: "<p>Hi</p>")!.lowerBound)
        #expect(!MailHTML.csp.contains("script-src"))
        #expect(!MailHTML.csp.contains("http"))
    }

    @Test func blockRulesAreValidJSON() throws {
        let rules = try #require(try JSONSerialization.jsonObject(with: Data(MailHTML.blockRules.utf8)) as? [[String: Any]])
        #expect(rules.count == 2)
    }

    @Test func linksOpenOnlyWhereTheyShould() {
        #expect(MailHTML.target(of: URL(string: "https://maps.example/station")) == .web(URL(string: "https://maps.example/station")!))
        #expect(MailHTML.target(of: URL(string: "http://example.org")) == .web(URL(string: "http://example.org")!))
        #expect(MailHTML.target(of: URL(string: "mailto:sam@example.com?subject=Hi")) == .mail("sam@example.com"))
        #expect(MailHTML.target(of: URL(string: "mailto:nobody")) == .none)
        #expect(MailHTML.target(of: URL(string: "javascript:alert(1)")) == .none)
        #expect(MailHTML.target(of: URL(string: "file:///etc/passwd")) == .none)
        #expect(MailHTML.target(of: URL(string: "mimi://pair?id=x")) == .none)
        #expect(MailHTML.target(of: URL(string: "tel:+33123")) == .none)
        #expect(MailHTML.target(of: nil) == .none)
    }
}
