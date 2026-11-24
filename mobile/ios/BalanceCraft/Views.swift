import SwiftUI
import UniformTypeIdentifiers

/// A document to be written where the person chooses.
struct NewDocument: FileDocument {
    static let readableContentTypes: [UTType] = [.data]
    var text: String

    init(text: String) { self.text = text }
    init(configuration: ReadConfiguration) throws {
        text = String(decoding: configuration.file.regularFileContents ?? Data(), as: UTF8.self)
    }
    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: Data(text.utf8))
    }
}

/// What a form edits: a task, or a new one or a new area (`node` is nil).
struct TaskForm: Identifiable {
    let id = UUID()
    var node: Node?
    var parent: String?
}

struct RootView: View {
    @EnvironmentObject var documents: Documents
    @State private var picking = false
    @State private var saving = false
    @State private var blank = NewDocument(text: "")

    var body: some View {
        Group {
            if !documents.ready {
                ProgressView()
            } else if !documents.open {
                VStack(spacing: 16) {
                    Text("BalanceCraft").font(.largeTitle)
                    Text("Open the .balancecraft document your computers share, or make a new one.").multilineTextAlignment(.center)
                    Button("Open a document") { picking = true }.buttonStyle(.borderedProminent)
                    Button("New document") {
                        Task {
                            guard let text = await documents.blank() else { return }
                            blank = NewDocument(text: text)
                            saving = true
                        }
                    }
                }
                .padding(24)
            } else {
                TabView {
                    NavigationStack { TodayView(pick: { picking = true }) }.tabItem { Label("Today", systemImage: "checklist") }
                    NavigationStack { AreasView(pick: { picking = true }) }.tabItem { Label("Areas", systemImage: "list.bullet.indent") }
                }
            }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.item]) { result in
            if case .success(let url) = result { documents.choose(url) }
        }
        .background(
            Color.clear.fileExporter(isPresented: $saving, document: blank, contentType: .data, defaultFilename: "Life.balancecraft") { result in
                if case .success = result { picking = true }
            }
        )
        .alert(
            "Something went wrong",
            isPresented: Binding(get: { documents.error != nil }, set: { if !$0 { documents.error = nil } }),
            actions: { Button("Close") { documents.error = nil } },
            message: { Text(documents.error ?? "") }
        )
    }
}

struct DocumentToolbar: ToolbarContent {
    @EnvironmentObject var documents: Documents
    let pick: () -> Void

    var body: some ToolbarContent {
        ToolbarItemGroup(placement: .navigationBarTrailing) {
            if documents.busy { ProgressView() }
            Button { documents.reload() } label: { Label("Read again", systemImage: "arrow.clockwise") }
            Button("Open", action: pick)
        }
    }
}

struct Offline: View {
    @EnvironmentObject var documents: Documents

    var body: some View {
        if documents.offline {
            Text("The shared document could not be updated. Changes are kept on the phone and written at the next try.").font(.footnote)
        }
    }
}

struct Line: View {
    @EnvironmentObject var documents: Documents
    let id: String
    let name: String
    let detail: String
    let open: () -> Void

    var body: some View {
        HStack {
            Button { documents.change(["cmd": "complete", "id": id]) } label: { Label("Complete", systemImage: "circle").labelStyle(.iconOnly) }
                .buttonStyle(.borderless)
            Button(action: open) {
                VStack(alignment: .leading) {
                    Text(name).foregroundStyle(.primary)
                    if !detail.isEmpty { Text(detail).font(.footnote).foregroundStyle(.secondary) }
                }
            }
            .buttonStyle(.plain)
        }
    }
}

struct TodayView: View {
    @EnvironmentObject var documents: Documents
    let pick: () -> Void
    @State private var form: TaskForm?

    var body: some View {
        List {
            Offline()
            if documents.today.isEmpty && !documents.busy { Text("Nothing to do now.") }
            ForEach(documents.today) { t in
                let detail = [t.path, t.due.replacingOccurrences(of: "T", with: " ")].filter { !$0.isEmpty }.joined(separator: " · ")
                Line(id: t.id, name: t.name, detail: detail) {
                    if let node = documents.tree.first(where: { $0.id == t.id }) { form = TaskForm(node: node) }
                }
            }
        }
        .navigationTitle("Today")
        .refreshable { documents.reload() }
        .toolbar { DocumentToolbar(pick: pick) }
        .sheet(item: $form) { FormView(form: $0) }
    }
}

