import SwiftUI

/// Four digits that submit by themselves, a wrong PIN saying how many tries are left, and
/// the lockout counting down: Windows' shared `PinPad.tsx`, for a profile's PIN and for the
/// parental PIN alike. The attempt count and the lockout are the shared database's, so they
/// are the same tries Windows counts.
struct PinPad: View {
    @Environment(\.dismiss) private var dismiss
    let heading: String
    /// Check the PIN; nil when it could not be asked.
    let verify: (String) async -> PinOutcome?
    let onUnlocked: () -> Void
    var onCancel: (() -> Void)?

    @State private var digits = ""
    @State private var message: String?
    @State private var lockedUntil: Date?
    @State private var checking = false

    var body: some View {
        VStack(spacing: 24) {
            Text(heading).displayFont(.title2).multilineTextAlignment(.center)
            HStack(spacing: 16) {
                ForEach(0..<4, id: \.self) { i in
                    Circle()
                        .fill(i < digits.count ? Color.primary : Color.secondary.opacity(0.3))
                        .frame(width: 18, height: 18)
                }
            }
            .accessibilityLabel("\(digits.count) of 4 digits entered")
            Ticking(every: 1) { context in
                if let until = lockedUntil, until > context.date {
                    Text("Too many attempts. Try again in \(Int(until.timeIntervalSince(context.date).rounded(.up)))s")
                        .foregroundStyle(.orange)
                } else if let message {
                    Text(message).foregroundStyle(.orange)
                } else {
                    Text(" ")
                }
            }
            LazyVGrid(columns: Array(repeating: GridItem(.fixed(keySize), spacing: 16), count: 3), spacing: 16) {
                ForEach(["1", "2", "3", "4", "5", "6", "7", "8", "9"], id: \.self) { key($0) }
                Button("Cancel") { cancel() }.font(.ui(.caption))
                key("0")
                Button { if !digits.isEmpty { digits.removeLast() } } label: {
                    Image(systemName: "delete.left")
                }
                .accessibilityLabel("Delete")
            }
            .disabled(checking || isLocked)
        }
        .padding(40)
    }

    #if os(tvOS)
    private let keySize: CGFloat = 120
    #else
    private let keySize: CGFloat = 72
    #endif

    private var isLocked: Bool { (lockedUntil ?? .distantPast) > Date() }

    private func cancel() {
        if let onCancel { onCancel() } else { dismiss() }
    }

    private func key(_ digit: String) -> some View {
        Button {
            push(digit)
        } label: {
            Text(digit).font(.ui(.title)).frame(width: keySize, height: keySize * 0.7)
        }
    }

    private func push(_ digit: String) {
        guard !isLocked, digits.count < 4 else { return }
        digits += digit
        message = nil
        if digits.count == 4 {
            let pin = digits
            Task { await submit(pin) }
        }
    }

    private func submit(_ pin: String) async {
        checking = true
        defer { checking = false; digits = "" }
        guard let outcome = await verify(pin) else {
            message = "Could not check that PIN. Try again."
            return
        }
        switch outcome {
        case .ok, .notRequired:
            onUnlocked()
        case .wrong(let remaining):
            message = "Incorrect PIN — \(remaining) attempt\(remaining == 1 ? "" : "s") left"
        case .lockedOut(let until):
            lockedUntil = Date(timeIntervalSince1970: TimeInterval(until))
            message = nil
        }
    }
}

/// Where the parental PIN dialog can appear (Windows' `ParentalPinDialog`, mounted once).
///
/// Any core call refused for want of the parental PIN asks `AppModel.askParentalPin`, and
/// the newest host on screen shows the pad: the root, and each sheet or full-screen cover
/// that can be refused, since a sheet can only be presented from the frontmost view.
private struct ParentalPinHost: ViewModifier {
    @Environment(AppModel.self) private var model
    @State private var id = UUID()

    func body(content: Content) -> some View {
        content
            .onAppear { model.addPinHost(id) }
            .onDisappear { model.removePinHost(id) }
            .sheet(isPresented: shown) {
                if let request = model.pinRequest {
                    PinPad(heading: Self.heading(request.refusal)) { pin in
                        await model.parentalUnlock(pin)
                    } onUnlocked: {
                        model.answerPin(true)
                    } onCancel: {
                        model.answerPin(false)
                    }
                    .interactiveDismissDisabled()
                    .auroraAppearance()
                }
            }
    }

    private var shown: Binding<Bool> {
        Binding(
            get: { model.pinRequest != nil && model.pinHosts.last == id },
            set: { if !$0 { model.answerPin(false) } }
        )
    }

    /// Windows' headings, word for word.
    static func heading(_ refusal: Refusal) -> String {
        switch refusal {
        case .locked: "Enter the parental PIN to watch this"
        case .timeUp: "Today's viewing time is used up. Enter the parental PIN to keep watching"
        case .pinRequired, .notAllowed: "Enter the parental PIN"
        }
    }
}

extension View {
    /// Let the parental PIN dialog appear over this view (see `ParentalPinHost`).
    func parentalPinHost() -> some View { modifier(ParentalPinHost()) }
}
