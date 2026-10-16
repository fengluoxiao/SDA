// Synthetic Apple spatial-transfer measurement only. No music input or app changes.
// AVAudioEnvironmentNode HRTFHQ is not claimed equivalent to Apple Music,
// personalized Spatial Audio, AirPods DSP, or an Atmos bitstream decoder.
import Foundation
import AVFoundation

struct Direction: Codable {
    let id: Int
    let azimuth: Double
    let elevation: Double
}
struct ProbeFailure: Error, CustomStringConvertible {
    let description: String
    init(_ text: String) { description = text }
}
let sampleRate = 48_000.0
let firstImpulse = 8_192
let separation = 16_384
let totalFrames = firstImpulse + 2 * separation

func capture(_ direction: Direction, algorithm: AVAudio3DMixingRenderingAlgorithm,
             amplitude: Float) throws -> ([Float], [String: Any]) {
    let engine = AVAudioEngine()
    let environment = AVAudioEnvironmentNode()
    let player = AVAudioPlayerNode()
    let mono = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: 1)!
    let stereo = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: 2)!
    engine.attach(environment)
    engine.attach(player)
    engine.connect(player, to: environment, format: mono)
    engine.connect(environment, to: engine.mainMixerNode, format: stereo)
    engine.mainMixerNode.outputVolume = 1
    environment.outputVolume = 1
    environment.outputType = .headphones
    environment.listenerPosition = AVAudio3DPoint(x: 0, y: 0, z: 0)
    environment.listenerAngularOrientation = AVAudio3DAngularOrientation(yaw: 0, pitch: 0, roll: 0)
    environment.reverbParameters.enable = false
    // All sources are at 1.2m, inside the 2m attenuation reference distance.
    // Keep rolloff > 0 as documented; do not use invalid zero-rolloff settings.
    environment.distanceAttenuationParameters.distanceAttenuationModel = .inverse
    environment.distanceAttenuationParameters.referenceDistance = 2
    environment.distanceAttenuationParameters.maximumDistance = 100
    environment.distanceAttenuationParameters.rolloffFactor = 1
    player.renderingAlgorithm = algorithm
    player.sourceMode = .pointSource
    player.reverbBlend = 0
    player.obstruction = 0
    player.occlusion = 0
    player.volume = 1
    let az = direction.azimuth * .pi / 180
    let el = direction.elevation * .pi / 180
    // SDA positive azimuth=left, elevation=up. Apple x=right,y=up,z=back.
    player.position = AVAudio3DPoint(x: Float(-1.2 * sin(az) * cos(el)),
                                    y: Float(1.2 * sin(el)),
                                    z: Float(-1.2 * cos(az) * cos(el)))
    try engine.enableManualRenderingMode(.offline, format: stereo, maximumFrameCount: 1024)
    let input = AVAudioPCMBuffer(pcmFormat: mono, frameCapacity: AVAudioFrameCount(totalFrames))!
    input.frameLength = AVAudioFrameCount(totalFrames)
    let source = input.floatChannelData![0]
    for i in 0..<totalFrames { source[i] = 0 }
    source[firstImpulse] = amplitude
    source[firstImpulse + separation] = amplitude
    player.scheduleBuffer(input)
    engine.prepare()
    try engine.start()
    defer { player.stop(); engine.stop() }
    let applicable = environment.applicableRenderingAlgorithms.compactMap { ($0 as? NSNumber)?.intValue }
    if algorithm != .auto && !applicable.contains(algorithm.rawValue) {
        throw ProbeFailure("Requested algorithm \(algorithm.rawValue) unavailable: \(applicable)")
    }
    guard player.renderingAlgorithm == algorithm else { throw ProbeFailure("Algorithm readback mismatch") }
    player.play()
    let buffer = AVAudioPCMBuffer(pcmFormat: stereo, frameCapacity: 1024)!
    var output = [Float](repeating: 0, count: totalFrames * 2)
    var cursor = 0
    var retries = 0
    var statuses: [String: Int] = [:]
    while cursor < totalFrames {
        let count = AVAudioFrameCount(min(1024, totalFrames - cursor))
        let status = try engine.renderOffline(count, to: buffer)
        statuses[String(status.rawValue), default: 0] += 1
        switch status {
        case .success:
            guard buffer.frameLength == count else { throw ProbeFailure("Short manual-render block") }
            for i in 0..<Int(count) {
                for ear in 0..<2 {
                    let value = buffer.floatChannelData![ear][i]
                    guard value.isFinite else { throw ProbeFailure("Nonfinite rendered sample") }
                    output[(cursor + i) * 2 + ear] = value
                }
            }
            cursor += Int(count)
            retries = 0
        case .cannotDoInCurrentContext:
            retries += 1
            if retries > 1000 { throw ProbeFailure("Manual-render context never became ready") }
            Thread.sleep(forTimeInterval: 0.001)
        case .insufficientDataFromInputNode:
            throw ProbeFailure("Input underrun despite complete scheduled synthetic buffer")
        case .error:
            throw ProbeFailure("Manual rendering returned error")
        @unknown default:
            throw ProbeFailure("Unknown manual-render status")
        }
    }
    let first = Array(output[(firstImpulse * 2)..<((firstImpulse + separation) * 2)])
    let second = Array(output[((firstImpulse + separation) * 2)..<(totalFrames * 2)])
    var energy = 0.0, error = 0.0, maxError = 0.0, tail = 0.0
    var earEnergy = [0.0, 0.0], peaks = [0.0, 0.0]
    for i in first.indices {
        let a = Double(first[i]), b = Double(second[i]), delta = a - b
        energy += a * a; error += delta * delta; maxError = max(maxError, abs(delta))
        earEnergy[i % 2] += a * a; peaks[i % 2] = max(peaks[i % 2], abs(a))
        if i >= first.count - 4096 { tail += a * a }
    }
    guard energy > 1e-16 else { throw ProbeFailure("Silent spatial response") }
    let repeatDb = 10 * log10(max(error / energy, 1e-30))
    let tailDb = 10 * log10(max(tail / energy, 1e-30))
    let kernel = first.map { $0 / amplitude }
    return (kernel, ["id": direction.id, "azimuth": direction.azimuth,
                     "elevation": direction.elevation, "algorithmRequested": algorithm.rawValue,
                     "algorithmReadback": player.renderingAlgorithm.rawValue,
                     "applicableAlgorithms": applicable, "inputAmplitude": amplitude,
                     "repeatResidualDb": repeatDb, "repeatMaximumSampleError": maxError,
                     "tailEnergyDb": tailDb, "earEnergy": earEnergy, "earPeaks": peaks,
                     "renderStatuses": statuses,
                     "applePosition": [player.position.x, player.position.y, player.position.z]])
}

