import SwiftUI

/// "Ask Aurora": a conversation that can look through the library and hand back titles
/// to play (Windows' `AssistantPage`, same wording). The loop, its bounds and the rule
/// that only looked-up titles become cards are the core's (`assistant.rs`).
struct AssistantView: View {
    @Environment(AppModel.self) private var model
    @State private var status: AssistantStatus?
    @State private var messages: [ChatMessage] = []
    @State private var draft = ""
    @State private var thinking = false
    @State private var steps: [String] = []
    @State private var playing: PlayerItem?
    @State private var series: Int64?
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
                    if !messages.isEmpty {
                        ToolbarItem(placement: .primaryAction) {
                            Button("Clear") { Task { await clear() } }.disabled(thinking)
                        }
                    }
                    if closable {
                        ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                    }
                }
                .task(id: model.profileId) { await load() }
                .fullScreenCover(item: $playing) { PlayerScreen(item: $0) }
                .navigationDestination(item: $series) { SeriesDetail(seriesId: $0) }
        }
    }

    @ViewBuilder private var content: some View {
        if let status, !status.hasKey {
            ContentUnavailableView("The assistant needs a key", systemImage: "sparkles",
                                   description: Text("Add a Gemini API key in Settings → Recommendations, and this becomes a conversation about your own library."))
        } else {
            VStack(spacing: 0) {
                PageHeading("Ask Aurora")
                thread
                input
            }
        }
    }

    private var thread: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 20) {
                    Text("Ask for something to watch. It can look through your library and hand you titles to play.")
                        .font(.ui(.callout)).foregroundStyle(.secondary)
                    if messages.isEmpty && !thinking {
                        Text("Try one of these:").foregroundStyle(.secondary)
                        ForEach(Self.suggestions, id: \.self) { s in
                            Button(s) { Task { await send(s) } }
                                .buttonStyle(.bordered)
                        }
                    }
                    ForEach(Array(messages.enumerated()), id: \.offset) { _, message in
                        Bubble(message: message, play: play)
                    }
                    if thinking {
                        VStack(alignment: .leading, spacing: 4) {
                            Label("Thinking…", systemImage: "sparkles").foregroundStyle(.secondary)
                            ForEach(Array(steps.enumerated()), id: \.offset) { _, step in
                                Text(step).font(.ui(.caption)).foregroundStyle(.tertiary)
                            }
                        }
                        #if os(tvOS)
                        .focusable()
                        #endif
                    }
                    Color.clear.frame(height: 1).id("bottom")
                }
                .padding()
            }
            .onChange(of: messages.count) { withAnimation { proxy.scrollTo("bottom") } }
            .onChange(of: steps.count) { withAnimation { proxy.scrollTo("bottom") } }
        }
    }

    private var input: some View {
        HStack {
            TextField("Ask for something to watch…", text: $draft)
                #if os(iOS)
                .textFieldStyle(.roundedBorder)
                #endif
                .onSubmit { Task { await send(draft) } }
                .disabled(thinking)
            Button("Ask") { Task { await send(draft) } }
                .buttonStyle(.borderedProminent)
                .disabled(thinking || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
        .padding()
    }

    private func load() async {
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
}

private struct Bubble: View {
    let message: ChatMessage
    let play: (ChatItem) -> Void

    var body: some View {
        let mine = message.role == "user"
        VStack(alignment: mine ? .trailing : .leading, spacing: 12) {
            Text(message.text)
                .fontWeight(mine ? .semibold : .regular)
                .padding(mine ? 12 : 0)
                .background(mine ? AnyShapeStyle(.tint.opacity(0.15)) : AnyShapeStyle(.clear),
                            in: RoundedRectangle(cornerRadius: 16))
                .frame(maxWidth: .infinity, alignment: mine ? .trailing : .leading)
                #if os(tvOS)
                .focusable()
                #else
                .textSelection(.enabled)
                #endif
            if !message.items.isEmpty {
                ScrollView(.horizontal) {
                    LazyHStack(alignment: .top, spacing: cardSpacing) {
                        ForEach(message.items, id: \.self) { item in
                            Card(item: item, play: play)
                        }
                    }
                    #if os(tvOS)
                    .padding(.vertical, 24)
                    #endif
                }
                .scrollClipDisabled()
                #if os(tvOS)
                .focusSection()
                #endif
            }
        }
    }

    #if os(tvOS)
    private let cardSpacing: CGFloat = 40
    #else
    private let cardSpacing: CGFloat = 12
    #endif
}

/// One title the assistant offered: poster, its line about why, and Play.
private struct Card: View {
    let item: ChatItem
    let play: (ChatItem) -> Void

    var body: some View {
        Button { play(item) } label: {
            VStack(alignment: .leading, spacing: 6) {
                AsyncImage(url: item.poster.flatMap(URL.init(string:))) { image in
                    image.resizable().scaledToFill()
                } placeholder: {
                    ZStack {
                        Rectangle().fill(.quaternary)
                        Text(item.title).font(.ui(.caption)).multilineTextAlignment(.center).padding(6)
                    }
                }
                .frame(width: width, height: item.kind == "live" ? width * 9 / 16 : width * 3 / 2)
                .clipShape(RoundedRectangle(cornerRadius: 8))
                .overlay(alignment: .bottomTrailing) {
                    Image(systemName: "play.circle.fill").font(.ui(.title2))
                        .foregroundStyle(.white).shadow(radius: 3).padding(6)
                }
                Text(item.title).font(.ui(.caption)).lineLimit(1)
                let line = [item.year.map(String.init), item.note].compactMap { $0 }.joined(separator: " · ")
                if !line.isEmpty {
                    Text(line).font(.ui(.caption2)).foregroundStyle(.secondary).lineLimit(3)
                }
            }
            .frame(width: width)
        }
        .tileButtonStyle()
    }

    #if os(tvOS)
    private let width: CGFloat = 220
    #else
    private let width: CGFloat = 130
    #endif
}
