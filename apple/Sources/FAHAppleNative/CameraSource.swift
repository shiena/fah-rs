import AVFoundation
import CoreVideo

/// Owns camera capture only; processing is injected by the host.
public final class CameraSource: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    private let capture = AVCaptureSession()
    private let queue = DispatchQueue(label: "FAH.camera", qos: .userInitiated)
    private var handler: ((CVPixelBuffer) -> Void)?
    public let name: String

    public static func devices() -> [AVCaptureDevice] {
        #if os(macOS)
        return AVCaptureDevice.DiscoverySession(deviceTypes: [.builtInWideAngleCamera, .external, .continuityCamera], mediaType: .video, position: .unspecified).devices
        #else
        return AVCaptureDevice.DiscoverySession(deviceTypes: [.builtInWideAngleCamera], mediaType: .video, position: .unspecified).devices
        #endif
    }
    public static func requestAccess(_ completion: @escaping (Bool) -> Void) {
        AVCaptureDevice.requestAccess(for: .video, completionHandler: completion)
    }
    public init(device: AVCaptureDevice? = nil) throws {
        guard let device = device ?? AVCaptureDevice.default(for: .video) else { throw FAHAppleError.message("No camera found") }
        name = device.localizedName
        super.init()
        capture.beginConfiguration()
        defer { capture.commitConfiguration() }
        if capture.canSetSessionPreset(.vga640x480) { capture.sessionPreset = .vga640x480 }
        let input = try AVCaptureDeviceInput(device: device)
        guard capture.canAddInput(input) else { throw FAHAppleError.message("Cannot add camera input") }
        capture.addInput(input)
        let output = AVCaptureVideoDataOutput()
        output.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
        output.alwaysDiscardsLateVideoFrames = true
        guard capture.canAddOutput(output) else { throw FAHAppleError.message("Cannot add camera output") }
        output.setSampleBufferDelegate(self, queue: queue)
        capture.addOutput(output)
    }
    public func start(_ handler: @escaping (CVPixelBuffer) -> Void) {
        queue.async { self.handler = handler; self.capture.startRunning() }
    }
    public func stop() {
        queue.async { self.handler = nil; self.capture.stopRunning() }
    }
    public func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        guard let buffer = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        handler?(buffer)
    }
}