func main() throws {
    guard CommandLine.arguments.count == 3 else { throw ProbeFailure("Usage: probe directions.json output-directory") }
    let input = URL(fileURLWithPath: CommandLine.arguments[1])
    let destination = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
    try FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true)
    let directions = try JSONDecoder().decode([Direction].self, from: Data(contentsOf: input))
    guard !directions.isEmpty, Set(directions.map { $0.id }).count == directions.count,
          directions.allSatisfy({ $0.azimuth.isFinite && $0.elevation.isFinite && abs($0.elevation) <= 90 })
    else { throw ProbeFailure("Invalid or duplicate direction list") }
    var rows: [[String: Any]] = []
    var checks: [[String: Any]] = []
    var firstKernels: [Int: [Float]] = [:]
    let calibration = Set(directions.filter { abs($0.azimuth) == 90 && $0.elevation == 45 || $0.azimuth == 0 && $0.elevation == 0 }.map { $0.id })
    for (i, direction) in directions.enumerated() {
        let (kernel, data) = try autoreleasepool { try capture(direction, algorithm: .HRTFHQ, amplitude: 0.1) }
        let name = String(format: "direction-%03d.f32", direction.id)
        let bytes = kernel.withUnsafeBufferPointer { Data(buffer: $0) }
        try bytes.write(to: destination.appendingPathComponent(name))
        var row = data; row["file"] = name; rows.append(row)
        if calibration.contains(direction.id) { firstKernels[direction.id] = kernel }
        if i % 32 == 0 { print("Captured \(i + 1)/\(directions.count) HRTFHQ directions") }
    }
    for direction in directions where calibration.contains(direction.id) {
        let base = firstKernels[direction.id]!
        for amplitude in [Float(0.05), Float(0.2)] {
            let (kernel, data) = try autoreleasepool { try capture(direction, algorithm: .HRTFHQ, amplitude: amplitude) }
            var energy = 0.0, error = 0.0, maximum = 0.0
            for i in base.indices {
                let d = Double(kernel[i] - base[i]); error += d * d
                energy += Double(base[i]) * Double(base[i]); maximum = max(maximum, abs(d))
            }
            var row = data
            row["normalizedLinearityResidualDb"] = 10 * log10(max(error / energy, 1e-30))
            row["normalizedLinearityMaximumError"] = maximum
            checks.append(row)
        }
    }
    #if arch(arm64)
    let architecture = "arm64"
    #else
    let architecture = "x86_64"
    #endif
    let report: [String: Any] = [
        "scope": "Synthetic AVAudioEnvironmentNode HRTFHQ transfer measurement. Not Apple Music, personalized Spatial Audio, AirPods, or an Atmos decode reference.",
        "os": ProcessInfo.processInfo.operatingSystemVersionString, "architecture": architecture,
        "sampleRate": sampleRate, "kernelFrames": separation, "format": "f32le interleaved L,R; divided only by known input impulse amplitude",
        "sourceRadiusMetres": 1.2, "distanceReferenceMetres": 2, "distanceRolloff": 1,
        "reverbEnabled": false, "outputType": "headphones", "headPose": "identity",
        "impulseSamples": [firstImpulse, firstImpulse + separation],
        "directions": rows, "linearityChecks": checks,
        "stationarityPassed": rows.allSatisfy { ($0["repeatResidualDb"] as! Double) < -60 },
        "linearityPassed": checks.allSatisfy { ($0["normalizedLinearityResidualDb"] as! Double) < -60 }
    ]
    let encoded = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
    try encoded.write(to: destination.appendingPathComponent("report.json"))
    guard (report["stationarityPassed"] as! Bool) && (report["linearityPassed"] as! Bool) else {
        throw ProbeFailure("Responses captured, but LTI validation failed; do not use as convolution kernels")
    }
    print("Captured and validated \(rows.count) directions; \(checks.count) linearity checks")
}
do { try main() }
catch { fputs("Apple spatial reference failed: \(error)\n", stderr); exit(1) }
