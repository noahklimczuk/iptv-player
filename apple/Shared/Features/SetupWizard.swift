import SwiftUI

/// First run (Windows' `SetupWizard`, same steps and wording): add a provider, check it,
/// choose what to import, import with progress, done. Settings and Live TV open the same
/// wizard to add a second provider (`additional`).
///
/// Recognising a pasted address comes first: a whole `get.php` link splits into address,
/// username and password, rather than making anyone do it by hand.
struct SetupWizard: View {
    @Environment(AppModel.self) private var model
    /// True when this is a second provider added from Settings, not first run.
    var additional = false
    /// A provider was saved, imported or importing: what lets the app open.
    let onFinished: () -> Void
    /// Leave with nothing added. Separate from `onFinished`, as on Windows: skipping
    /// leaves no provider, and the caller has to know or it puts the wizard straight back.
    let onSkip: () -> Void

    enum Step: Int, CaseIterable {
        case source, content, importing, done
        var label: String {
            switch self {
            case .source: "Provider"
            case .content: "Content"
            case .importing: "Import"
            case .done: "Done"
            }
        }
    }

    /// What recognising the address could tell: nothing, a full sign-in, or a panel
    /// whose username and password still have to be typed.
    enum Detected { case none, credentials, panel }

    @State private var step: Step = .source
    /// The box at the top: what was pasted or typed, which is not always the address
    /// saved (a link with a sign-in in it collapses to the address alone).
    @State private var pasted = ""
    @State private var kind = "m3u"
    /// The type picked by hand for the address in the box, which a late answer from
    /// recognising must not switch back (Windows' `chosenKind`).
    @State private var chosenKind: String?
    @State private var name = ""
    @State private var url = ""
    @State private var username = ""
    @State private var password = ""
    @State private var detected: Detected = .none
    /// Set when the box is rewritten here rather than typed in, so that change is not
    /// recognised all over again (which would clear the sign-in it just filled in).
    @State private var ownEdit: String?
    @State private var checking = false
    @State private var validation: ProviderCheck?
    #if os(tvOS)
    // Live TV only by default on Apple TV: a whole film catalogue and its search index
    // on storage the system can purge is docs §5 risk 3. Both are a switch away.
    @State private var content = ImportContent(live: true, vod: false, series: false)
    #else
    @State private var content = ImportContent(live: true, vod: true, series: true)
    #endif
    @State private var report: ImportReport?
    @State private var error: String?
    /// The provider saved by an earlier attempt, so trying the import again updates it
    /// rather than adding a second copy.
    @State private var savedId: Int64?
    /// The import was left running. A reference, so the import still reads it after this
    /// screen has gone, and says a failure through the app-wide alert instead.
    @State private var left = Flag()

    final class Flag { var on = false }

