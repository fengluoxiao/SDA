import Foundation

enum SdaError: LocalizedError {
 case message(String)
 var errorDescription: String? { if case let .message(s) = self { return s }; return nil }
}

@main struct ReaderProbe {
 static func main() throws {
  let fixture = URL(fileURLWithPath: CommandLine.arguments[1])
  let expected = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[2]))
  let input = try CompressedInput(url: fixture, name: fixture.lastPathComponent)
  var actual = Data(); var packets = 0
  while let packet = try input.next() { actual.append(packet); packets += 1 }
  guard actual == expected else { throw SdaError.message("Compressed MP4 changed bytes: \(actual.count) vs \(expected.count)") }
  print("PASS: \(packets) compressed packets, \(actual.count) bytes exactly match raw E-AC-3; no Apple downmix")
 }
}
