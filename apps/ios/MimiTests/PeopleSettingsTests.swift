import Foundation
import Testing
@testable import Mimi

struct PeopleModelTests {
    func decode<T: Decodable>(_ type: T.Type, _ json: String) throws -> T {
        try JSONDecoder().decode(T.self, from: Data(json.utf8))
    }

    @Test func peopleGoUnderTheirFirstLetter() {
        let people = ["Zoé", "Émile", "anna", "Ben", "+41 Hotline", "Ana"].map { PersonSummary(id: UUID(), name: $0) }
        let groups = PeopleIndex.groups(people)
        #expect(groups.map(\.letter) == ["A", "B", "E", "Z", "#"])
        #expect(groups[0].people.map(\.name) == ["anna", "Ana"])
        #expect(PeopleIndex.letter(of: "  léa") == "L")
        #expect(PeopleIndex.letter(of: "") == "#")
    }

    @Test func initialsTakeTheFirstAndLastWord() {
        #expect(personInitials("Léa Marie Martin") == "LM")
        #expect(personInitials("mum") == "M")
        #expect(personInitials("  ") == "?")
    }

    @Test func avatarsHaveTheSameColourAsOnTheComputer() {
        // Worked out with the desktop's `hash` in JavaScript.
        #expect(PersonAvatar.toneIndex("01a10253-657c-7158-8c54-5d76d74450ee") == 5)
        #expect(PersonAvatar.toneIndex("00000000-0000-0000-0000-000000000000") == 0)
        #expect(PersonAvatar.toneIndex("sam") == 3)
    }

