import WebKit
import AppKit

final class NativeWebViews {
    weak var owner: RusterizeView?
    private var views: [UInt32: Entry] = [:]
    private var events: [(UInt32, Data)] = []
    init(_ owner: RusterizeView) { self.owner = owner }
    private final class Entry: NSObject, WKNavigationDelegate {
        let id: UInt32
        let web: WKWebView
        let javascript: Bool
        weak var manager: NativeWebViews?
        init(_ id: UInt32, _ frame: CGRect, _ javascript: Bool, _ manager: NativeWebViews) {
            self.id = id; self.javascript = javascript; self.manager = manager
            let config = WKWebViewConfiguration()
            config.defaultWebpagePreferences.allowsContentJavaScript = javascript
            web = WKWebView(frame: frame, configuration: config)
            super.init(); web.navigationDelegate = self
        }
        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
            let url = action.request.url?.absoluteString ?? ""
            let allowed = url.hasPrefix("https://") || url.hasPrefix("http://") || url == "about:blank"
            if !allowed { manager?.emit(self, 3, 0, "Only HTTP(S) and about:blank navigation is allowed") }
            decisionHandler(allowed ? .allow : .cancel)
        }
        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { manager?.emit(self, 2, 0, web.url?.absoluteString ?? "about:blank") }
        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { manager?.emit(self, 3, 0, error.localizedDescription) }
        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { manager?.emit(self, 3, 0, error.localizedDescription) }
        deinit { web.stopLoading(); web.navigationDelegate = nil }
    }
    private func emit(_ entry: Entry, _ kind: UInt32, _ token: UInt32, _ text: String) {
        guard views[entry.id] === entry else { return }
        var w = Writer(); w.u32(kind); w.u32(entry.id); w.u32(token); w.string(text)
        events.append((entry.id, w.data))
        DispatchQueue.main.async { [weak self] in self?.owner?.pollNative() }
    }
    func command(_ op: UInt32, _ r: inout Reader) throws -> Data {
        if op == 21 {
            if events.isEmpty { var w = Writer(); w.u32(0); return w.data }
            return events.removeFirst().1
        }
        let id = try r.u32(), action = try r.u32()
        if action == 1 {
            guard views[id] == nil, let owner = owner else { throw WireError.native("WebView ID already exists or owner is closed") }
            let frame = try r.rect(), visible = try r.u32() != 0, javascript = try r.u32() != 0
            let entry = Entry(id, frame, javascript, self)
            views[id] = entry; entry.web.isHidden = !visible; owner.addSubview(entry.web)
            emit(entry, 1, 0, ""); return Data()
        }
        guard let entry = views[id] else { throw WireError.native("Unknown WebView ID") }
        switch action {
        case 2:
            views.removeValue(forKey: id); events.removeAll { $0.0 == id }; entry.web.stopLoading(); entry.web.removeFromSuperview()
        case 3: entry.web.frame = try r.rect()
        case 4: entry.web.isHidden = try r.u32() == 0
        case 5:
            guard let url = URL(string: try r.string()) else { throw WireError.malformed }
            entry.web.load(URLRequest(url: url))
        case 6: entry.web.loadHTMLString(try r.string(), baseURL: URL(string: "about:blank"))
        case 7:
            guard entry.javascript else { throw WireError.native("JavaScript is disabled") }
            let token = try r.u32(), script = try r.string()
            entry.web.evaluateJavaScript(script) { [weak self, weak entry] value, error in
                guard let self = self, let entry = entry else { return }
                if let error = error { self.emit(entry, 5, token, error.localizedDescription); return }
                do {
                    let json = try JSONSerialization.data(withJSONObject: value ?? NSNull(), options: [.fragmentsAllowed, .sortedKeys])
                    self.emit(entry, 4, token, String(decoding: json, as: UTF8.self))
                } catch { self.emit(entry, 5, token, error.localizedDescription) }
            }
        case 8: owner?.window?.makeFirstResponder(entry.web)
        case 9: entry.web.reload()
        case 10: entry.web.goBack()
        case 11: entry.web.goForward()
        default: throw WireError.native("Unknown WebView command")
        }
        return Data()
    }
    func close() {
        for entry in views.values { entry.web.stopLoading(); entry.web.navigationDelegate = nil; entry.web.removeFromSuperview() }
        views.removeAll(); events.removeAll()
    }
}
