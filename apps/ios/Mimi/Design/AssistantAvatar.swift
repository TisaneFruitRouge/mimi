import SwiftUI

/// The assistant as a small, soft character: a lime mochi with a sprout on top. The same
/// drawing as the desktop's `components/assistant-avatar.tsx` and the app icon (a
/// 100×100 grid). Used sparingly: the welcome, an empty chat, "Thinking", and the offline
/// screen. Never named or lettered.
struct AssistantAvatar: View {
    enum Mood { case idle, thinking, happy, sleepy }

    var size: CGFloat = 96
    var mood: Mood = .idle

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var blink = false

    var body: some View {
        TimelineView(.animation(minimumInterval: 1 / 30, paused: reduceMotion)) { context in
            let t = context.date.timeIntervalSinceReferenceDate
            let breath = reduceMotion ? 0 : sin(t * 2 * .pi / breathPeriod)
            Canvas { ctx, canvasSize in
                let s = canvasSize.width / 100
                ctx.scaleBy(x: s, y: s)
                draw(in: &ctx, breath: breath, sway: reduceMotion ? 0 : sin(t * 1.3) * 3)
            }
        }
        .frame(width: size, height: size)
        .task(id: mood) { await blinkLoop() }
        .accessibilityHidden(true)
    }

    private var breathPeriod: Double {
        switch mood {
        case .idle: 4.2
        case .thinking: 3.2
        case .happy: 3
        case .sleepy: 6
        }
    }

