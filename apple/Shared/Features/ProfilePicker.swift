import SwiftUI

/// "Who's watching?", and the PIN pad for a profile that has one. Windows'
/// `ProfilePicker.tsx`: four digits submit by themselves, and a wrong PIN says how many
/// tries are left. The attempt count and the lockout are the shared database's
/// (`profiles::verify_profile_pin`), so they are the same tries Windows counts.
struct ProfilePicker: View {
    @Environment(AppModel.self) private var model
    @State private var unlocking: Profile?

    var body: some View {
        VStack(spacing: 40) {
            Text("Who's watching?").font(.largeTitle.bold())
            HStack(spacing: 32) {
                ForEach(model.profiles, id: \.id) { profile in
                    Button {
                        if profile.hasPin { unlocking = profile } else { model.activate(profile) }
                    } label: {
                        ProfileTile(profile: profile).padding()
                    }
                    .tileButtonStyle()
                }
            }
        }
        .padding()
        .sheet(item: $unlocking) { profile in
            PinPad(profile: profile) {
                unlocking = nil
                model.activate(profile)
            }
        }
    }
}

extension Profile: Identifiable {}

private struct ProfileTile: View {
    let profile: Profile

    var body: some View {
        VStack(spacing: 12) {
            Text(profile.name.prefix(1).uppercased())
                .font(.system(size: 56, weight: .heavy, design: .rounded))
                .frame(width: 140, height: 140)
                .background(Self.colour(profile), in: RoundedRectangle(cornerRadius: 40))
                .foregroundStyle(.white)
            Text(profile.name).font(.headline)
            HStack(spacing: 6) {
                if profile.hasPin { Label("PIN", systemImage: "lock.fill") }
                if profile.isKids { Text("KIDS").foregroundStyle(.green) }
            }
            .font(.caption)
        }
    }

    /// A steady colour per profile, so the same person is the same colour every time.
    private static func colour(_ p: Profile) -> Color {
        let hues: [Color] = [.indigo, .teal, .orange, .pink, .purple, .blue, .green]
        return hues[Int(p.id.magnitude % UInt64(hues.count))]
    }
}

private struct PinPad: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let profile: Profile
    let onUnlocked: () -> Void

    @State private var digits = ""
    @State private var message: String?
    @State private var lockedUntil: Date?
    @State private var checking = false

    var body: some View {
        VStack(spacing: 24) {
            Text("Enter \(profile.name)'s PIN").font(.title2.bold())
            HStack(spacing: 16) {
                ForEach(0..<4, id: \.self) { i in
                    Circle()
                        .fill(i < digits.count ? Color.primary : Color.secondary.opacity(0.3))
                        .frame(width: 18, height: 18)
                }
            }
            .accessibilityLabel("\(digits.count) of 4 digits entered")
            TimelineView(.periodic(from: .now, by: 1)) { context in
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
                Button("Cancel") { dismiss() }.font(.caption)
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

    private func key(_ digit: String) -> some View {
        Button {
            push(digit)
        } label: {
            Text(digit).font(.title).frame(width: keySize, height: keySize * 0.7)
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
        guard let outcome = await model.verifyPin(profile, pin: pin) else {
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