    var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                VStack(alignment: .leading, spacing: 28) {
                    header
                    Group {
                        switch step {
                        case .source: sourceStep
                        case .content: contentStep
                        case .importing: importingStep
                        case .done: doneStep
                        }
                    }
                    .transition(.opacity)
                    if let error {
                        Label(error, systemImage: "exclamationmark.triangle.fill")
                            .font(.callout.weight(.semibold))
                            .padding(16)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(.red.opacity(0.12), in: RoundedRectangle(cornerRadius: 16))
                    }
                }
                .padding(cardPadding)
                .frame(maxWidth: cardWidth, alignment: .leading)
                .background(.background.secondary, in: RoundedRectangle(cornerRadius: 40))
                // Not during the import: a provider has been saved by then, so "carry on
                // with none" can no longer happen. The step offers the background instead.
                Button("Skip for now", action: onSkip)
                    .buttonStyle(.borderless)
                    .foregroundStyle(.secondary)
                    .opacity(step == .importing ? 0 : 1)
                    .disabled(step == .importing)
            }
            .padding()
            .frame(maxWidth: .infinity)
            .animation(.easeOut(duration: 0.2), value: step)
        }
        #if os(iOS)
        .scrollDismissesKeyboard(.interactively)
        #endif
    }

    // MARK: Header

    private var header: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack(alignment: .center, spacing: 16) {
                HStack(spacing: 9) {
                    Circle().fill(.tint).frame(width: 14, height: 14)
                    Text("aurora").font(.title2.weight(.heavy)).kerning(-0.5)
                }
                .accessibilityHidden(true)
                Spacer(minLength: 0)
                // Decorative: each step's content says where you are.
                HStack(spacing: 14) {
                    ForEach(Step.allCases, id: \.self) { s in
                        stepBadge(s)
                    }
                }
                .accessibilityHidden(true)
            }
            VStack(alignment: .leading, spacing: 6) {
                Text(additional ? "Add another provider" : "Connect your provider")
                    .font(.largeTitle.weight(.bold))
                    .accessibilityLabel(additional ? "Add another provider" : "Welcome to Aurora TV")
                Text(additional
                     ? "Its channels and titles join the library you already have."
                     : "Aurora is a player. Bring your own subscription and it will do the rest.")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func stepBadge(_ s: Step) -> some View {
        let reached = s.rawValue <= step.rawValue
        return HStack(spacing: 8) {
            Text("\(s.rawValue + 1)")
                .font(.caption.weight(.heavy))
                .frame(width: 26, height: 26)
                // The background colour on the tint: white on blue on a phone, dark on the
                // Apple TV's white tint, where white on it vanished.
                .foregroundStyle(reached ? AnyShapeStyle(.background) : AnyShapeStyle(.secondary))
                .background(reached ? AnyShapeStyle(.tint) : AnyShapeStyle(.clear),
                            in: RoundedRectangle(cornerRadius: 8))
                .overlay {
                    if !reached {
                        RoundedRectangle(cornerRadius: 8).strokeBorder(.tertiary, lineWidth: 1.5)
                    }
                }
            #if os(tvOS)
            Text(s.label).font(.caption).fontWeight(s == step ? .heavy : .semibold)
                .foregroundStyle(s == step ? .primary : .secondary)
            #else
            // Four labels do not fit beside the wordmark on a phone; the numbers do.
            if !compactWidth {
                Text(s.label).font(.subheadline).fontWeight(s == step ? .heavy : .semibold)
                    .foregroundStyle(s == step ? .primary : .secondary)
            }
            #endif
        }
    }

    // MARK: 1. Provider

    private var sourceStep: some View {
        VStack(alignment: .leading, spacing: 18) {
            field("Paste your playlist URL or Xtream address") {
                // Verbatim: as a string key the address would be read as Markdown and
                // drawn as a link, which also took the tap meant for the field.
                TextField(text: $pasted,
                          prompt: Text(verbatim: "http://your-provider.example.com/get.php?username=…&password=…"),
                          axis: .vertical) { Text("Provider address") }
                    .lineLimit(2...4)
                    .font(.system(.callout, design: .monospaced))
                    .textContentType(.URL)
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
                    .textFieldStyle(.roundedBorder)
                    #endif
                    .onChange(of: pasted) { _, text in typed(text) }
                    .task(id: pasted) {
                        // Asked a beat after typing stops (Windows' `DETECT_AFTER_TYPING_MS`):
                        // asked on every keystroke, a link typed by hand split itself at
                        // the first letter of its password.
                        guard ownEdit != pasted else { return }
                        try? await Task.sleep(for: .milliseconds(Int(detectAfterTypingMs())))
                        if !Task.isCancelled { recognise(pasted) }
                    }
            }

            if detected != .none {
                Label(detected == .credentials
                      ? "Recognised an Xtream address — username and password filled in below, and taken out of the address."
                      : "Looks like a panel login. Enter the username and password your provider gave you.",
                      systemImage: detected == .credentials ? "checkmark" : "info.circle")
                    .font(.callout)
                    .foregroundStyle(detected == .credentials ? AnyShapeStyle(.green) : AnyShapeStyle(.secondary))
            }

            // Always offered, never inferred away: most providers give a bare address and
            // a separate sign-in, which leaves nothing in the address to recognise.
            field("Provider type") {
                Picker("Provider type", selection: Binding(get: { kind }, set: { chooseKind($0) })) {
                    Text("Xtream / panel login").tag("xtream")
                    Text("M3U playlist URL").tag("m3u")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
            }

            field("Name this provider") {
                TextField("My subscription", text: $name)
                    #if os(iOS)
                    .textFieldStyle(.roundedBorder)
                    #endif
                    .accessibilityLabel("Provider name")
            }

            if kind == "xtream" {
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .top, spacing: 14) { usernameField; passwordField }
                    VStack(alignment: .leading, spacing: 18) { usernameField; passwordField }
                }
            }

            if let validation {
                CheckResult(result: validation) { suggested in
                    // Offered rather than applied: `http://` puts the sign-in on the wire
                    // in clear text, a trade the viewer makes, not one made for them.
                    if let other = validation.suggestedKind { kind = other }
                    url = suggested
                    rewriteBox(suggested)
                    Task { await check() }
                }
                .padding(16)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background((validation.ok ? Color.green : Color.red).opacity(0.10),
                            in: RoundedRectangle(cornerRadius: 16))
            }

            // A failed check is advice, not a gate: a panel that is overloaded or down for
            // ten minutes is not one typed wrongly. The button says which it is doing.
            // Side by side where they fit, stacked full width on a phone.
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 14) {
                    Spacer(minLength: 0)
                    checkButton(wide: false)
                    continueButton(wide: false)
                }
                VStack(spacing: 12) {
                    continueButton(wide: true)
                    checkButton(wide: true)
                }
            }
            .controlSize(.large)
        }
    }

    private func checkButton(wide: Bool) -> some View {
        Button { Task { await check() } } label: {
            Text(checking ? "Checking…" : "Check connection").lineLimit(1)
                .frame(maxWidth: wide ? .infinity : nil)
        }
        .buttonStyle(.bordered)
        .disabled(checking || url.isEmpty)
    }

    private func continueButton(wide: Bool) -> some View {
        Button {
            error = nil
            step = .content
        } label: {
            Text(validation.map { !$0.ok } == true ? "Add it anyway" : "Continue")
                .lineLimit(1).frame(maxWidth: wide ? .infinity : nil)
        }
        .buttonStyle(.borderedProminent)
        .disabled(checking || url.isEmpty || !canContinue)
    }

    private var usernameField: some View {
        field("Username") {
            TextField("Username", text: $username)
                .textContentType(.username)
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                .textFieldStyle(.roundedBorder)
                #endif
        }
    }

    private var passwordField: some View {
        field("Password") {
            SecureField("Password", text: $password)
                .textContentType(.password)
                #if os(iOS)
                .textFieldStyle(.roundedBorder)
                #endif
        }
    }

    /// Enough of a provider to be worth saving, without asking the network anything
    /// (Windows' `canContinue`): an address that is a URL, and for a panel both halves
    /// of the sign-in.
    private var canContinue: Bool {
        let address = url.trimmingCharacters(in: .whitespaces).lowercased()
        guard address.hasPrefix("http://") || address.hasPrefix("https://") else { return false }
        if kind != "xtream" { return true }
        return !username.trimmingCharacters(in: .whitespaces).isEmpty
            && !password.trimmingCharacters(in: .whitespaces).isEmpty
    }

    private func chooseKind(_ value: String) {
        chosenKind = value
        kind = value
    }

    /// The box changed: whatever was checked or chosen was about the old address.
    private func typed(_ text: String) {
        if ownEdit == text { return }
        ownEdit = nil
        validation = nil
        chosenKind = nil
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            detected = .none
            url = ""
        }
    }

    /// Put an address in the box without it counting as typing.
    private func rewriteBox(_ text: String) {
        ownEdit = text
        pasted = text
    }

    /// Fill the form from what is in the box (Windows' `runDetect`).
    private func recognise(_ text: String) {
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        let found = detectSource(text: text)
        // Two different things: a link carrying a sign-in fills the fields in, a bare
        // panel address only asks for them.
        detected = found.kind != "xtream" ? .none : (found.username != nil ? .credentials : .panel)
        let chosen = chosenKind
        kind = chosen ?? found.kind
        url = found.url
        // A sign-in typed after picking a panel by hand stays, unless the address itself
        // carried one.
        username = found.username ?? (chosen != nil ? username : "")
        password = found.password ?? (chosen != nil ? password : "")
        if name.trimmingCharacters(in: .whitespaces).isEmpty {
            name = host(of: found.url)
        }
        // The password was in the box in clear text; once it is in the masked field, the
        // box keeps the address alone.
        if found.username != nil, found.password != nil, found.url != text {
            rewriteBox(found.url)
        }
    }

    private var draft: ProviderDraft {
        let named = name.trimmingCharacters(in: .whitespaces)
        return ProviderDraft(
            name: named.isEmpty ? host(of: url) : named,
            kind: kind,
            url: url.trimmingCharacters(in: .whitespaces),
            username: kind == "xtream" ? username : nil,
            password: kind == "xtream" ? password : nil
        )
    }

    private func check() async {
        checking = true
        error = nil
        validation = await model.checkProvider(draft)
        checking = false
    }

    // MARK: 2. Content

    private var contentStep: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("What should Aurora import?").font(.title3.weight(.bold))
                .padding(.bottom, 6)
            contentRow("Live TV", "Channels and the programme guide", isOn: $content.live)
            contentRow("Films", "On-demand films", isOn: $content.vod)
            contentRow("Series", "Shows, seasons and episodes", isOn: $content.series)
            HStack {
                Button("Back", systemImage: "chevron.left") { step = .source }
                    .buttonStyle(.bordered)
                Spacer()
                Button("Import library") { Task { await runImport() } }
                    .buttonStyle(.borderedProminent)
                    .disabled(!content.live && !content.vod && !content.series)
            }
            .controlSize(.large)
            .padding(.top, 12)
        }
    }

    private func contentRow(_ title: String, _ hint: String, isOn: Binding<Bool>) -> some View {
        Toggle(isOn: isOn) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).fontWeight(.heavy)
                Text(hint).font(.subheadline).foregroundStyle(.secondary)
            }
        }
        #if os(iOS)
        .padding(.horizontal, 16).padding(.vertical, 12)
        .background(isOn.wrappedValue ? AnyShapeStyle(.tint.opacity(0.12)) : AnyShapeStyle(.background),
                    in: RoundedRectangle(cornerRadius: 16))
        #endif
    }

    private func runImport() async {
        step = .importing
        error = nil
        do {
            let id: Int64
            if let savedId {
                try await model.updateProvider(savedId, draft: draft)
                id = savedId
            } else {
                id = try await model.saveProvider(draft)
                savedId = id
            }
            report = try await model.importProvider(id, content: content)
            step = .done
        } catch {
            if left.on {
                model.error = "The import did not finish. \(error.viewerMessage)"
            } else {
                self.error = error.viewerMessage
                step = .content
            }
        }
    }

    // MARK: 3. Import

    /// The import, with a way out of it: it carries on in the background, and Settings
    /// and Live TV show the same progress.
    private var importingStep: some View {
        let progress = model.importProgress
        return VStack(spacing: 16) {
            ProgressView().controlSize(.large)
            Text((progress?.phase ?? .fetchingPlaylist).sentence)
                .font(.title3.weight(.semibold))
            if let progress, progress.total > 0 {
                VStack(spacing: 6) {
                    ProgressView(value: Double(progress.done), total: Double(progress.total))
                    Text("\(progress.done.formatted()) of \(progress.total.formatted())")
                        .font(.caption).foregroundStyle(.tertiary)
                }
            }
            Text("A large subscription can take several minutes. You can leave this running and start watching — the progress carries on in Settings.")
                .font(.callout).foregroundStyle(.tertiary)
                .multilineTextAlignment(.center)
            Button("Continue in the background", systemImage: "chevron.right") {
                left.on = true
                onFinished()
            }
            .buttonStyle(.bordered)
        }
        .frame(maxWidth: .infinity)
    }

    // MARK: 4. Done

    @ViewBuilder private var doneStep: some View {
        if let report {
            DoneSummary(report: report, onFinished: onFinished)
        } else {
            VStack(spacing: 16) {
                Text("Import finished").font(.title3.weight(.bold))
                Button("Start watching", action: onFinished).buttonStyle(.borderedProminent)
            }
            .frame(maxWidth: .infinity)
        }
    }

    // MARK: Layout

    private func field<Content: View>(_ label: String, @ViewBuilder _ content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(.subheadline.weight(.heavy)).foregroundStyle(.secondary)
            content()
        }
    }

    #if os(tvOS)
    private let cardPadding: CGFloat = 56
    private let cardWidth: CGFloat = 1200
    #else
    private let cardPadding: CGFloat = 28
    private let cardWidth: CGFloat = 800
    @Environment(\.horizontalSizeClass) private var sizeClass
    private var compactWidth: Bool { sizeClass == .compact }
    #endif
}