    @Test func aPersonSaysWhereTheyComeFrom() throws {
        let p = try decode(Person.self, #"""
        {"id":"\#(UUID())","name":"Sam Carter","nickname":null,"manual":true,
         "handles":[{"id":"\#(UUID())","channel":"email","value":"sam@x.org","label":"work","source_id":"c1","source_name":"iCloud"},
                    {"id":"\#(UUID())","channel":"pager","value":"123","label":null,"source_id":null,"source_name":"Added by you"}],
         "sources":[{"source_id":"c1","source_name":"iCloud","record":"r1","name":"Sam C."}],
         "something_new":1}
        """#)
        #expect(p.sourcesLine == "From iCloud and added by you")
        #expect(p.isCombined)
        #expect(p.firstName == "Sam")
        #expect(p.handles[1].channel == .other)
        #expect(p.handles[1].isOwn)
        #expect(handleDetail(p.handles[0]) == "Work · Email · from iCloud")
        #expect(p.summary.channels == [.email, .other])
    }

    @Test func someoneAddedByHandIsNotCombined() throws {
        let p = try decode(Person.self, #"{"id":"\#(UUID())","name":"Mum","nickname":"Mama","handles":[],"sources":[],"manual":true}"#)
        #expect(p.sourcesLine == "Added by you")
        #expect(!p.isCombined)
        #expect(!p.hasImportedCards)
        #expect(p.firstName == "Mama")
    }

    @Test func mergeRequestsUseTheComputersIds() throws {
        let keep = UUID(), other = UUID()
        let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(MergeRequest(keep: keep, others: [other], name: nil))) as! [String: Any]
        #expect(json["keep"] as? String == keep.uuidString.lowercased())
        #expect(json["others"] as? [String] == [other.uuidString.lowercased()])
        #expect(json["name"] is NSNull)
    }

    @Test func comingUpShowsARepeatingEventOnce() throws {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let ms = Int64(now.timeIntervalSince1970 * 1000)
        func event(_ title: String, _ start: Int64, calendar: String = "c") throws -> PersonEvent {
            try decode(PersonEvent.self, #"{"id":"\#(UUID())","calendar_id":"\#(calendar)","calendar":"Work","title":"\#(title)","start":\#(start),"end":\#(start + 3_600_000),"all_day":false}"#)
        }
        let list = PersonView.upcoming([
            try event("Standup", ms + 86_400_000),
            try event("Standup", ms + 3_600_000),
            try event("Lunch", ms - 7_200_000),
            try event("Standup", ms + 7_200_000, calendar: "other"),
        ], now: now)
        #expect(list.map(\.title) == ["Standup", "Standup"])
        #expect(list[0].start == ms + 3_600_000)
    }
}

struct SettingsModelTests {
    @Test func notesAreGroupedByFolderInOrder() throws {
        let overview = try JSONDecoder().decode(MemoryOverview.self, from: Data(#"""
        {"profile":"","profile_limit":1200,"learning":true,
         "notes":[{"path":"work/atlas.md","title":"Atlas","preview":"The user's project","subject":null,"subject_name":null,"source":"learned","updated_at":1},
                  {"path":"people/sam.md","title":"Sam","preview":"","subject":"x","subject_name":"Sam Carter","source":"you","updated_at":1},
                  {"path":"odd/thing.md","title":"Thing","preview":"","subject":null,"subject_name":null,"source":"robot","updated_at":1}],
         "semantic":{"enabled":false,"available":true,"installed":false,"download_bytes":300000000,"provider_id":null,"model":"e","indexed":0,"total":3,"error":null}}
        """#.utf8))
        let groups = MemoryFolders.groups(overview.notes)
        #expect(groups.map(\.label) == ["People", "Work", "Other"])
        #expect(overview.notes[2].source == .assistant)
        #expect(MemoryFolders.forYou("The user's sister. The user is tall; the user runs.") == "Your sister. You are tall; you runs.")
    }

    @Test func permissionKindsDecodeWithTheirExceptions() throws {
        let kinds = try JSONDecoder().decode([PermissionKind].self, from: Data(#"""
        [{"id":"send_mail","title":"Send emails","ask_detail":"Asks.","automatic_detail":"Sends.","note":null,"icon":"send",
          "color":"#0a84ff","default_autonomy":"ask","targets":"person","autonomy":"automatic",
          "rules":[{"target":{"kind":"person","id":"abc"},"autonomy":"ask","label":"Sam","missing":false}]}]
        """#.utf8))
        #expect(kinds[0].detail == "Sends.")
        #expect(kinds[0].rules[0].target == PermissionTarget(kind: "person", id: "abc"))
        #expect(PermissionsSettingsView.symbol("send") == "paperplane.fill")
        #expect(PermissionsSettingsView.symbol("whatever") == "checkmark.shield.fill")

        let body = try JSONSerialization.jsonObject(with: JSONEncoder().encode(KindPermission(
            autonomy: .ask, rules: [.init(target: PermissionTarget(kind: "calendar", id: "c1"), autonomy: .automatic)]
        ))) as! [String: Any]
        let rule = (body["rules"] as! [[String: Any]])[0]
        #expect(body["autonomy"] as? String == "ask")
        #expect((rule["target"] as? [String: String]) == ["kind": "calendar", "id": "c1"])
    }

    @Test func connectionSetupsSayWhichIntegration() throws {
        func json(_ s: ConnectionSetup) throws -> [String: Any] {
            try JSONSerialization.jsonObject(with: JSONEncoder().encode(s)) as! [String: Any]
        }
        let tg = try json(.telegram(botToken: "1:A"))
        #expect(tg["integration"] as? String == "telegram" && tg["bot_token"] as? String == "1:A")
        let cal = try json(.caldav(serverUrl: "https://c", username: "u", password: "p"))
        #expect(cal["integration"] as? String == "caldav" && cal["server_url"] as? String == "https://c")
        let mail = try json(.email(email: "a@b.org", password: "p", preset: nil, servers: MailServerSettings(imapHost: "imap.b.org")))
        #expect(mail["integration"] as? String == "email")
        #expect(mail["preset"] is NSNull)
        #expect((mail["servers"] as? [String: Any])?["imap_host"] as? String == "imap.b.org")
        #expect((mail["servers"] as? [String: Any])?["imap_security"] as? String == "tls")
    }

    @Test func mailAddressesMustBeWhole() {
        #expect(MailAddressCheck.looksComplete("me@fastmail.com"))
        #expect(!MailAddressCheck.looksComplete("me@fastmail"))
        #expect(!MailAddressCheck.looksComplete("me fastmail.com"))
    }

    @Test func modelsGetFriendlyNamesAndPrices() {
        let catalog = [CatalogModel(id: "qwen3:8b", name: "Qwen3 8B", description: "Solid.", downloadBytes: 1)]
        #expect(ModelNames.info("qwen3:8b", sourceName: nil, catalog: catalog).name == "Qwen3 8B")
        let routed = ModelNames.info("deepseek/v4", sourceName: "DeepSeek: DeepSeek V4 Flash", catalog: [])
        #expect(routed.name == "DeepSeek V4 Flash" && routed.maker == "DeepSeek")
        #expect(ModelNames.prettify("llama3.2:3b") == "llama3.2 3B")
        #expect(ModelNames.prettify("library/mistral:latest") == "mistral")
        #expect(ModelNames.costLabel(ModelPrice(input: 0, output: 0)) == "Free")
        #expect(ModelNames.costLabel(ModelPrice(input: 2, output: 10)) == "about 1.5¢ a message")
    }

    @Test func personalityPresetsMatchWhatWasSaved() {
        #expect(PersonalityPresets.matching("")?.id == "default")
        #expect(PersonalityPresets.matching(PersonalityPresets.all[2].text + "\n")?.id == "concise")
        #expect(PersonalityPresets.matching("Talk like a pirate") == nil)
    }

    @Test func timeSinceReadsNaturally() {
        let now = Date(timeIntervalSince1970: 100_000)
        let ms = Int64(now.timeIntervalSince1970 * 1000)
        #expect(timeSince(ms - 10_000, now: now) == "just now")
        #expect(timeSince(ms - 5 * 60_000, now: now) == "5 min ago")
        #expect(timeSince(ms - 3_600_000, now: now) == "an hour ago")
        #expect(timeSince(ms - 30 * 3_600_000, now: now) == "yesterday")
    }

    @Test func queryValuesAreEscaped() {
        #expect("people/léa.md".mimiQueryEscaped == "people%2Fl%C3%A9a.md")
        #expect("a&b=c d".mimiQueryEscaped == "a%26b%3Dc%20d")
    }
}
