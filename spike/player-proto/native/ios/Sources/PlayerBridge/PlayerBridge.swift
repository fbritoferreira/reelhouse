import AVFoundation
import AVKit
import UIKit
import WebKit

/// AVPlayer drawn under the Dioxus WKWebView.
///
/// manganis 0.7.10 converts only NSString/nil returns from Swift correctly (numbers come back as 0,
/// void is read as an object pointer), so every method returns `String?`.
@objc(PlayerBridge)
public class PlayerBridge: NSObject {
    private let player = AVPlayer()
    private var host: PlayerHostView?
    private var pip: AVPictureInPictureController?

    public override init() {
        super.init()
        onMain { self.attach() }
    }

    private func attach() {
        try? AVAudioSession.sharedInstance().setCategory(.playback, mode: .moviePlayback)
        try? AVAudioSession.sharedInstance().setActive(true)
        guard let webView = Self.findWebView() else { return }
        webView.isOpaque = false
        webView.backgroundColor = .clear
        webView.scrollView.backgroundColor = .clear
        webView.configuration.allowsPictureInPictureMediaPlayback = true

        let host = PlayerHostView(frame: webView.frame)
        host.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        host.playerLayer.player = player
        host.playerLayer.videoGravity = .resizeAspect
        host.backgroundColor = .black
        webView.superview?.insertSubview(host, belowSubview: webView)
        self.host = host
        if AVPictureInPictureController.isPictureInPictureSupported() {
            let pip = AVPictureInPictureController(playerLayer: host.playerLayer)
            pip?.canStartPictureInPictureAutomaticallyFromInline = true
            self.pip = pip
        }
    }

    private static func findWebView() -> WKWebView? {
        let windows = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap { $0.windows }
        for window in windows {
            if let found = search(window) { return found }
        }
        return nil
    }

    private static func search(_ view: UIView) -> WKWebView? {
        if let web = view as? WKWebView { return web }
        for child in view.subviews {
            if let found = search(child) { return found }
        }
        return nil
    }

    @objc public func load(_ url: String) -> String? {
        onMain {
            if self.host == nil { self.attach() }
            guard let u = URL(string: url) else { return }
            self.player.replaceCurrentItem(with: AVPlayerItem(url: u))
        }
        return nil
    }

    @objc public func play() -> String? { onMain { self.player.play() }; return nil }
    @objc public func pause() -> String? { onMain { self.player.pause() }; return nil }

    @objc public func seek(_ seconds: Double) -> String? {
        onMain { self.player.seek(to: CMTime(seconds: seconds, preferredTimescale: 600)) }
        return nil
    }

    @objc public func position() -> String? {
        onMain { String(self.player.currentTime().seconds) }
    }

    @objc public func duration() -> String? {
        onMain { String(self.player.currentItem?.duration.seconds ?? 0) }
    }

    @objc public func isPlaying() -> String? {
        onMain { self.player.timeControlStatus == .playing ? "1" : nil }
    }

    @objc public func error() -> String? {
        onMain { self.player.currentItem?.error?.localizedDescription }
    }

    @objc public func startPip() -> String? {
        onMain { self.pip?.startPictureInPicture() }
        return nil
    }
}

final class PlayerHostView: UIView {
    override class var layerClass: AnyClass { AVPlayerLayer.self }
    var playerLayer: AVPlayerLayer { layer as! AVPlayerLayer }
}

private func onMain<T>(_ body: () -> T) -> T {
    Thread.isMainThread ? body() : DispatchQueue.main.sync(execute: body)
}
