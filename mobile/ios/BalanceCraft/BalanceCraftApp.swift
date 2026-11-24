import SwiftUI

@main
struct BalanceCraftApp: App {
    @StateObject private var documents = Documents()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            RootView().environmentObject(documents)
        }
        .onChange(of: phase) { now in
            if now == .active { documents.reload() }
        }
    }
}
