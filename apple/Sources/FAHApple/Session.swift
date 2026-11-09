import FAHCore
import Foundation

public enum FAHAppleError: Error, LocalizedError {
    case message(String)
    public var errorDescription: String? { if case .message(let text) = self { return text }; return nil }
}

private func callback(_ errorBuffer: UnsafeMutablePointer<CChar>?, _ operation: () throws -> Void) -> Int32 {
    do { try autoreleasepool(invoking: operation); return 0 }
    catch {
        if let destination = errorBuffer {
            let bytes = Array(String(describing: error).utf8.prefix(1023))
            for (index, byte) in bytes.enumerated() { destination[index] = CChar(bitPattern: byte) }
            destination[bytes.count] = 0
        }
        return -1
    }
}

/// Host-owned synchronous session. Call process from your capture/render queue.
/// No camera, GUI, worker thread, MANO fitting, or contact estimation is created.
public final class TrackingSession {
    private let handle: OpaquePointer
    public init(models: URL) throws {
        let runtime = try MetalRuntime(models: models)
        let context = Unmanaged.passRetained(runtime).toOpaque()
        let callbacks = FahMetalCallbacks(
            abi_version: 1, context: context,
            detect: { context, image, output, error in
                callback(error) {
                    guard let context, let image, let output else { throw FAHAppleError.message("Missing detector buffers") }
                    try Unmanaged<MetalRuntime>.fromOpaque(context).takeUnretainedValue().detect(image, output: output)
                }
            },
            pose: { context, image, ray, condition, output, error in
                callback(error) {
                    guard let context, let image, let ray, let condition, let output else { throw FAHAppleError.message("Missing pose buffers") }
                    try Unmanaged<MetalRuntime>.fromOpaque(context).takeUnretainedValue().predict(image, ray: ray, condition: condition, output: output)
                }
            },
            destroy: { context in
                if let context { Unmanaged<MetalRuntime>.fromOpaque(context).release() }
            })
        var error: UnsafeMutablePointer<CChar>?
        // The valid callback context is consumed even if Rust initialization fails.
        let created = models.path.withCString { fah_apple_create($0, callbacks, &error) }
        guard let created else { throw Self.takeError(error) }
        handle = created
    }
    deinit { fah_apple_destroy(handle) }

    /// RGBA8 in upright image coordinates; stride may include row padding.
    /// Returns the shared JSON schema with 21 joints per hand, at most two hands.
    public func process(rgba: Data, width: Int, height: Int, stride: Int, reset: Bool = false) throws -> Data {
        guard (1...8192).contains(width), (1...8192).contains(height), stride >= width * 4 else {
            throw FAHAppleError.message("Invalid image dimensions or stride")
        }
        var error: UnsafeMutablePointer<CChar>?
        let result = rgba.withUnsafeBytes { bytes in
            fah_apple_process(handle, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count,
                              UInt32(width), UInt32(height), stride, reset ? 1 : 0, &error)
        }
        guard let result else { throw Self.takeError(error) }
        defer { fah_apple_string_free(result) }
        return Data(String(cString: result).utf8)
    }
    private static func takeError(_ pointer: UnsafeMutablePointer<CChar>?) -> FAHAppleError {
        guard let pointer else { return .message("Apple tracking failed") }
        defer { fah_apple_string_free(pointer) }
        return .message(String(cString: pointer))
    }
}
