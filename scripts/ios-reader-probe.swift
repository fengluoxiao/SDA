import Foundation
import AVFoundation

enum SdaError: LocalizedError {
 case message(String)
 var errorDescription: String? { if case let .message(s) = self { return s }; return nil }
}

@main struct ReaderProbe {
 static func main() throws {
  let fixture = URL(fileURLWithPath: CommandLine.arguments[1])
  let expected = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[2]))
  precondition(CompressedInput.isRecoverableReaderError(NSError(domain:AVFoundationErrorDomain,code:-11880)))
  precondition(CompressedInput.isRecoverableReaderError(NSError(domain:AVFoundationErrorDomain,code:-11847)))
  precondition(!CompressedInput.isRecoverableReaderError(NSError(domain:AVFoundationErrorDomain,code:-11828)))
  precondition(!CompressedInput.isRecoverableReaderError(NSError(domain:NSOSStatusErrorDomain,code:-12551)))
  let input = try CompressedInput(url: fixture, name: fixture.lastPathComponent)
  var actual = Data(); var packets = 0
  while let packet = try input.next() { actual.append(packet); packets += 1 }
  guard actual == expected else { throw SdaError.message("Compressed MP4 changed bytes: \(actual.count) vs \(expected.count)") }
  let resumed = try CompressedInput(url: fixture, name: fixture.lastPathComponent)
  var recovered = Data()
  for _ in 0..<3 { if let packet = try resumed.next() { recovered.append(packet) } }
  try resumed.reopenAfterInterruption()
  while let packet = try resumed.next() { recovered.append(packet) }
  guard recovered == expected else { throw SdaError.message("Resumed compressed cursor dropped or duplicated packets") }
  print("PASS: \(packets) compressed packets, \(actual.count) bytes exactly match raw E-AC-3; no Apple downmix")
 }
}
