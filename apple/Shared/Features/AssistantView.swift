import SwiftUI

/// "Ask Aurora": a conversation that can look through the library and hand back titles
/// to play (Windows' `AssistantPage`, same wording). The loop, its bounds and the rule
/// that only looked-up titles become cards are the core's (`assistant.rs`).
struct AssistantView: View {
    @Environment(AppModel.self) private var model
    @State private var status: AssistantStatus?
    /// Settings → Recommendations' switch, which starts off on Apple until asked.
    @State private var aiOn: Bool?
    @State private var messages: [ChatMessage] = []
    @State private var draft = ""
    @State private var thinking = false
    @State private var steps: [String] = []
    @State private var playing: PlayerItem?
    @State private var series: Int64?
    @State private var movie: Movie?
    @Environment(\.auroraTheme) private var theme
    #if os(tvOS)
    /// Where focus goes once the consent button it was on has gone: the first suggestion,
    /// not wherever the system falls to (the tab bar).
    @FocusState private var suggestion: String?
    #endif
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    /// Shown as a sheet (an iPhone), so it needs its own way out.
    var closable = false
    @Environment(\.dismiss) private var dismiss

    static let suggestions = [
        "What should I watch tonight?",
        "What's on right now?",
        "Something under 90 minutes",
        "A series I can start and finish this week",
    ]

    var body: some View {
        NavigationStack {
            content
                .auroraTitle("Ask Aurora")
                .toolbar {
                    if closable {
                        ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                    }
                }
                .task(id: model.profileId) { await load() }
                .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
                .sheet(item: $movie) { MovieDetail(movie: $0).auroraAppearance() }
                .navigationDestination(item: $series) { SeriesDetail(seriesId: $0) }
        }
    }

