import SwiftUI
import CoreML
import FAHAppleNative

@main
struct CheckApp: App {
    var body: some Scene { WindowGroup { CheckView() } }
}
struct CheckView: View {
    @State private var status = "Running shared C ABI validation..."
    var body: some View {
        ScrollView { Text(status).font(.system(.body, design: .monospaced)).padding() }
            .task { status = await Task.detached { validate() }.value }
    }
}
private func check(_ status: Int32) throws {
    if status != 0 {
        var bytes = [CChar](repeating: 0, count: 2048)
        _ = fah_engine_last_error(&bytes, bytes.count)
        throw NSError(domain: "FAH", code: Int(status), userInfo: [NSLocalizedDescriptionKey:String(cString:bytes)])
    }
}
private func validate() -> String {
    defer { _ = fah_engine_shutdown() }
    do {
        let models = Bundle.main.resourceURL!.appendingPathComponent("models")
        let work = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("models")
        if !FileManager.default.fileExists(atPath: work.path) { try FileManager.default.copyItem(at: models, to: work) }
        let compiled = work.appendingPathComponent("detector-640.mlmodelc")
        if !FileManager.default.fileExists(atPath: compiled.path) {
            let temp = try MLModel.compileModel(at: work.appendingPathComponent("detector-640.mlpackage"))
            try FileManager.default.copyItem(at: temp, to: compiled)
        }
        var session: UInt64 = 0
        try work.path.withCString { path in
            var config = FahConfig(abi_version: 1, struct_size: UInt32(MemoryLayout<FahConfig>.size), models_utf8: path, native_library_utf8: nil, backend: 5)
            try check(fah_engine_create(&config, &session))
        }
        defer { _ = fah_engine_destroy(session) }
        let pixels = try Data(contentsOf: work.appendingPathComponent("sample.rgba"))
        var result: UInt64 = 0
        try pixels.withUnsafeBytes { bytes in
            var input = FahImage(data: bytes.baseAddress!.assumingMemoryBound(to: UInt8.self), byte_len: bytes.count, width: 640, height: 426, stride: 2560, reset: 1)
            try check(fah_engine_process(session, &input, &result))
        }
        defer { _ = fah_engine_result_destroy(result) }
        var native = FahResult()
        try check(fah_engine_result_data(result, &native, MemoryLayout<FahResult>.size))
        var size = 0
        guard fah_engine_result_json(result, nil, 0, &size) == 2 else { throw NSError(domain: "FAH", code: 2) }
        var bytes = [CChar](repeating: 0, count: size)
        try check(fah_engine_result_json(result, &bytes, size, &size))
        let text = String(cString: bytes)
        let output = work.deletingLastPathComponent().appendingPathComponent("engine-result.json")
        try text.write(to: output, atomically: true, encoding: .utf8)
        print("FAH_ENGINE_RESULT \(text)")
        return "C ABI OK\nHands: \(native.hand_count)\nProcessing: \(native.total_ms) ms\n\n\(text)"
    } catch { print("FAH_ENGINE_ERROR \(error)"); return "Error: \(error)" }
}
