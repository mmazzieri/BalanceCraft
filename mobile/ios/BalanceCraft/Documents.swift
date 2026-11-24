import Foundation

struct Today: Identifiable {
    let id: String
    let name: String
    let path: String
    let due: String
}

/// A line of the tree: an area (depth 0) or a task.
struct Node: Identifiable {
    let id: String
    let name: String
    let depth: Int
    var due = ""
    var notes = ""
    var weight: Int?
    var onHold: String?
}

/// The shared document and its mirror on this phone. The engine works on the mirror, where each
/// change stays in a replica; the mirror is then carried to the document. File work runs on a
/// queue of its own, one task at a time.
@MainActor
final class Documents: ObservableObject {
    @Published private(set) var ready = false
    @Published private(set) var open = false
    @Published private(set) var busy = false
    @Published private(set) var today: [Today] = []
    @Published private(set) var tree: [Node] = []
    /// The shared document could not be reached: changes wait on the phone.
    @Published private(set) var offline = false
    @Published var error: String?

    private let work = DispatchQueue(label: "balancecraft.document")
    private let worker = Worker()
    private let defaults = UserDefaults.standard

    init() {
        if let saved = defaults.data(forKey: "document"), let id = defaults.string(forKey: "id") {
            var stale = false
            let url = try? URL(resolvingBookmarkData: saved, bookmarkDataIsStale: &stale)
            if let url, url.startAccessingSecurityScopedResource(), stale, let fresh = try? url.bookmarkData() {
                defaults.set(fresh, forKey: "document")
            }
            // Without the document the mirror still shows what there was.
            open = true
            run { w in
                w.use(url, id)
                try w.round(["cmd": "today"])
            }
        }
        ready = true
    }

    /// Uses a document the person picked.
    func choose(_ url: URL) {
        guard url.startAccessingSecurityScopedResource() else {
            error = String(localized: "The document cannot be opened.")
            return
        }
        let bookmark = try? url.bookmarkData()
        run { w in
            let id = try w.identify(url)
            w.use(url, id)
            UserDefaults.standard.set(bookmark, forKey: "document")
            UserDefaults.standard.set(id, forKey: "id")
            try w.round(["cmd": "today"])
        } then: { $0.open = true }
    }

    /// A new document, to be exported where the person wants it.
    func blank() async -> String? {
        let worker = self.worker
        return await withCheckedContinuation { done in
            work.async { done.resume(returning: try? worker.blank()) }
        }
    }

    /// Looks at the document again: other computers may have changed it.
    func reload() {
        if open { change(["cmd": "today"]) }
    }

    /// A change: a command of the engine.
    func change(_ command: [String: Any]) {
        run { w in try w.round(command) }
    }

    private func run(_ body: @escaping (Worker) throws -> Void, then: @escaping (Documents) -> Void = { _ in }) {
        busy = true
        let worker = self.worker
        work.async {
            var failure: String?
            do { try body(worker) } catch { failure = error.localizedDescription }
            let (today, tree, offline) = (worker.today, worker.tree, worker.offline)
            Task { @MainActor in
                self.busy = false
                self.today = today
                self.tree = tree
                self.offline = offline
                if let failure { self.error = failure } else { then(self) }
            }
        }
    }
}

/// The document's file work, done on one queue.
final class Worker: @unchecked Sendable {
    private(set) var today: [Today] = []
    private(set) var tree: [Node] = []
    private(set) var offline = false
    private var url: URL?
    private var id = ""
    private let manager = FileManager.default

    func use(_ url: URL?, _ id: String) {
        self.url = url
        self.id = id
        today = []
        tree = []
    }

    /// The id of the document in a file.
    func identify(_ url: URL) throws -> String { try check(read(url)) }

    func blank() throws -> String {
        let file = try support("new").appendingPathComponent("document.balancecraft")
        try? manager.removeItem(at: file)
        try Core.ask("new", ["mirror": file.path, "state": try support("replicas").path])
        defer { try? manager.removeItem(at: file) }
        return try String(contentsOf: file, encoding: .utf8)
    }

