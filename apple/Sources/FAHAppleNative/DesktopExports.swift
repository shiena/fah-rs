#if os(macOS)
import AVFoundation
import Darwin
import Foundation

@_cdecl("fah_macos_abi_version")
public func nativeVersion() -> UInt32 { 1 }

private func checked(_ errorBuffer: UnsafeMutablePointer<CChar>?, _ body: () throws -> Void) -> Int32 {
    do { try autoreleasepool(invoking: body); return 0 }
    catch {
        if let destination = errorBuffer {
            let bytes = Array(String(describing: error).utf8.prefix(1023))
            for (i, byte) in bytes.enumerated() { destination[i] = CChar(bitPattern: byte) }
            destination[bytes.count] = 0
        }
        return -1
    }
}
@_cdecl("fah_macos_runtime_create")
public func runtimeCreate(_ path: UnsafePointer<CChar>?, _ error: UnsafeMutablePointer<CChar>?) -> UnsafeMutableRawPointer? {
    var result: UnsafeMutableRawPointer?
    _ = checked(error) {
        guard let path else { throw FAHAppleError.message("Missing models path") }
        result = Unmanaged.passRetained(try MetalRuntime(models: URL(fileURLWithPath: String(cString: path)))).toOpaque()
    }
    return result
}
@_cdecl("fah_macos_detect")
public func nativeDetect(_ context: UnsafeMutableRawPointer?, _ image: UnsafePointer<Float>?, _ output: UnsafeMutablePointer<Float>?, _ error: UnsafeMutablePointer<CChar>?) -> Int32 {
    checked(error) {
        guard let context, let image, let output else { throw FAHAppleError.message("Invalid detect pointers") }
        try Unmanaged<MetalRuntime>.fromOpaque(context).takeUnretainedValue().detect(image, output: output)
    }
}
@_cdecl("fah_macos_pose")
public func nativePose(_ context: UnsafeMutableRawPointer?, _ image: UnsafePointer<Float>?, _ ray: UnsafePointer<Float>?, _ condition: UnsafePointer<Float>?, _ output: UnsafeMutablePointer<Float>?, _ error: UnsafeMutablePointer<CChar>?) -> Int32 {
    checked(error) {
        guard let context, let image, let ray, let condition, let output else { throw FAHAppleError.message("Invalid pose pointers") }
        try Unmanaged<MetalRuntime>.fromOpaque(context).takeUnretainedValue().predict(image, ray: ray, condition: condition, output: output)
    }
}
@_cdecl("fah_macos_runtime_destroy")
public func runtimeDestroy(_ context: UnsafeMutableRawPointer?) {
    if let context { Unmanaged<MetalRuntime>.fromOpaque(context).release() }
}
@_cdecl("fah_macos_free")
public func nativeFree(_ data: UnsafeMutableRawPointer?) { free(data) }
@_cdecl("fah_macos_cameras")
public func nativeCameras(_ error: UnsafeMutablePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    var result: UnsafeMutablePointer<CChar>?
    _ = checked(error) {
        let data = try JSONSerialization.data(withJSONObject: CameraSource.devices().map(\.localizedName))
        result = String(decoding: data, as: UTF8.self).withCString { strdup($0) }
    }
    return result
}

private final class NativeStream {
    private let lock = NSLock()
    private var latest: (Data, Int, Int)?
    private var stopped = false
    private var ended = false
    private var failure: String?
    private var camera: CameraSource?
    private var reader: AVAssetReader?