private func host(of url: String) -> String {
    URL(string: url.trimmingCharacters(in: .whitespaces))?.host() ?? ""
}

/// What the import brought in, how much of it the guide covers, and what to look at.
private struct DoneSummary: View {
    let report: ImportReport
    let onFinished: () -> Void

    var body: some View {
        // Coverage over every channel, not over the names that fitted in the report:
        // dividing by a capped list drifted towards 100% (Windows' R-121).
        let unmatched = report.epgUnmatchedTotal
        let known = report.epgMatched + unmatched
        let coverage = known > 0 ? Int((Double(report.epgMatched) / Double(known) * 100).rounded()) : 0
        VStack(alignment: .leading, spacing: 20) {
            VStack(spacing: 8) {
                Image(systemName: "checkmark").font(.largeTitle.weight(.bold)).foregroundStyle(.green)
                Text("Your library is ready").font(.title3.weight(.bold))
            }
            .frame(maxWidth: .infinity)

            Grid(horizontalSpacing: 10) {
                GridRow {
                    stat("Channels", report.channels)
                    stat("Films", report.movies)
                    stat("Series", report.series)
                    stat("Episodes", report.episodes)
                }
            }

            Text(summary(unmatched: unmatched, known: known, coverage: coverage))
                .font(.callout).foregroundStyle(.secondary)

            if !report.warnings.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(report.warnings.count) warning\(report.warnings.count == 1 ? "" : "s")")
                        .font(.callout.weight(.semibold)).foregroundStyle(.orange)
                    ForEach(Array(report.warnings.prefix(8).enumerated()), id: \.offset) { _, w in
                        Text("• \(w)").font(.caption).foregroundStyle(.secondary)
                    }
                }
            }

            Button(action: onFinished) {
                Label("Start watching", systemImage: "play.fill").frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
        }
    }

    private func summary(unmatched: UInt64, known: UInt64, coverage: Int) -> String {
        var text = "Guide data matched \(report.epgMatched.formatted()) of \(known.formatted()) channels (\(coverage)%)."
        if unmatched > 0 {
            // Windows adds "can be mapped by hand in Settings"; there is no such screen
            // here yet, so it says only what is true.
            text += " The other \(unmatched.formatted()) have no guide yet."
        }
        if report.channelsMissing > 0 {
            text += " \(report.channelsMissing.formatted()) channels your provider no longer lists were kept, marked Dropped in Live TV."
        }
        return text
    }

    private func stat(_ label: String, _ value: UInt64) -> some View {
        VStack(spacing: 2) {
            Text(value.formatted()).font(.title2.weight(.heavy)).monospacedDigit()
                .lineLimit(1).minimumScaleFactor(0.6)
            Text(label).font(.caption).foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 12)
        .background(.background, in: RoundedRectangle(cornerRadius: 12))
    }
}
