import SwiftUI

/// The assistant's replies, as Markdown: paragraphs, headings, lists, quotes and code.
/// No raw HTML is ever rendered (it shows as text), and links open in the browser.
struct MarkdownText: View {
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(Self.blocks(text).enumerated()), id: \.offset) { _, block in
                view(for: block)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .textSelection(.enabled)
    }

    @ViewBuilder
    private func view(for block: Block) -> some View {
        switch block {
        case .paragraph(let s):
            Text(Self.inline(s)).font(.body)
        case .heading(let level, let s):
            Text(Self.inline(s)).font(level == 1 ? .title3.weight(.semibold) : .headline)
        case .list(let items, let ordered):
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(items.enumerated()), id: \.offset) { i, item in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(ordered ? "\(i + 1)." : "•")
                            .foregroundStyle(.secondary)
                            .monospacedDigit()
                        Text(Self.inline(item))
                    }
                    .padding(.leading, 2)
                }
            }
        case .quote(let s):
            Text(Self.inline(s))
                .foregroundStyle(.secondary)
                .padding(.leading, 12)
                .overlay(alignment: .leading) {
                    Capsule().fill(Color.secondary.opacity(0.35)).frame(width: 3)
                }
        case .code(let s):
            ScrollView(.horizontal, showsIndicators: false) {
                Text(s).font(.system(.callout, design: .monospaced))
                    .padding(12)
            }
            .background(Color.subtle, in: .rect(cornerRadius: 12))
        case .rule:
            Divider()
        }
    }

    enum Block: Equatable {
        case paragraph(String)
        case heading(Int, String)
        case list([String], ordered: Bool)
        case quote(String)
        case code(String)
        case rule
    }

    static func inline(_ s: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(
            allowsExtendedAttributes: false,
            interpretedSyntax: .inlineOnlyPreservingWhitespace,
            failurePolicy: .returnPartiallyParsedIfPossible
        )
        var out = (try? AttributedString(markdown: s, options: options)) ?? AttributedString(s)
        // Only web links: anything else (file:, custom schemes) stays plain text.
        for run in out.runs {
            guard let url = run.link else { continue }
            if url.scheme == "https" || url.scheme == "http" || url.scheme == "mailto" {
                out[run.range].underlineStyle = .single
            } else {
                out[run.range].link = nil
            }
        }
        return out
    }

    /// Splits Markdown into blocks. Simple on purpose: what models write, not all of CommonMark.
    static func blocks(_ text: String) -> [Block] {
        var blocks: [Block] = []
        var paragraph: [String] = []
        var list: [String] = []
        var ordered = false
        var quote: [String] = []
        var code: [String]? = nil

        func flush() {
            if !paragraph.isEmpty { blocks.append(.paragraph(paragraph.joined(separator: "\n"))); paragraph = [] }
            if !list.isEmpty { blocks.append(.list(list, ordered: ordered)); list = [] }
            if !quote.isEmpty { blocks.append(.quote(quote.joined(separator: "\n"))); quote = [] }
        }

        for raw in text.components(separatedBy: "\n") {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if var c = code {
                if line.hasPrefix("```") {
                    blocks.append(.code(c.joined(separator: "\n")))
                    code = nil
                } else {
                    c.append(raw)
                    code = c
                }
                continue
            }
            if line.hasPrefix("```") { flush(); code = []; continue }
            if line.isEmpty { flush(); continue }
            if line == "---" || line == "***" || line == "___" { flush(); blocks.append(.rule); continue }
            if let hashes = line.firstIndex(where: { $0 != "#" }), line.hasPrefix("#"),
               line[hashes] == " ", line.distance(from: line.startIndex, to: hashes) <= 6 {
                flush()
                let level = line.distance(from: line.startIndex, to: hashes)
                blocks.append(.heading(level, String(line[hashes...]).trimmingCharacters(in: .whitespaces)))
                continue
            }
            if line.hasPrefix("> ") || line == ">" {
                if !paragraph.isEmpty || !list.isEmpty { flush() }
                quote.append(String(line.dropFirst(line == ">" ? 1 : 2)))
                continue
            }
            if let item = bullet(line) {
                if !paragraph.isEmpty || !quote.isEmpty || (!list.isEmpty && ordered) { flush() }
                ordered = false
                list.append(item)
                continue
            }
            if let item = numbered(line) {
                if !paragraph.isEmpty || !quote.isEmpty || (!list.isEmpty && !ordered) { flush() }
                ordered = true
                list.append(item)
                continue
            }
            if !list.isEmpty, raw.hasPrefix("  ") {
                // A wrapped list item.
                list[list.count - 1] += " " + line
                continue
            }
            if !list.isEmpty || !quote.isEmpty { flush() }
            paragraph.append(line)
        }
        if let c = code { blocks.append(.code(c.joined(separator: "\n"))) }
        flush()
        return blocks
    }

    private static func bullet(_ line: String) -> String? {
        for marker in ["- ", "* ", "• ", "+ "] where line.hasPrefix(marker) {
            return String(line.dropFirst(marker.count))
        }
        return nil
    }

    private static func numbered(_ line: String) -> String? {
        let digits = line.prefix { $0.isNumber }
        guard !digits.isEmpty, digits.count <= 3 else { return nil }
        let rest = line.dropFirst(digits.count)
        guard rest.hasPrefix(". ") || rest.hasPrefix(") ") else { return nil }
        return String(rest.dropFirst(2))
    }
}
