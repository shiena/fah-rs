import Foundation

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
    guard let url = Bundle.module.url(forResource: name, withExtension: "metal", subdirectory: "Shaders") else {
        throw FAHError.missingResource("Shaders/\(name).metal")
    }
    return try String(contentsOf: url, encoding: .utf8)
}