    @ViewBuilder private var content: some View {
        if aiOn == false {
            // Asking first (`AIConsent.swift`): nothing goes to Google until this is on.
            VStack(alignment: .leading, spacing: 16) {
                PageHeading("Ask Aurora")
                AIConsentText()
                Button("Turn on Suggestions and the assistant") {
                    Task {
                        UserDefaults.standard.set(true, forKey: AIConsentCard.answeredKey)
                        await model.setGeminiEnabled(true)
                        await load()
                        #if os(tvOS)
                        try? await Task.sleep(for: .milliseconds(100))
                        suggestion = Self.suggestions.first
                        #endif
                    }
                }
                .buttonStyle(DaylightButtonStyle(variant: .primary))
                Spacer()
            }
            .padding(.horizontal, side)
            .frame(maxWidth: 760 + side * 2, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        } else if let status, !status.hasKey {
            ScrollView {
                VStack(alignment: .leading, spacing: Daylight.px(20)) {
                    header
                    DaylightEmpty(icon: "sparkles", title: "The assistant needs a key",
                                  message: "Add a Gemini API key in Settings → Recommendations, and this becomes a conversation about your own library.")
                }
                .padding(.horizontal, side)
            }
        } else {
            VStack(alignment: .leading, spacing: 0) {
                thread
                input
            }
        }
    }

    /// Windows' header: the page's name, what it is for, and Clear once there is
    /// something to clear.
    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 16) {
            VStack(alignment: .leading, spacing: 4) {
                PageHeading("Ask Aurora")
                Text("Ask for something to watch. It can look through your library and hand you titles to play.")
                    .font(.ui(size: Daylight.px(15), weight: .medium))
                    .foregroundStyle(theme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
            if !messages.isEmpty {
                Button("Clear") { Task { await clear() } }
                    .buttonStyle(.daylight(.secondary))
                    .disabled(thinking)
            }
        }
    }

    private var thread: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: Daylight.px(20)) {
                    header
                    if messages.isEmpty && !thinking {
                        VStack(alignment: .leading, spacing: Daylight.px(12)) {
                            Text("Try one of these:").foregroundStyle(theme.textMuted)
                            FlowLayout(spacing: Daylight.px(8)) {
                                ForEach(Self.suggestions, id: \.self) { s in
                                    Button(s) { Task { await send(s) } }
                                        .buttonStyle(.daylight(.secondary))
                                        #if os(tvOS)
                                        .focused($suggestion, equals: s)
                                        #endif
                                }
                            }
                        }
                        #if os(tvOS)
                        .focusSection()
                        #endif
                    }
                    ForEach(Array(messages.enumerated()), id: \.offset) { _, message in
                        Bubble(message: message, play: play, open: open)
                    }
                    if thinking {
                        VStack(alignment: .leading, spacing: 4) {
                            HStack(spacing: 8) {
                                AuroraIcon(name: .sparkle, size: Daylight.px(16), strokeWidth: 2)
                                Text("Thinking…").foregroundStyle(theme.textMuted)
                            }
                            ForEach(Array(steps.enumerated()), id: \.offset) { _, step in
                                Text(step).font(.ui(size: Daylight.px(12))).foregroundStyle(theme.textFaint)
                            }
                        }
                        .padding(.vertical, 12).padding(.horizontal, 18)
                        .daylightRaised(theme, radius: 18)
                        .frame(maxWidth: 736, alignment: .leading)
                        #if os(tvOS)
                        .focusable()
                        #endif
                    }
                    Color.clear.frame(height: 1).id("bottom")
                }
                .padding(.horizontal, side)
                .padding(.bottom, 16)
            }
            .onChange(of: messages.count) { withAnimation { proxy.scrollTo("bottom") } }
            .onChange(of: steps.count) { withAnimation { proxy.scrollTo("bottom") } }
        }
    }

    /// Windows' form: one panel holding the field and Ask.
    private var input: some View {
        HStack(spacing: 8) {
            TextField("Ask for something to watch…", text: $draft)
                .font(.ui(size: Daylight.px(15)))
                .textFieldStyle(.plain)
                .padding(.horizontal, 8)
                .onSubmit { Task { await send(draft) } }
                .disabled(thinking)
                .accessibilityLabel("Ask the assistant")
            Button("Ask") { Task { await send(draft) } }
                .buttonStyle(.daylight(.primary))
                .disabled(thinking || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
        .padding(10)
        .daylightRaised(theme, radius: Daylight.px(24))
        .frame(maxWidth: 780)
        .padding(.horizontal, side)
        .padding(.bottom, 16)
    }

    /// Windows' page padding, 36 either side; a phone keeps 20.
    private var side: CGFloat {
        #if os(iOS)
        sizeClass == .compact ? 20 : 36
        #else
        Daylight.px(36)
        #endif
    }

    private func load() async {
        aiOn = await model.geminiStatus()?.enabled
        status = await model.assistantStatus()
        messages = await model.assistantHistory()
        // A question handed over by the rail, asked once and only with a key: without
        // one the refusal would be the first thing in the thread.
        if status?.hasKey == true, let asked = model.assistantQuestion {
            model.assistantQuestion = nil
            await send(asked)
        }
    }

    private func send(_ text: String) async {
        let asked = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !asked.isEmpty, !thinking else { return }
        draft = ""
        steps = []
        thinking = true
        // The question goes up at once: a model taking six seconds should not also make
        // it look as though the typing was lost.
        let at = Int64(Date().timeIntervalSince1970)
        messages.append(ChatMessage(role: "user", text: asked, items: [], at: at))
        switch await model.assistantSend(asked, step: { steps.append($0) }) {
        case .success(let reply):
            messages.append(reply.message)
        case .failure(let error):
            model.error = "The assistant could not answer. \(error.viewerMessage)"
            // The question stays — it is still what they asked — marked as unanswered.
            messages.append(ChatMessage(role: "assistant",
                                        text: "I could not answer that one. Try again, or check the key in Settings.",
                                        items: [], at: Int64(Date().timeIntervalSince1970)))
        }
        thinking = false
        steps = []
    }

    private func clear() async {
        await model.assistantClear()
        messages = []
        status = await model.assistantStatus()
    }

    /// A channel tunes, a film plays, a series opens on its episodes (where Play picks
    /// up the one they are on).
    private func play(_ item: ChatItem) {
        switch item.kind {
        case "live": playing = .channel(id: item.id, name: item.title)
        case "series": series = item.id
        default: playing = .movie(id: item.id, title: item.title, startAt: nil)
        }
    }

    /// The card's (i): the title's own page, as Windows' detail modal.
    private func open(_ item: ChatItem) {
        if item.kind == "series" {
            series = item.id
        } else {
            Task { movie = await model.movieDetail(id: item.id) }
        }
    }
}

/// Daylight: the viewer's turn is an ink bubble on the right; the answer is bold text on
/// the page, with its picks as cards under it.
private struct Bubble: View {
    let message: ChatMessage
    let play: (ChatItem) -> Void
    let open: (ChatItem) -> Void
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        let mine = message.role == "user"
        VStack(alignment: mine ? .trailing : .leading, spacing: Daylight.px(12)) {
            Group {
                if mine {
                    Text(message.text)
                        .font(.ui(size: Daylight.px(15), weight: .bold))
                        .lineSpacing(3)
                        .foregroundStyle(theme.background)
                        .padding(.vertical, 10).padding(.horizontal, 18)
                        .background(theme.text, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                        .frame(maxWidth: 640, alignment: .trailing)
                } else {
                    Text(message.text)
                        .font(.ui(size: Daylight.px(17), weight: .bold))
                        .lineSpacing(5)
                        .foregroundStyle(theme.text)
                        .frame(maxWidth: 832, alignment: .leading)
                }
            }
            .frame(maxWidth: .infinity, alignment: mine ? .trailing : .leading)
            #if os(tvOS)
            .focusable()
            #else
            .textSelection(.enabled)
            #endif
            if !message.items.isEmpty {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: cardMin), spacing: Daylight.px(16), alignment: .top)],
                          alignment: .leading, spacing: Daylight.px(16)) {
                    ForEach(message.items, id: \.self) { item in
                        Card(item: item, play: play, open: open)
                    }
                }
                .frame(maxWidth: Daylight.px(1000), alignment: .leading)
                #if os(tvOS)
                .focusSection()
                #endif
            }
        }
    }

    #if os(tvOS)
    private let cardMin: CGFloat = Daylight.px(260)
    #else
    private let cardMin: CGFloat = 160
    #endif
}

