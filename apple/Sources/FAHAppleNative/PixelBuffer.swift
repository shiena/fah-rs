import CoreVideo
import Foundation

/// Frame input conversion is separate from inference. Orientation is host-owned.
public enum FrameInput {
    public static func rgba(from buffer: CVPixelBuffer) throws -> (data: Data, width: Int, height: Int, stride: Int) {
        guard CVPixelBufferGetPixelFormatType(buffer) == kCVPixelFormatType_32BGRA,
              !CVPixelBufferIsPlanar(buffer) else {
            throw FAHAppleError.message("Configure capture for non-planar 32BGRA pixel buffers")
        }
        guard CVPixelBufferLockBaseAddress(buffer, .readOnly) == kCVReturnSuccess else {
            throw FAHAppleError.message("Cannot lock pixel buffer")
        }
        defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
        let width = CVPixelBufferGetWidth(buffer), height = CVPixelBufferGetHeight(buffer)
        let stride = CVPixelBufferGetBytesPerRow(buffer)
        guard (1...8192).contains(width), (1...8192).contains(height), stride >= width * 4,
              let base = CVPixelBufferGetBaseAddress(buffer) else {
            throw FAHAppleError.message("Invalid pixel buffer")
        }
        let source = base.assumingMemoryBound(to: UInt8.self)
        var data = Data(count: width * height * 4)
        data.withUnsafeMutableBytes { raw in
            let target = raw.bindMemory(to: UInt8.self)
            for y in 0..<height { for x in 0..<width {
                let s = y * stride + x * 4, d = (y * width + x) * 4
                target[d] = source[s + 2]; target[d + 1] = source[s + 1]
                target[d + 2] = source[s]; target[d + 3] = source[s + 3]
            } }
        }
        return (data, width, height, width * 4)
    }
}
