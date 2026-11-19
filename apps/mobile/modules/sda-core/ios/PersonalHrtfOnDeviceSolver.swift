import Foundation
import UIKit

// CPU Mesh2HRTF inside the iOS application. No Python, Blender, shell, network,
// document export, or desktop helper. Input is a qualified local mesh project;
// raw scans must never be passed straight to the acoustic solver.
final class PersonalHrtfOnDeviceSolver {
 struct Output {
  let project: URL
  let frequencies: Int
  let peakTrackedArrayBytes: UInt64
  let numericalBudgetBytes: UInt64
 }
 private let lock = NSLock()
 private var handle: OpaquePointer?
 private var cancelled = false
 private var running = false
 private var generation = UUID()
 private var observers: [NSObjectProtocol] = [] // main thread only
 private var onProgress: ((Double) -> Void)? // worker thread only
 private var currentEar = 0
 private var lastProgress = -1.0

 func cancel() {
  lock.lock(); defer { lock.unlock() }
  cancelled = true
  if let handle { sda_hrtf_solver_cancel(handle) }
 }

 // The owner provides a dedicated temporary job directory and removes it only
 // AFTER completion. A partial/failing solve is never installed as a profile.
 func run(qualifiedProject: URL, frequencies: Int,
          progress: @escaping (Double) -> Void,
          completion: @escaping (Result<Output, Error>) -> Void) {
  dispatchPrecondition(condition: .onQueue(.main))
  guard (1...4096).contains(frequencies) else {
   completion(.failure(PersonalHrtfError.message("本地求解的频率数量无效"))); return
  }
  lock.lock()
  guard !running else {
   lock.unlock(); completion(.failure(PersonalHrtfError.message("已有个人 HRTF 计算正在进行"))); return
  }
  running = true; cancelled = false; generation = UUID(); let id = generation
  lock.unlock()
  observers = [UIApplication.didEnterBackgroundNotification, UIApplication.didReceiveMemoryWarningNotification].map { name in
   NotificationCenter.default.addObserver(forName:name,object:nil,queue:.main) { [weak self] _ in self?.cancel() }
  }
  let worker = Thread { [self] in
   autoreleasepool {
    let result: Result<Output, Error> = Result {
     // Budget is for all tracked numerical arrays; Foundation, input geometry,
     // audio playback and the application's other allocations also need room.
     let budget = sda_hrtf_recommended_budget(sda_hrtf_available_memory(),ProcessInfo.processInfo.physicalMemory)
     guard budget > 0 else {
      throw PersonalHrtfError.message("iPhone 当前可用内存不足以开始本机声学计算，请稍后重试；现有档案保持可用")
     }
     guard let native = qualifiedProject.path.withCString({ sda_hrtf_solver_create($0,budget) }) else {
      throw PersonalHrtfError.message("无法创建 iPhone 本地声学计算任务")
     }
     lock.lock(); handle = native; if cancelled { sda_hrtf_solver_cancel(native) }; lock.unlock()
     defer {
      lock.lock(); handle = nil; lock.unlock()
      sda_hrtf_solver_destroy(native)
      onProgress = nil
     }
     lastProgress = -1
     onProgress = { value in
      DispatchQueue.main.async { [weak self] in
       guard let self, self.generation == id else { return }; progress(value)
      }
     }
     let callback: SdaHrtfProgress = { context, current, total in
      guard let context, total > 0 else { return }
      let owner = Unmanaged<PersonalHrtfOnDeviceSolver>.fromOpaque(context).takeUnretainedValue()
      let value = (Double(owner.currentEar) + Double(current) / Double(total)) / 2
      if value - owner.lastProgress >= 0.002 || value >= 1 {
       owner.lastProgress = value; owner.onProgress?(min(1,max(0,value)))
      }
     }
     var peak: UInt64 = 0
     for ear in 0..<2 {
      currentEar = ear
      var reply = "NumCalc/source_\(ear + 1)".withCString { path in
       sda_hrtf_solver_run(native,path,1,Int32(frequencies),0,callback,Unmanaged.passUnretained(self).toOpaque())
      }
      guard reply.code == 0 else {
       let detail = withUnsafePointer(to:&reply.message) { pointer in
        pointer.withMemoryRebound(to:CChar.self,capacity:512) { String(cString:$0) }
       }
       if reply.code == 2 { throw PersonalHrtfError.message("已取消本地计算，未启用未完成的个人档案") }
       if reply.code == 3 { throw PersonalHrtfError.message("当前扫描超出 iPhone 本地计算的内存预算；未替换现有 SDA Spatial 档案") }
       throw PersonalHrtfError.message("本地声学计算未完成：\(detail)")
      }
      guard reply.remaining_array_bytes == 0 else { throw PersonalHrtfError.message("本地声学任务未正确释放资源") }
      peak = max(peak,reply.peak_array_bytes)
     }
     return Output(project:qualifiedProject,frequencies:frequencies,peakTrackedArrayBytes:peak,numericalBudgetBytes:budget)
    }
    DispatchQueue.main.async { [self] in
     for observer in observers { NotificationCenter.default.removeObserver(observer) }; observers.removeAll()
     lock.lock(); running = false; lock.unlock()
     completion(result)
    }
   }
  }
  worker.name = "SDA iOS personal HRTF"
  worker.qualityOfService = .utility
  worker.stackSize = 4 * 1024 * 1024
  worker.start()
 }
}