    init(kind: Int32, path: String) throws {
        if kind == 0 {
            let signal = DispatchSemaphore(value: 0)
            var allowed = false
            CameraSource.requestAccess { value in allowed = value; signal.signal() }
            guard signal.wait(timeout: .now() + 60) == .success, allowed else {
                throw FAHAppleError.message("Camera access denied or permission request timed out")
            }
            guard let device = CameraSource.devices().first(where: { $0.localizedName == path }) else { throw FAHAppleError.message("Camera not found: \(path)") }
            camera = try CameraSource(device: device)
            camera!.start { [weak self] buffer in self?.publish(buffer) }
        } else if kind == 1 {
            let asset = AVURLAsset(url: URL(fileURLWithPath: path))
            guard let track = asset.tracks(withMediaType: .video).first else { throw FAHAppleError.message("No video track") }
            let reader = try AVAssetReader(asset: asset)
            let output = AVAssetReaderTrackOutput(track: track, outputSettings: [kCVPixelBufferPixelFormatTypeKey as String:kCVPixelFormatType_32BGRA])
            guard reader.canAdd(output) else { throw FAHAppleError.message("Cannot decode video") }
            reader.add(output); self.reader = reader
            guard reader.startReading() else { throw reader.error ?? FAHAppleError.message("Cannot start video") }
            DispatchQueue(label: "FAH.video").async { [self] in
                let start = Date.timeIntervalSinceReferenceDate
                var first: Double?
                while !isStopped(), let sample = output.copyNextSampleBuffer() {
                    let timestamp = CMSampleBufferGetPresentationTimeStamp(sample).seconds
                    if first == nil { first = timestamp }
                    let delay = timestamp - first! - (Date.timeIntervalSinceReferenceDate - start)
                    if delay > 0 { Thread.sleep(forTimeInterval: min(delay, 1)) }
                    if let buffer = CMSampleBufferGetImageBuffer(sample) { publish(buffer) }
                }
                lock.lock()
                if reader.status == .failed { failure = String(describing: reader.error) }
                ended = true; lock.unlock()
            }
        } else { throw FAHAppleError.message("Unknown input kind") }
    }
    private func isStopped() -> Bool { lock.lock(); defer { lock.unlock() }; return stopped }
    private func publish(_ buffer: CVPixelBuffer) {
        do {
            let value = try FrameInput.rgba(from: buffer)
            lock.lock(); defer { lock.unlock() }
            if !stopped { latest = (value.data,value.width,value.height) }
        } catch { lock.lock(); failure = String(describing: error); lock.unlock() }
    }
    func read(_ pointer: UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>, _ count: UnsafeMutablePointer<Int>, _ width: UnsafeMutablePointer<UInt32>, _ height: UnsafeMutablePointer<UInt32>) throws -> Int32 {
        lock.lock(); defer { lock.unlock() }
        if let failure { throw FAHAppleError.message(failure) }
        guard let frame = latest else { return ended ? 2 : 0 }
        guard let memory = malloc(frame.0.count) else { throw FAHAppleError.message("Frame allocation failed") }
        frame.0.withUnsafeBytes { memory.copyMemory(from: $0.baseAddress!, byteCount: $0.count) }
        pointer.pointee = memory.assumingMemoryBound(to: UInt8.self)
        count.pointee = frame.0.count; width.pointee = UInt32(frame.1); height.pointee = UInt32(frame.2)
        latest = nil
        return 1
    }
    func stop() {
        lock.lock(); stopped = true; latest = nil; lock.unlock()
        camera?.stop(); reader?.cancelReading()
    }
}
@_cdecl("fah_macos_input_create")
public func inputCreate(_ kind: Int32, _ path: UnsafePointer<CChar>?, _ error: UnsafeMutablePointer<CChar>?) -> UnsafeMutableRawPointer? {
    var result: UnsafeMutableRawPointer?
    _ = checked(error) {
        guard let path else { throw FAHAppleError.message("Missing input path") }
        result = Unmanaged.passRetained(try NativeStream(kind: kind, path: String(cString: path))).toOpaque()
    }
    return result
}
@_cdecl("fah_macos_input_read")
public func inputRead(_ context: UnsafeMutableRawPointer?, _ pointer: UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?, _ count: UnsafeMutablePointer<Int>?, _ width: UnsafeMutablePointer<UInt32>?, _ height: UnsafeMutablePointer<UInt32>?, _ error: UnsafeMutablePointer<CChar>?) -> Int32 {
    var result: Int32 = -1
    let status = checked(error) {
        guard let context, let pointer, let count, let width, let height else { throw FAHAppleError.message("Invalid frame pointers") }
        result = try Unmanaged<NativeStream>.fromOpaque(context).takeUnretainedValue().read(pointer,count,width,height)
    }
    return status == 0 ? result : status
}
@_cdecl("fah_macos_input_destroy")
public func inputDestroy(_ context: UnsafeMutableRawPointer?) {
    if let context { let input = Unmanaged<NativeStream>.fromOpaque(context).takeRetainedValue(); input.stop() }
}
#endif
