import CoreML
import CoreVideo
import XCTest
@testable import FAHApple
@testable import FAHAppleNative

final class AdapterTests: XCTestCase {
    func testDFLDecodePreservesChannelOrder() throws {
        let boxes = try MLMultiArray(shape: [1,64,8400], dataType: .float32)
        let scores = try MLMultiArray(shape: [1,2,8400], dataType: .float32)
        boxes.dataPointer.assumingMemoryBound(to: Float.self).initialize(repeating: 0, count: boxes.count)
        scores.dataPointer.assumingMemoryBound(to: Float.self).initialize(repeating: 0, count: scores.count)
        var output = [Float](repeating: .nan, count: 6 * 8400)
        try output.withUnsafeMutableBufferPointer { try MetalRuntime.decode(boxes: boxes, scores: scores, output: $0.baseAddress!) }
        XCTAssertEqual(output[0], 4)
        XCTAssertEqual(output[8400], 4)
        XCTAssertEqual(output[16800], 120)
        XCTAssertEqual(output[25200], 120)
        XCTAssertEqual(output[33600], 0.5)
        XCTAssertEqual(output[42000], 0.5)
        XCTAssertEqual(output[6400], 8)
        XCTAssertEqual(output[8000], 16)
        XCTAssertTrue(output.allSatisfy(\.isFinite))
    }
    func testPixelBufferConversion() throws {
        var buffer: CVPixelBuffer?
        XCTAssertEqual(CVPixelBufferCreate(kCFAllocatorDefault, 2, 1, kCVPixelFormatType_32BGRA, nil, &buffer), kCVReturnSuccess)
        let pixelBuffer = try XCTUnwrap(buffer)
        CVPixelBufferLockBaseAddress(pixelBuffer, [])
        let bytes = CVPixelBufferGetBaseAddress(pixelBuffer)!.assumingMemoryBound(to: UInt8.self)
        for (index, value) in [UInt8(1),2,3,255,4,5,6,255].enumerated() { bytes[index] = value }
        CVPixelBufferUnlockBaseAddress(pixelBuffer, [])
        let frame = try FrameInput.rgba(from: pixelBuffer)
        XCTAssertEqual(Array(frame.data), [3,2,1,255,6,5,4,255])
        XCTAssertEqual(frame.stride, 8)
    }
}