/// Windows' `ItemCard`: a panel with the poster, the title over its foot, (i) for a film
/// or show, the year and why, and Play.
private struct Card: View {
    let item: ChatItem
    let play: (ChatItem) -> Void
    let open: (ChatItem) -> Void
    @Environment(\.auroraTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ZStack(alignment: .bottomLeading) {
                Color.clear
                    .aspectRatio(1.28, contentMode: .fit)
                    .overlay {
                        AsyncImage(url: item.poster.flatMap(URL.init(string:))) { image in
                            image.resizable().scaledToFill()
                        } placeholder: {
                            Rectangle().fill(theme.surfaceSunk)
                        }
                    }
                    .clipped()
                LinearGradient(colors: [Color(red: 22 / 255, green: 22 / 255, blue: 26 / 255).opacity(0.55), .clear],
                               startPoint: .bottom, endPoint: .top)
                    .frame(height: Daylight.px(90))
                Text(item.title)
                    .font(.ui(size: Daylight.px(20), weight: .heavy))
                    .tracking(Daylight.px(20) * -0.01)
                    .foregroundStyle(.white)
                    .lineLimit(2)
                    .padding(.horizontal, 16).padding(.bottom, 12)
            }
            .clipShape(RoundedRectangle(cornerRadius: Daylight.px(18), style: .continuous))
            .overlay(alignment: .topTrailing) {
                if item.kind != "live" {
                    Button { open(item) } label: { Image(systemName: "info.circle") }
                        .buttonStyle(.daylight(.secondary, .sm, iconOnly: true))
                        .accessibilityLabel("About \(item.title)")
                        .padding(10)
                }
            }
            let line = [item.year.map(String.init), item.note].compactMap { $0 }.joined(separator: " · ")
            if !line.isEmpty {
                Text(line)
                    .font(.ui(size: Daylight.px(13), weight: .semibold))
                    .foregroundStyle(theme.textMuted)
                    .lineLimit(3)
                    .padding(.horizontal, 2)
            }
            Spacer(minLength: 0)
            Button { play(item) } label: {
                Label("Play", systemImage: "play.fill").frame(maxWidth: .infinity)
            }
            .buttonStyle(.daylight(.primary))
        }
        .padding(12)
        .daylightRaised(theme, radius: Daylight.px(24))
    }
}
