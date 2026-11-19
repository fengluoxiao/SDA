import Foundation
import simd

// Deliberately synthetic, disconnected surfaces for landmark/unit tests only.
// This is not a valid head mesh, scan fixture or installable response.
enum ReviewFixture {
 static let vertices: [Float] = [
  -0.002,0.08,-0.002, -0.002,0.08,0.002, 0.002,0.08,0.002, 0.002,0.08,-0.002,
  -0.002,-0.08,-0.002, -0.002,-0.08,0.002, 0.002,-0.08,0.002, 0.002,-0.08,-0.002,
  0.1,-0.002,-0.002, 0.1,0.002,-0.002, 0.1,0,0.002,
  -0.002,-0.002,0.1, 0.002,-0.002,0.1, 0,0.002,0.1]
 static let triangles: [UInt32] = [0,1,2, 0,2,3, 4,6,5, 4,7,6, 8,9,10, 11,12,13]
 static func surface() throws -> PersonalHrtfReviewGeometry { try .init(vertices:vertices,triangles:triangles) }
 static func require(_ yes: Bool,_ message: String) throws { if !yes { throw NSError(domain:message,code:1) } }
 static func reject(_ operation: () throws -> Void) throws {
  do { try operation() } catch { return }; throw NSError(domain:"Expected invalid landmark rejection",code:1)
 }
}
