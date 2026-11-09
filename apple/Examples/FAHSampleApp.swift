import FAHApple
import Foundation
import SwiftUI

// Add this file to a macOS/iOS SwiftUI App target, replacing its generated App.
// Add FAHApple as a local package and copy models as a folder reference.
@main
struct FAHSampleApp: App {
    var body: some Scene { WindowGroup { SampleView() } }
}

private struct SampleView: View {
    @State private var text = "Run the bundled 640 x 426 RGBA sample."
    @State private var running = false
    var body: some View {
        VStack {
            Button(running ? "Processing…" : "Run sample") {
                running = true
                Task {
                    do {
                        text = try await Task.detached(priority: .userInitiated) {
                            guard let models = Bundle.main.resourceURL?.appendingPathComponent("models") else {
                                throw FAHAppleError.message("Missing model resources")
                            }
                            let session = try TrackingSession(models: models)
                            let image = try Data(contentsOf: models.appendingPathComponent("sample.rgba"))
                            let result = try session.process(rgba: image, width: 640, height: 426, stride: 2560, reset: true)
                            return String(decoding: result, as: UTF8.self)
                        }.value
                    } catch { text = String(describing: error) }
                    running = false
                }
            }.disabled(running)
            ScrollView { Text(text).font(.system(.caption, design: .monospaced)).textSelection(.enabled) }
        }.padding()
    }
}
