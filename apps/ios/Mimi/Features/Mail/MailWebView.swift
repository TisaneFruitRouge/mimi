import SwiftUI
import WebKit

/// The page an email's own HTML is shown in. The computer has already made the HTML safe
/// (no scripts, forms, frames, hidden text or anything loaded from elsewhere); this is the
/// second fence, the same as the desktop's frame: a strict policy that lets nothing load
/// but the pictures the computer put in, and nothing run.
nonisolated enum MailHTML {
    /// Nothing may load but `data:` pictures and the inline styles; no scripts at all.
    static let csp = "default-src 'none'; img-src data:; style-src 'unsafe-inline'"

    /// Dark text on the white card, long lines wrapped, pictures and tables never wider
    /// than the card.
    static let style = """
    html,body{margin:0;padding:0;background:transparent}
    html{-webkit-text-size-adjust:100%}
    body{color:#1d1d1f;font:16px/1.5 -apple-system,system-ui,sans-serif;overflow-wrap:break-word;word-break:break-word}
    .mimi-mail{overflow-x:auto;overflow-y:hidden}
    img{max-width:100%}
    img[src]{height:auto}
    table{max-width:100%}
    pre{white-space:pre-wrap}
    a{color:#0b63ce}
    blockquote{margin:0 0 0 .6em;padding-left:.8em;border-left:2px solid #e3e3e8;color:#6e6e73}
    """

    static func document(_ body: String) -> String {
        """
        <!doctype html><html><head><meta charset="utf-8">\
        <meta http-equiv="Content-Security-Policy" content="\(csp)">\
        <meta name="referrer" content="no-referrer">\
        <meta name="viewport" content="width=device-width,initial-scale=1">\
        <meta name="color-scheme" content="light">\
        <meta name="format-detection" content="telephone=no,date=no,address=no,email=no">\
        <style>\(style)</style></head><body><div class="mimi-mail">\(body)</div></body></html>
        """
    }

    /// What a tap on a link inside an email does: web pages open in the browser, an email
    /// address starts a new message in Mimi, anything else (files, app links, javascript:)
    /// does nothing.
    enum LinkTarget: Equatable {
        case web(URL)
        case mail(String)
        case none
    }

    static func target(of url: URL?) -> LinkTarget {
        guard let url, let scheme = url.scheme?.lowercased() else { return .none }
        switch scheme {
        case "http", "https":
            return url.host?.isEmpty == false ? .web(url) : .none
        case "mailto":
            let raw = url.absoluteString.dropFirst("mailto:".count)
            let address = String(raw.prefix { $0 != "?" }).removingPercentEncoding ?? ""
            return MailText.looksLikeAddress(address) ? .mail(address) : .none
        default:
            return .none
        }
    }

    /// Content rules that block every load but `data:` pictures, in case anything got
    /// past the computer and the policy above.
    static let blockRules = """
    [
      {"trigger": {"url-filter": ".*", "resource-type": ["image", "style-sheet", "script", "font", "raw", "svg-document", "media", "popup", "ping", "fetch", "websocket", "other"]}, "action": {"type": "block"}},
      {"trigger": {"url-filter": "^data:"}, "action": {"type": "ignore-previous-rules"}}
    ]
    """
}

/// Compiles the blocking rules once.
private enum MailBlockRules {
    static var compiled: WKContentRuleList?
    static var pending: [CheckedContinuation<WKContentRuleList?, Never>] = []
    static var compiling = false

    static func get() async -> WKContentRuleList? {
        if let compiled { return compiled }
        return await withCheckedContinuation { continuation in
            pending.append(continuation)
            guard !compiling else { return }
            compiling = true
            WKContentRuleListStore.default().compileContentRuleList(
                forIdentifier: "mimi-mail-block",
                encodedContentRuleList: MailHTML.blockRules
            ) { list, _ in
                Task { @MainActor in
                    compiled = list
                    compiling = false
                    let waiting = pending
                    pending = []
                    for c in waiting { c.resume(returning: list) }
                }
            }
        }
    }
}

/// An email as it was sent, in a web view that runs no JavaScript from the email, loads
/// nothing from the network, keeps no cookies or storage, and opens links in the browser.
/// It's as tall as its content; a layout wider than the card is scaled down to fit.
struct MailWebView: View {
    let html: String
    var onMailTo: (String) -> Void = { _ in }
    @State private var height: CGFloat = 1
    @Environment(\.openURL) private var openURL

    var body: some View {
        MailWebViewRepresentable(html: html, height: $height) { url in
            switch MailHTML.target(of: url) {
            case .web(let url): openURL(url)
            case .mail(let address): onMailTo(address)
            case .none: break
            }
        }
        // Capped, so an email can't stretch the reader without end.
        .frame(height: min(max(height, 24), 30_000))
        .opacity(height > 1 ? 1 : 0)
        .accessibilityLabel("Email")
    }
}

