import Foundation
import Darwin

// Minimal error/resource support extracted from upstream Types.swift and ImageWarp.swift.
public enum FAHError: Error, CustomStringConvertible {
    case missingResource(String), badResource(String), metal(String)
    public var description: String {
        switch self {
        case .missingResource(let text): return "FAHMetal: missing resource \(text)"
        case .badResource(let text): return "FAHMetal: bad resource \(text)"
        case .metal(let text): return "FAHMetal: Metal: \(text)"
        }
    }
}

func shaderSource(_ name: String) throws -> String {
    #if os(macOS)
    // Engine plugins keep resources beside the loaded native library.
    var info = Dl_info()
    if dladdr(#dsohandle, &info) != 0, let filename = info.dli_fname {
        let directory = URL(fileURLWithPath: String(cString: filename)).deletingLastPathComponent()
        if let bundle = Bundle(url: directory.appendingPathComponent("fah-metal_FAHMetal.bundle")),
           let url = bundle.url(forResource: name, withExtension: "metal", subdirectory: "Shaders") {
            return try String(contentsOf: url, encoding: .utf8)
        }
    }
    #endif
    guard let url = Bundle.module.url(forResource: name, withExtension: "metal", subdirectory: "Shaders") else {
        throw FAHError.missingResource("Shaders/\(name).metal")
    }
    return try String(contentsOf: url, encoding: .utf8)
}