    /// Merges the document in the mirror, runs `command` there, and carries the result back.
    func round(_ command: [String: Any]) throws {
        let mirror = try self.mirror()
        var shared: String?
        do {
            if let url { shared = try fetch(url) }
        } catch let e as CoreError {
            throw e
        } catch {
            shared = nil
        }
        if let shared {
            try put(shared, mirror)
        } else if !manager.fileExists(atPath: mirror.path) {
            throw CoreError(message: "the document could not be read")
        }
        // Once here, the change is in the replica: what follows may fail and be tried again later.
        try run(command)
        offline = !((try? carry(shared, mirror)) ?? false)
        try show()
    }

    private func carry(_ first: String?, _ mirror: URL) throws -> Bool {
        guard let url, var shared = first else { return false }
        for _ in 0..<5 {
            let ours = try String(contentsOf: mirror, encoding: .utf8)
            if ours == shared { return true }
            let now = try fetch(url)
            if now != shared {
                // Changed elsewhere meanwhile: merge that first.
                shared = now
                try put(now, mirror)
                try run(["cmd": "today"])
                continue
            }
            try write(ours, url)
            shared = ours
        }
        return try String(contentsOf: mirror, encoding: .utf8) == shared
    }

    private func fetch(_ url: URL) throws -> String {
        let text = try read(url)
        if try check(text) != id { throw CoreError(message: "the file holds another document; open it again") }
        return text
    }

    private func check(_ text: String) throws -> String {
        let answer = try Core.ask("check", ["text": text]) as? [String: String]
        guard let id = answer?["document"] else { throw CoreError(message: "the file is not a BalanceCraft document") }
        return id
    }

    private func read(_ url: URL) throws -> String {
        var result: Result<String, Error> = .failure(CocoaError(.fileReadUnknown))
        var coordination: NSError?
        NSFileCoordinator().coordinate(readingItemAt: url, options: [], error: &coordination) { at in
            result = Result { String(decoding: try Data(contentsOf: at), as: UTF8.self) }
        }
        if let coordination { throw coordination }
        return try result.get()
    }

    private func write(_ text: String, _ url: URL) throws {
        var failure: Error?
        var coordination: NSError?
        NSFileCoordinator().coordinate(writingItemAt: url, options: .forReplacing, error: &coordination) { at in
            do { try Data(text.utf8).write(to: at, options: .atomic) } catch { failure = error }
        }
        if let coordination { throw coordination }
        if let failure { throw failure }
    }

    private func put(_ text: String, _ file: URL) throws {
        try Data(text.utf8).write(to: file, options: .atomic)
    }

    @discardableResult
    private func run(_ command: [String: Any]) throws -> Any? {
        try Core.ask("run", ["mirror": try mirror().path, "state": try support("replicas").path, "command": command])
    }

    private func show() throws {
        let list = (try run(["cmd": "today"]) as? [String: Any])?["tasks"] as? [[String: Any]] ?? []
        today = list.compactMap { t in
            guard let id = t["id"] as? String else { return nil }
            let path = (t["path"] as? [String] ?? []).dropLast().joined(separator: " › ")
            return Today(id: id, name: t["name"] as? String ?? "", path: path, due: t["due"] as? String ?? "")
        }
        var nodes: [Node] = []
        walk((try run(["cmd": "list"]) as? [String: Any])?["areas"] as? [[String: Any]] ?? [], 0, &nodes)
        tree = nodes
    }

    private func walk(_ list: [[String: Any]], _ depth: Int, _ out: inout [Node]) {
        for o in list {
            guard let id = o["id"] as? String else { continue }
            out.append(Node(
                id: id, name: o["name"] as? String ?? "", depth: depth, due: o["due"] as? String ?? "",
                notes: o["notes"] as? String ?? "", weight: o["weight"] as? Int, onHold: o["on_hold"] as? String
            ))
            walk(o["tasks"] as? [[String: Any]] ?? [], depth + 1, &out)
        }
    }

    private func support(_ name: String) throws -> URL {
        let base = try manager.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let dir = base.appendingPathComponent(name)
        try manager.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    /// A folder for each document: what lies beside a document is searched for conflict copies.
    private func mirror() throws -> URL { try support("mirrors/\(id)").appendingPathComponent("document.balancecraft") }
}