    private func blinkLoop() async {
        guard mood == .idle || mood == .thinking else { return }
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(Double.random(in: 2.4...6)))
            withAnimation(.easeInOut(duration: 0.08)) { blink = true }
            try? await Task.sleep(for: .milliseconds(110))
            withAnimation(.easeInOut(duration: 0.1)) { blink = false }
        }
    }

    private func draw(in ctx: inout GraphicsContext, breath: Double, sway: Double) {
        // Shadow.
        ctx.fill(Path(ellipseIn: CGRect(x: 22, y: 88, width: 56, height: 6)), with: .color(.black.opacity(0.08)))

        // Breathing: a slight squash and stretch from the bottom.
        let squash = 1 + breath * 0.015
        var body = ctx
        body.translateBy(x: 50, y: 88)
        body.scaleBy(x: 1 / squash, y: squash)
        let tilt: Double = mood == .thinking ? -4 : mood == .sleepy ? 2 : 0
        body.rotate(by: .degrees(tilt))
        body.translateBy(x: -50, y: -88)

        // Sprout.
        var sprout = body
        sprout.translateBy(x: 50, y: 19)
        let lean = (mood == .sleepy ? -24 : mood == .thinking ? 6 : 0) + sway
        sprout.rotate(by: .degrees(lean))
        sprout.translateBy(x: -50, y: -19)
        var stem = Path()
        stem.move(to: CGPoint(x: 50, y: 19.5))
        stem.addCurve(to: CGPoint(x: 47, y: 7.5), control1: CGPoint(x: 50.4, y: 14), control2: CGPoint(x: 49.4, y: 10.5))
        sprout.stroke(stem, with: .color(Color(hex: 0x6F9A1C)), style: StrokeStyle(lineWidth: 2.6, lineCap: .round))
        var leafA = Path()
        leafA.move(to: CGPoint(x: 47.6, y: 9.2))
        leafA.addCurve(to: CGPoint(x: 32, y: 6), control1: CGPoint(x: 43, y: 3), control2: CGPoint(x: 35, y: 3))
        leafA.addCurve(to: CGPoint(x: 47.6, y: 9.2), control1: CGPoint(x: 35.5, y: 11.5), control2: CGPoint(x: 42.5, y: 12))
        sprout.fill(leafA, with: .color(Color(hex: 0x9CCC2E)))
        var leafB = Path()
        leafB.move(to: CGPoint(x: 48.6, y: 10.4))
        leafB.addCurve(to: CGPoint(x: 62.8, y: 4.6), control1: CGPoint(x: 51.6, y: 3.6), control2: CGPoint(x: 59.4, y: 2.2))
        leafB.addCurve(to: CGPoint(x: 48.6, y: 10.4), control1: CGPoint(x: 60.4, y: 10.8), control2: CGPoint(x: 53.6, y: 12.6))
        sprout.fill(leafB, with: .color(Color(hex: 0xB8E24A)))

        // Body.
        var shape = Path()
        shape.move(to: CGPoint(x: 50, y: 18))
        shape.addCurve(to: CGPoint(x: 91, y: 60), control1: CGPoint(x: 76, y: 18), control2: CGPoint(x: 91, y: 38))
        shape.addCurve(to: CGPoint(x: 50, y: 88), control1: CGPoint(x: 91, y: 80), control2: CGPoint(x: 74, y: 88))
        shape.addCurve(to: CGPoint(x: 9, y: 60), control1: CGPoint(x: 26, y: 88), control2: CGPoint(x: 9, y: 80))
        shape.addCurve(to: CGPoint(x: 50, y: 18), control1: CGPoint(x: 9, y: 38), control2: CGPoint(x: 24, y: 18))
        shape.closeSubpath()
        body.fill(shape, with: .linearGradient(
            Gradient(colors: [Color(hex: 0xE0F98F), Color(hex: 0xB4E140)]),
            startPoint: CGPoint(x: 50, y: 18), endPoint: CGPoint(x: 50, y: 88)))

        // Highlight.
        var shine = body
        shine.translateBy(x: 33, y: 30)
        shine.rotate(by: .degrees(-22))
        shine.fill(Path(ellipseIn: CGRect(x: -10.5, y: -5.2, width: 21, height: 10.4)), with: .color(.white.opacity(0.6)))

        // Blush.
        for x in [24.5, 75.5] {
            let rect = CGRect(x: x - 8, y: 59, width: 16, height: 10)
            body.fill(Path(ellipseIn: rect), with: .radialGradient(
                Gradient(stops: [
                    .init(color: Color(hex: 0xFF8AA5, opacity: 0.7), location: 0.35),
                    .init(color: Color(hex: 0xFF8AA5, opacity: 0), location: 1),
                ]),
                center: CGPoint(x: x, y: 64), startRadius: 0, endRadius: 8))
        }

        // Face, looking where the mood looks.
        var face = body
        switch mood {
        case .thinking: face.translateBy(x: 2.2, y: -3)
        case .sleepy: face.translateBy(x: 0, y: 1.6)
        case .happy: face.translateBy(x: 0, y: -1)
        case .idle: break
        }
        let ink = Color.ink
        for x in [37.5, 62.5] {
            var eye = face
            eye.translateBy(x: x, y: 54.75)
            switch mood {
            case .happy:
                var arch = Path()
                arch.move(to: CGPoint(x: -5.6, y: 2.4))
                arch.addCurve(to: CGPoint(x: 5.6, y: 2.4), control1: CGPoint(x: -5.6, y: -8), control2: CGPoint(x: 5.6, y: -8))
                arch.addCurve(to: CGPoint(x: -5.6, y: 2.4), control1: CGPoint(x: 3.4, y: -3.4), control2: CGPoint(x: -3.4, y: -3.4))
                eye.fill(arch, with: .color(ink))
            case .sleepy:
                var lid = Path()
                lid.move(to: CGPoint(x: -5.6, y: 1.4))
                lid.addCurve(to: CGPoint(x: 5.6, y: 1.4), control1: CGPoint(x: -3, y: 3.4), control2: CGPoint(x: 3, y: 3.4))
                lid.addCurve(to: CGPoint(x: -5.6, y: 1.4), control1: CGPoint(x: 5.6, y: 8.8), control2: CGPoint(x: -5.6, y: 8.8))
                eye.fill(lid, with: .color(ink))
            case .idle, .thinking:
                eye.scaleBy(x: 1, y: blink ? 0.12 : 1)
                eye.fill(Path(ellipseIn: CGRect(x: -5, y: -6.75, width: 10, height: 13.5)), with: .color(ink))
                if !blink {
                    eye.fill(Path(ellipseIn: CGRect(x: 0, y: -5.1, width: 3.8, height: 3.8)), with: .color(.white))
                }
            }
        }
        var mouth = Path()
        switch mood {
        case .happy:
            mouth.move(to: CGPoint(x: 45, y: 65.4))
            mouth.addQuadCurve(to: CGPoint(x: 55, y: 65.4), control: CGPoint(x: 50, y: 71.6))
        case .thinking:
            mouth.move(to: CGPoint(x: 47.6, y: 67.3))
            mouth.addQuadCurve(to: CGPoint(x: 52.6, y: 66.2), control: CGPoint(x: 50, y: 67.1))
        case .sleepy:
            mouth.move(to: CGPoint(x: 47.6, y: 66.8))
            mouth.addQuadCurve(to: CGPoint(x: 52.4, y: 66.8), control: CGPoint(x: 50, y: 68.4))
        case .idle:
            mouth.move(to: CGPoint(x: 46.2, y: 66))
            mouth.addQuadCurve(to: CGPoint(x: 53.8, y: 66), control: CGPoint(x: 50, y: 69.4))
        }
        face.stroke(mouth, with: .color(ink), style: StrokeStyle(lineWidth: 2.3, lineCap: .round))
    }
}

#Preview {
    HStack {
        AssistantAvatar(mood: .idle)
        AssistantAvatar(mood: .thinking)
        AssistantAvatar(mood: .happy)
        AssistantAvatar(mood: .sleepy)
    }
}
