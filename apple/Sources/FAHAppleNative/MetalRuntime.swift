import CoreML
import FAHMetal
import Foundation

// The raw network adapter preserves the Rust core's preprocessing and tracking.
public final class MetalRuntime {
    let pose: FAHPoseNetwork
    let detector: MLModel
    let input: MLMultiArray

    public init(models: URL) throws {
        pose = try FAHPoseNetwork(modelDirectory: models, maxBatch: 1, contact: false)
        let configuration = MLModelConfiguration()
        configuration.computeUnits = .all
        configuration.functionName = "s640x640"
        let compiled = models.appendingPathComponent("detector-640.mlmodelc")
        guard FileManager.default.fileExists(atPath: compiled.path) else {
            throw FAHAppleError.message("Compile detector-640.mlpackage with tools/apple-models.py on a Mac first")
        }
        detector = try MLModel(contentsOf: compiled, configuration: configuration)
        input = try MLMultiArray(shape: [1, 3, 640, 640], dataType: .float16)
    }

    public func detect(_ image: UnsafePointer<Float>, output: UnsafeMutablePointer<Float>) throws {
        let pointer = input.dataPointer.bindMemory(to: Float16.self, capacity: 3 * 640 * 640)
        let strides = input.strides.map(\.intValue)
        for c in 0..<3 { for y in 0..<640 { for x in 0..<640 {
            pointer[c * strides[1] + y * strides[2] + x * strides[3]] = Float16(image[c * 640 * 640 + y * 640 + x])
        } } }
        let prediction = try detector.prediction(from: MLDictionaryFeatureProvider(dictionary: ["x": input]))
        guard let boxes = prediction.featureValue(for: "boxes")?.multiArrayValue,
              let scores = prediction.featureValue(for: "scores")?.multiArrayValue else {
            throw FAHAppleError.message("Detector boxes/scores are missing")
        }
        try Self.decode(boxes: boxes, scores: scores, output: output)
    }

    // DFL decoding follows fah-metal Detector.swift; NMS remains in the Rust core.
    static func decode(boxes: MLMultiArray, scores: MLMultiArray, output: UnsafeMutablePointer<Float>) throws {
        guard boxes.shape.map(\.intValue) == [1,64,8400], scores.shape.map(\.intValue) == [1,2,8400] else {
            throw FAHAppleError.message("Expected detector s640x640 output shapes")
        }
        func read(_ array: MLMultiArray, _ channel: Int, _ anchor: Int) throws -> Float {
            let offset = channel * array.strides[1].intValue + anchor * array.strides[2].intValue
            switch array.dataType {
            case .float16: return Float(array.dataPointer.assumingMemoryBound(to: Float16.self)[offset])
            case .float32: return array.dataPointer.assumingMemoryBound(to: Float.self)[offset]
            default: throw FAHAppleError.message("Unsupported detector output type")
            }
        }
        var base = 0
        for stride in [8,16,32] {
            let width = 640 / stride
            for index in 0..<(width * width) {
                let anchor = base + index
                var distance = [Float](repeating: 0, count: 4)
                for side in 0..<4 {
                    let logits = try (0..<16).map { try read(boxes, side * 16 + $0, anchor) }
                    guard logits.allSatisfy(\.isFinite) else { throw FAHAppleError.message("Non-finite DFL logits") }
                    let peak = logits.max()!
                    let weights = logits.map { exp($0 - peak) }
                    distance[side] = zip(0..<16, weights).reduce(Float(0)) { $0 + Float($1.0) * $1.1 } / weights.reduce(0,+)
                }
                let x = Float(index % width) + 0.5, y = Float(index / width) + 0.5
                let scale = Float(stride)
                output[anchor] = (x + (distance[2] - distance[0]) * 0.5) * scale
                output[8400 + anchor] = (y + (distance[3] - distance[1]) * 0.5) * scale
                output[16800 + anchor] = (distance[0] + distance[2]) * scale
                output[25200 + anchor] = (distance[1] + distance[3]) * scale
                for channel in 0..<2 {
                    let logit = try read(scores, channel, anchor)
                    guard logit.isFinite else { throw FAHAppleError.message("Non-finite detector score") }
                    output[(4 + channel) * 8400 + anchor] = 1 / (1 + exp(-logit))
                }
            }
            base += width * width
        }
    }

    public func predict(_ image: UnsafePointer<Float>, ray: UnsafePointer<Float>, condition: UnsafePointer<Float>, output: UnsafeMutablePointer<Float>) throws {
        let (value, _) = try pose.predict(img: image, ray: ray, cond: condition, batch: 1)
        let arrays = [value.uv, value.xyz, value.conf, value.vis, value.depth, value.verify]
        guard arrays.map(\.count) == [922,1383,461,461,1,2] else {
            throw FAHAppleError.message("Unexpected Metal pose output shape")
        }
        var offset = 0
        for array in arrays {
            array.withUnsafeBufferPointer { output.advanced(by: offset).update(from: $0.baseAddress!, count: $0.count) }
            offset += array.count
        }
    }
}
