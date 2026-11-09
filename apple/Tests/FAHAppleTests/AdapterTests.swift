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

    func testDFLDecodeReadsStridedFloat16Outputs() throws {
        let boxMemory = UnsafeMutablePointer<Float16>.allocate(capacity: 8400 * 128)
        let scoreMemory = UnsafeMutablePointer<Float16>.allocate(capacity: 8400 * 4)
        defer { boxMemory.deallocate(); scoreMemory.deallocate() }
        boxMemory.initialize(repeating: .nan, count: 8400 * 128)
        scoreMemory.initialize(repeating: .nan, count: 8400 * 4)
        for anchor in 0..<8400 {
            for channel in 0..<64 {
                boxMemory[anchor * 128 + channel] = Float16(Float(channel % 16) * Float(channel / 16 + 1) * 0.125)
            }
            scoreMemory[anchor * 4] = 0.25
            scoreMemory[anchor * 4 + 1] = -0.5
        }
        let boxes = try MLMultiArray(dataPointer: boxMemory, shape: [1,64,8400], dataType: .float16,
                                    strides: [8400 * 128,1,128].map { NSNumber(value: $0) }, deallocator: nil)
        let scores = try MLMultiArray(dataPointer: scoreMemory, shape: [1,2,8400], dataType: .float16,
                                     strides: [8400 * 4,1,4].map { NSNumber(value: $0) }, deallocator: nil)
        var output = [Float](repeating: .nan, count: 6 * 8400)
        try output.withUnsafeMutableBufferPointer { try MetalRuntime.decode(boxes: boxes, scores: scores, output: $0.baseAddress!) }
        let distances: [Float] = (0..<4).map { side in
            let weights = (0..<16).map { exp(Double($0 - 15) * Double(side + 1) * 0.125) }
            return Float(zip(0..<16, weights).reduce(0.0) { $0 + Double($1.0) * $1.1 } / weights.reduce(0,+))
        }
        for (anchor, scale) in [(0,Float(8)), (6400,Float(16)), (8000,Float(32))] {
            XCTAssertEqual(output[anchor], (0.5 + (distances[2] - distances[0]) * 0.5) * scale, accuracy: 0.0001)
            XCTAssertEqual(output[16800 + anchor], (distances[0] + distances[2]) * scale, accuracy: 0.0001)
            XCTAssertEqual(output[33600 + anchor], 1 / (1 + exp(-0.25)), accuracy: 0.0001)
            XCTAssertEqual(output[42000 + anchor], 1 / (1 + exp(0.5)), accuracy: 0.0001)
        }
        XCTAssertTrue(output.allSatisfy(\.isFinite))
        boxMemory[0] = .nan
        XCTAssertThrowsError(try output.withUnsafeMutableBufferPointer {
            try MetalRuntime.decode(boxes: boxes, scores: scores, output: $0.baseAddress!)
        })
    }
}