private struct MailWebViewRepresentable: UIViewRepresentable {
    let html: String
    @Binding var height: CGFloat
    let onLink: (URL?) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> MailWKWebView {
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        config.preferences.javaScriptCanOpenWindowsAutomatically = false
        config.dataDetectorTypes = []
        config.allowsInlineMediaPlayback = false
        config.mediaTypesRequiringUserActionForPlayback = .all
        config.suppressesIncrementalRendering = true
        let web = MailWKWebView(frame: CGRect(x: 0, y: 0, width: 320, height: 1), configuration: config)
        web.onWidthChange = { [weak coordinator = context.coordinator, weak web] in
            if let web { coordinator?.widthChanged(web) }
        }
        web.navigationDelegate = context.coordinator
        web.isOpaque = false
        web.backgroundColor = .clear
        web.scrollView.backgroundColor = .clear
        web.scrollView.isScrollEnabled = false
        web.scrollView.bounces = false
        web.scrollView.contentInsetAdjustmentBehavior = .never
        web.allowsLinkPreview = false
        web.allowsBackForwardNavigationGestures = false
        context.coordinator.load(html, in: web)
        return web
    }

    func updateUIView(_ web: MailWKWebView, context: Context) {
        context.coordinator.parent = self
        if context.coordinator.loaded != html {
            context.coordinator.load(html, in: web)
        }
    }

    static func dismantleUIView(_ web: MailWKWebView, coordinator: Coordinator) {
        web.navigationDelegate = nil
        web.stopLoading()
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        var parent: MailWebViewRepresentable
        var loaded: String?
        /// Only the page this view loads itself may be navigated to.
        private var allowNext = false

        init(_ parent: MailWebViewRepresentable) { self.parent = parent }

        func load(_ html: String, in web: WKWebView) {
            loaded = html
            finished = false
            Task { @MainActor in
                if let rules = await MailBlockRules.get() {
                    web.configuration.userContentController.removeAllContentRuleLists()
                    web.configuration.userContentController.add(rules)
                }
                allowNext = true
                web.loadHTMLString(MailHTML.document(html), baseURL: nil)
            }
        }

        private func setHeight(_ h: CGFloat) {
            guard h > 0, abs(h - parent.height) > 0.5 else { return }
            parent.height = h
        }

        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
            if allowNext, action.navigationType == .other, action.request.url?.scheme == "about" || action.request.url == nil {
                allowNext = false
                return .allow
            }
            if action.navigationType == .linkActivated {
                parent.onLink(action.request.url)
            }
            return .cancel
        }

        private var finished = false
        private var measuredWidth: CGFloat = 0

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            finished = true
            measure(webView)
        }

        /// The card's width changed (layout settled, rotation): measure again.
        func widthChanged(_ webView: WKWebView) {
            guard finished, abs(webView.bounds.width - measuredWidth) > 0.5 else { return }
            measure(webView)
        }

        private func measure(_ webView: WKWebView) {
            measuredWidth = webView.bounds.width
            // Measure the email, scaling a layout wider than the card down to fit (as
            // mail apps do). This runs in the app's own script world: the email's own
            // scripts stay off, and the page's policy doesn't apply here.
            let fit = """
            (() => {
              const box = document.querySelector('.mimi-mail');
              if (!box) return document.documentElement.scrollHeight;
              box.style.cssText = '';
              document.body.style.overflow = '';
              const view = document.documentElement.clientWidth;
              const wide = box.scrollWidth;
              if (wide > view + 1) {
                const z = Math.max(view / wide, 0.35);
                document.body.style.overflow = 'hidden';
                box.style.width = wide + 'px';
                box.style.overflow = 'visible';
                box.style.transformOrigin = '0 0';
                box.style.transform = 'scale(' + z + ')';
              }
              return Math.ceil(box.getBoundingClientRect().height);
            })()
            """
            webView.evaluateJavaScript(fit, in: nil, in: .defaultClient) { [weak self] result in
                if case .success(let value) = result, let h = value as? Double {
                    self?.setHeight(CGFloat(h))
                } else {
                    self?.setHeight(ceil(webView.scrollView.contentSize.height))
                }
            }
        }
    }
}

/// Tells when its width changes, so the email is measured at the card's real width.
final class MailWKWebView: WKWebView {
    var onWidthChange: (() -> Void)?
    private var lastWidth: CGFloat = 0

    override func layoutSubviews() {
        super.layoutSubviews()
        if abs(bounds.width - lastWidth) > 0.5 {
            lastWidth = bounds.width
            onWidthChange?()
        }
    }
}