struct AreasView: View {
    @EnvironmentObject var documents: Documents
    let pick: () -> Void
    @State private var form: TaskForm?

    var body: some View {
        List {
            Offline()
            ForEach(documents.tree) { n in
                if n.depth == 0 {
                    HStack {
                        Text(n.name).bold()
                        Spacer()
                        Button { form = TaskForm(parent: n.id) } label: { Label("New task", systemImage: "plus").labelStyle(.iconOnly) }
                            .buttonStyle(.borderless)
                    }
                } else {
                    Line(id: n.id, name: n.name, detail: detail(n)) { form = TaskForm(node: n) }
                        .padding(.leading, CGFloat(16 * (n.depth - 1)))
                }
            }
        }
        .navigationTitle("Areas")
        .refreshable { documents.reload() }
        .toolbar {
            ToolbarItem(placement: .navigationBarLeading) {
                Button { form = TaskForm() } label: { Label("New area", systemImage: "plus") }
            }
            DocumentToolbar(pick: pick)
        }
        .sheet(item: $form) { FormView(form: $0) }
    }

    private func detail(_ n: Node) -> String {
        var parts: [String] = []
        if !n.due.isEmpty { parts.append(n.due.replacingOccurrences(of: "T", with: " ")) }
        if let hold = n.onHold { parts.append(String(localized: "On hold") + (hold.isEmpty ? "" : " " + hold)) }
        return parts.joined(separator: " · ")
    }
}

struct FormView: View {
    @EnvironmentObject var documents: Documents
    @Environment(\.dismiss) private var dismiss
    let form: TaskForm
    @State private var name = ""
    @State private var due = ""
    @State private var weight = ""
    @State private var notes = ""
    @State private var inside = ""
    @State private var deleting = false

    private var area: Bool { form.node == nil && form.parent == nil }

    var body: some View {
        NavigationStack {
            Form {
                TextField("Name", text: $name)
                if !area {
                    TextField("Deadline (YYYY-MM-DD)", text: $due)
                    TextField("Weight, 1 to 100", text: $weight).keyboardType(.numberPad)
                    TextField("Notes", text: $notes, axis: .vertical)
                }
                if let node = form.node {
                    Section {
                        HStack {
                            TextField("New task", text: $inside)
                            Button("Add") { send(["cmd": "add", "parent": node.id, "name": inside]) }.disabled(blank(inside))
                        }
                        if node.onHold == nil {
                            Button("Put on hold") { send(["cmd": "hold", "id": node.id]) }
                        } else {
                            Button("Resume") { send(["cmd": "resume", "id": node.id]) }
                        }
                        Button("Delete", role: .destructive) { deleting = true }
                    }
                }
            }
            .confirmationDialog("Delete with everything inside", isPresented: $deleting, titleVisibility: .visible) {
                Button("Delete", role: .destructive) { if let node = form.node { send(["cmd": "delete", "id": node.id]) } }
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) { Button("Save", action: save).disabled(blank(name)) }
            }
        }
        .onAppear {
            guard let n = form.node else { return }
            (name, due, notes, weight) = (n.name, n.due, n.notes, n.weight.map(String.init) ?? "")
        }
    }

    private func blank(_ s: String) -> Bool { s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    private func send(_ command: [String: Any]) {
        documents.change(command)
        dismiss()
    }

    private func save() {
        if area { return send(["cmd": "add_area", "name": name]) }
        guard let n = form.node else {
            var add: [String: Any] = ["cmd": "add", "parent": form.parent ?? "", "name": name]
            if !blank(due) { add["due"] = due }
            if !blank(notes) { add["notes"] = notes }
            if let w = Int(weight) { add["weight"] = w }
            return send(add)
        }
        // Only what was changed, so that another computer's edit of the rest is kept.
        var edit: [String: Any] = ["cmd": "edit", "id": n.id]
        if name != n.name { edit["name"] = name }
        if due != n.due { edit["due"] = blank(due) ? NSNull() : due as Any }
        if notes != n.notes { edit["notes"] = blank(notes) ? NSNull() : notes as Any }
        if weight != (n.weight.map(String.init) ?? "") { edit["weight"] = Int(weight).map { $0 as Any } ?? NSNull() }
        send(edit)
    }
}
