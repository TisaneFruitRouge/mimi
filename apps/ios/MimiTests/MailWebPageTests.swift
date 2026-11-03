import Foundation
import Testing
import WebKit
@testable import Mimi

/// The email page for real, in WebKit: it loads, is measured, and runs nothing from the
/// email.
@MainActor
@Suite(.serialized)
struct MailWebPageTests {
    func load(_ html: String, width: CGFloat = 320) async throws -> (CGFloat, MailWebPage) {
        let page = MailWebPage()
        page.webView.frame = CGRect(x: 0, y: 0, width: width, height: 1)
        let height = try await withCheckedThrowingContinuation { (c: CheckedContinuation<CGFloat, Error>) in
            var done = false
            page.onHeight = { h in
                guard !done else { return }
                done = true
                c.resume(returning: h)
            }
            page.load(html)
            Task {
                try? await Task.sleep(for: .seconds(60))
                guard !done else { return }
                done = true
                c.resume(throwing: CancellationError())
            }
        }
        return (height, page)
    }

    @Test func anEmailLoadsAndIsMeasured() async throws {
        let (h, _) = try await load("<p>Hello</p><p>Second line</p><p>Third</p>")
        #expect(h > 40 && h < 400)
    }

    @Test func wideLayoutsAreScaledToFit() async throws {
        let (h, page) = try await load("<div style=\"width:960px\"><p>Wide</p></div>")
        let transform = try await page.webView.evaluateJavaScript(
            "document.querySelector('.mimi-mail').style.transform", in: nil, contentWorld: .defaultClient)
        #expect((transform as? String)?.hasPrefix("scale(0.3") == true)
        #expect(h > 0)
    }

    /// A newsletter laid out wider than the phone is scaled down and drawn, not left blank.
    @Test func wideNewslettersAreDrawn() async throws {
        let html = """
        <table width="100%" bgcolor="#f4f1ea"><tbody><tr><td align="center" style="padding:24px">\
        <table width="560" style="background:#ffffff"><tbody><tr><td style="padding:24px 28px">\
        <h1 style="color:#2f4a2a;font-size:26px">This week's baskets</h1><p style="color:#444">Hello! Autumn is here.</p>\
        <img height="220" alt="Baskets" width="504"></td></tr></tbody></table></td></tr></tbody></table>
        """
        let (h, page) = try await load(html, width: 358)
        #expect(h > 100 && h < 600)
        let window = try #require(UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows).first)
        page.webView.frame = CGRect(x: 0, y: 0, width: 358, height: h)
        window.addSubview(page.webView)
        defer { page.webView.removeFromSuperview() }
        try await Task.sleep(for: .seconds(2))
        let image = try await page.webView.takeSnapshot(configuration: nil)
        #expect(Self.inkedFraction(image) > 0.05)
    }

    /// How much of an image isn't white or transparent.
    static func inkedFraction(_ image: UIImage) -> Double {
        guard let cg = image.cgImage else { return 0 }
        let w = cg.width, h = cg.height
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        let ctx = CGContext(data: &pixels, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        ctx?.draw(cg, in: CGRect(x: 0, y: 0, width: w, height: h))
        var inked = 0
        for i in stride(from: 0, to: pixels.count, by: 4) where pixels[i + 3] > 20 && (pixels[i] < 235 || pixels[i + 1] < 235 || pixels[i + 2] < 235) {
            inked += 1
        }
        return Double(inked) / Double(w * h)
    }

    @Test func scriptsInTheEmailDontRun() async throws {
        let (_, page) = try await load(
            "<p id=x>Before</p><script>document.getElementById('x').textContent='Ran'</script><img src=x onerror=\"document.title='ran'\">")
        let text = try await page.webView.evaluateJavaScript(
            "document.getElementById('x').textContent + '|' + document.title", in: nil, contentWorld: .defaultClient)
        #expect(text as? String == "Before|")
    }
}
