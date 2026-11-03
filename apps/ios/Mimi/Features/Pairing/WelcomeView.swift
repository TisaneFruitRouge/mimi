import SwiftUI
import VisionKit

/// First run, or after this phone was removed: what Mimi on a phone is, and pairing it
/// with the computer by scanning the code from Settings › Phone.
struct WelcomeView: View {
    var reason: String?
    @Environment(AppModel.self) private var model
    @State private var scanning = false
    @State private var pasting = false
    @State private var pasted = ""
    @State private var link: PairingLink?

    var body: some View {
        ScrollView {
            VStack(spacing: 28) {
                AssistantAvatar(size: 120, mood: reason == nil ? .happy : .sleepy)
                    .padding(.top, 48)
                VStack(spacing: 10) {
                    Text("Your assistant, on your phone")
                        .font(.largeTitle.weight(.bold))
                        .multilineTextAlignment(.center)
                    Text("Mimi lives on your computer. Pair this phone with it to talk to it from anywhere. Your conversations stay on your computer.")
                        .font(.body)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }
                if let reason {
                    Label(reason, systemImage: "info.circle")
                        .font(.callout)
                        .padding(14)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color.subtle, in: .rect(cornerRadius: 14))
                }
                VStack(alignment: .leading, spacing: 14) {
                    Step(number: 1, text: "On your computer, open Mimi, then Settings › Phone.")
                    Step(number: 2, text: "Choose Pair a phone.")
                    Step(number: 3, text: "Scan the code that appears.")
                }
                .padding(18)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.background, in: .rect(cornerRadius: 18))
            }
            .padding(.horizontal, 24)
            .padding(.bottom, 24)
        }
        .background(Color.canvas)
        .safeAreaInset(edge: .bottom) {
            VStack(spacing: 10) {
                Button {
                    scanning = true
                } label: {
                    Label("Scan the code", systemImage: "qrcode.viewfinder")
                }
                .buttonStyle(.lime)
                Button("Paste a pairing link instead") { pasting = true }
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 12)
            .background(.bar)
        }
        .fullScreenCover(isPresented: $scanning) {
            ScannerSheet { found in
                scanning = false
                link = found
            }
        }
        .alert("Pairing link", isPresented: $pasting) {
            TextField("mimi://pair?…", text: $pasted)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            Button("Cancel", role: .cancel) { pasted = "" }
            Button("Continue") {
                link = PairingLink(string: pasted)
                pasted = ""
            }
        } message: {
            Text("On your computer, the pairing code can also be copied as a link.")
        }
        .sheet(item: $link) { link in
            PairSheet(link: link)
                .presentationDetents([.medium])
        }
        .onAppear { link = model.incomingLink ?? link; model.incomingLink = nil }
        .onChange(of: model.incomingLink) { _, new in
            if let new { link = new; model.incomingLink = nil }
        }
    }
}

extension PairingLink: Identifiable {
    var id: String { code }
}

private struct Step: View {
    let number: Int
    let text: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text("\(number)")
                .font(.footnote.weight(.bold))
                .frame(width: 24, height: 24)
                .background(Color.limeSoft, in: .circle)
                .foregroundStyle(Color.limeDeep)
            Text(text).font(.callout)
        }
    }
}

/// Names the phone, then pairs.
private struct PairSheet: View {
    let link: PairingLink
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var name = AppModel.defaultPhoneName
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        VStack(spacing: 18) {
            Text("Pair with your computer")
                .font(.title3.weight(.semibold))
                .padding(.top, 24)
            VStack(alignment: .leading, spacing: 6) {
                Text("Name this phone").font(.footnote).foregroundStyle(.secondary)
                TextField("Name", text: $name)
                    .textFieldStyle(.roundedBorder)
                    .submitLabel(.go)
                    .onSubmit(pair)
                Text("It's how this phone shows on your computer.").font(.footnote).foregroundStyle(.secondary)
            }
            if let error {
                Text(error).font(.callout).foregroundStyle(Color.danger).multilineTextAlignment(.center)
            }
            Spacer()
            Button(action: pair) {
                if busy {
                    HStack(spacing: 8) { ProgressView().tint(.limeInk); Text("Connecting…") }
                } else {
                    Text("Pair")
                }
            }
            .buttonStyle(.lime)
            .disabled(busy || name.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        .padding(24)
        .interactiveDismissDisabled(busy)
    }

    private func pair() {
        busy = true
        error = nil
        Task {
            do {
                try await model.pair(with: link, name: name.trimmingCharacters(in: .whitespaces))
                UINotificationFeedbackGenerator().notificationOccurred(.success)
                dismiss()
            } catch {
                self.error = error.localizedDescription
                busy = false
            }
        }
    }
}

/// The camera, looking for a Mimi pairing code.
private struct ScannerSheet: View {
    let onFound: (PairingLink) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ZStack(alignment: .top) {
            if DataScannerViewController.isSupported && DataScannerViewController.isAvailable {
                QRScanner(onFound: onFound).ignoresSafeArea()
            } else {
                ContentUnavailableView(
                    "The camera isn't available",
                    systemImage: "camera",
                    description: Text("Paste the pairing link instead, or allow Mimi to use the camera in Settings.")
                )
            }
            HStack {
                Spacer()
                Button("Close", systemImage: "xmark") { dismiss() }
                    .labelStyle(.iconOnly)
                    .font(.title3)
                    .padding(12)
                    .glassEffect(.regular.interactive(), in: .circle)
            }
            .padding()
            Text("Point at the code on your computer")
                .font(.callout.weight(.medium))
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .glassEffect(.regular, in: .capsule)
                .padding(.top, 80)
        }
    }
}

private struct QRScanner: UIViewControllerRepresentable {
    let onFound: (PairingLink) -> Void

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            isHighlightingEnabled: true
        )
        scanner.delegate = context.coordinator
        try? scanner.startScanning()
        return scanner
    }

    func updateUIViewController(_ controller: DataScannerViewController, context: Context) {}

    func makeCoordinator() -> Coordinator { Coordinator(onFound: onFound) }

    final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        let onFound: (PairingLink) -> Void
        private var done = false

        init(onFound: @escaping (PairingLink) -> Void) { self.onFound = onFound }

        func dataScanner(_ scanner: DataScannerViewController, didAdd items: [RecognizedItem], allItems: [RecognizedItem]) {
            for item in items {
                guard !done, case .barcode(let code) = item, let value = code.payloadStringValue,
                      let link = PairingLink(string: value) else { continue }
                done = true
                scanner.stopScanning()
                UINotificationFeedbackGenerator().notificationOccurred(.success)
                onFound(link)
            }
        }
    }
}
