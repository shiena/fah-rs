import FAHApple
import Foundation

// A concatenated RGBA8 video exercises tracking across distinct frames.
do {
    let args = CommandLine.arguments
    guard args.count == 5, let width = Int(args[3]), let height = Int(args[4]),
          (1...8192).contains(width), (1...8192).contains(height) else {
        throw FAHAppleError.message("Usage: fah-apple-check MODELS FRAMES.rgba WIDTH HEIGHT")
    }
    let frames = try Data(contentsOf: URL(fileURLWithPath: args[2]))
    let size = width * height * 4
    guard !frames.isEmpty, frames.count % size == 0 else { throw FAHAppleError.message("Incomplete RGBA frames") }
    let session = try TrackingSession(models: URL(fileURLWithPath: args[1]))
    for offset in stride(from: 0, to: frames.count, by: size) {
        let result = try session.process(rgba: frames.subdata(in: offset..<(offset + size)),
                                        width: width, height: height, stride: width * 4, reset: offset == 0)
        FileHandle.standardOutput.write(result)
        FileHandle.standardOutput.write(Data([10]))
    }
} catch {
    FileHandle.standardError.write(Data("\(error)\n".utf8))
    exit(1)
}
