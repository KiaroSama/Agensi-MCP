import Darwin
import Foundation

// Raw unified-log records never leave this bounded, invocation-owned observer.
final class AuthorizationEvidence {
    private struct Parser {
        let path: String
        let pid: Int32
        var engine: String?
        var emitter: Int32?
        var emitterPath: String?
        var invalid = false
        var mechanism = false
        var uidMissing = false
        var credentialRejected = false
        var denied = false
        var succeeded = false
        var failed = false

        mutating func consume(_ data: Data) {
            guard data.count <= 32768, String(data: data, encoding: .utf8) != nil,
                  let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let message = object["eventMessage"] as? String,
                  let image = object["processImagePath"] as? String,
                  let origin = object["processIdentifier"] as? NSNumber,
                  origin.int64Value > 0, origin.int64Value <= Int64(Int32.max),
                  URL(fileURLWithPath: image).lastPathComponent == "authd" else {
                invalid = true
                return
            }
            if let emitter, emitter != origin.int32Value || emitterPath != image {
                invalid = true
                return
            }
            let client = NSRegularExpression.escapedPattern(for: path)
            let right = "com\\.apple\\.trust-settings\\.admin"
            let anchor = "^Process " + client + " \\(PID " + String(pid) +
                "\\) evaluates 1 rights with flags [0-9a-fA-F]{8} \\(engine ([0-9]+), token [0-9]+\\):\\s*\\(\\s*\"?" + right + "\"?\\s*\\)\\s*$"
            if message.range(of: anchor, options: .regularExpression) != nil {
                guard let expression = try? NSRegularExpression(pattern: anchor),
                      let result = expression.firstMatch(in: message, range: NSRange(message.startIndex..., in: message)),
                      let range = Range(result.range(at: 1), in: message), engine == nil else {
                    invalid = true
                    return
                }
                engine = String(message[range])
                emitter = origin.int32Value
                emitterPath = image
                return
            }
            guard let engine else { return }
            func matches(_ pattern: String) -> Bool {
                message.range(of: pattern, options: .regularExpression) != nil
            }
            mechanism = mechanism || matches("^engine " + engine + ": running mechanism [A-Za-z0-9:_,-]+ \\([0-9]+ of [0-9]+\\)$")
            uidMissing = uidMissing || message == "Mechanism failed to return a valid UID (engine " + engine + ")"
            credentialRejected = credentialRejected || matches("^User credential for rule failed \\(-?[0-9]+\\) \\(engine " + engine + "\\)$")
            denied = denied || message == "Evaluate denied (engine " + engine + ")" ||
                matches("^Evaluate cancelled or failed -?[0-9]+ \\(engine " + engine + "\\)$")
            let final = " right '" + right + "' by client '" + client + "' \\[" + String(pid) +
                "\\] for authorization created by '" + client + "' \\[" + String(pid) + "\\] \\([0-9A-Fa-f]+,[01]\\) "
            succeeded = succeeded || matches("^Succeeded authorizing" + final + "\\(engine " + engine + "\\)$")
            failed = failed || matches("^Failed to authorize" + final + "\\(-?[0-9]+\\) \\(engine " + engine + "\\)$")
        }

        var summary: String {
            guard !invalid, engine != nil else { return "Native authorization evidence unresolved." }
            return "Native authorization evidence anchor=true mechanism_started=" + String(mechanism) +
                " uid_missing=" + String(uidMissing) + " credential_rejected=" + String(credentialRejected) +
                " denied=" + String(denied) + " right_succeeded=" + String(succeeded) + " right_failed=" + String(failed)
        }
    }

    private let process = Process()
    private let pipe = Pipe()
    private let queue = DispatchQueue(label: "agensi.native-authorization-evidence")
    private let stopLock = NSLock()
    private var source: DispatchSourceRead?
    private var parser: Parser
    private var pending = Data()
    private var total = 0
    private var stopped = false
    private var started = false
    private var result: String?

    init() throws {
        parser = Parser(path: URL(fileURLWithPath: CommandLine.arguments[0]).standardizedFileURL.path, pid: getpid())
        try Self.selfCheck()
        let fd = pipe.fileHandleForReading.fileDescriptor
        let flags = fcntl(fd, F_GETFL)
        guard flags >= 0, fcntl(fd, F_SETFL, flags | O_NONBLOCK) == 0 else {
            throw FixtureFailure(stage: "authorization-evidence-pipe")
        }
        process.executableURL = URL(fileURLWithPath: "/usr/bin/log")
        process.arguments = ["stream", "--style", "ndjson", "--level", "info", "--timeout", "8", "--predicate", "process == \"authd\""]
        process.environment = ["PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "en_US.UTF-8"]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
    }

    func start() throws {
        stopLock.lock()
        defer { stopLock.unlock() }
        guard !stopped else { throw FixtureFailure(stage: "authorization-evidence-stopped") }
        try process.run()
        started = true
        try? pipe.fileHandleForWriting.close()
        let fd = pipe.fileHandleForReading.fileDescriptor
        let reader = DispatchSource.makeReadSource(fileDescriptor: fd, queue: queue)
        source = reader
        reader.setEventHandler { [weak self] in self?.drain() }
        reader.resume()
    }

    private func drain() {
        var bytes = [UInt8](repeating: 0, count: 4096)
        while total <= 262144 {
            let count = Darwin.read(pipe.fileHandleForReading.fileDescriptor, &bytes, bytes.count)
            if count == 0 { source?.cancel(); return }
            if count < 0 {
                if errno != EAGAIN && errno != EWOULDBLOCK { parser.invalid = true; source?.cancel() }
                return
            }
            total += count
            guard total <= 262144 else { parser.invalid = true; source?.cancel(); return }
            pending.append(contentsOf: bytes.prefix(count))
            while let end = pending.firstIndex(of: 10) {
                let line = Data(pending[..<end])
                pending.removeSubrange(...end)
                guard line.count <= 32768 else { parser.invalid = true; source?.cancel(); return }
                if !line.isEmpty { parser.consume(line) }
            }
            guard pending.count <= 32768 else { parser.invalid = true; source?.cancel(); return }
        }
    }

    func finish() -> (clean: Bool, summary: String) {
        stopLock.lock()
        defer { stopLock.unlock() }
        if stopped { return (result != nil, result ?? "Native authorization evidence cleanup failed.") }
        stopped = true
        guard started else {
            result = "Native authorization evidence not started."
            try? pipe.fileHandleForReading.close()
            try? pipe.fileHandleForWriting.close()
            return (true, result ?? "Native authorization evidence unresolved.")
        }
        if process.isRunning { _ = kill(process.processIdentifier, SIGKILL) }
        let deadline = ProcessInfo.processInfo.systemUptime + 0.2
        while process.isRunning && ProcessInfo.processInfo.systemUptime < deadline {
            Thread.sleep(forTimeInterval: 0.001)
        }
        guard !process.isRunning else { return (false, "Native authorization evidence cleanup failed.") }
        process.waitUntilExit()
        source?.cancel()
        queue.sync {
            drain()
            if !pending.isEmpty { parser.invalid = true }
            pending.removeAll(keepingCapacity: false)
            result = parser.summary
        }
        try? pipe.fileHandleForReading.close()
        return (true, result ?? "Native authorization evidence unresolved.")
    }

    private static func selfCheck() throws {
        func record(_ message: String, emitter: Int32 = 7) throws -> Data {
            try JSONSerialization.data(withJSONObject: ["processImagePath": "/usr/libexec/authd", "processIdentifier": emitter, "eventMessage": message])
        }
        let path = "/owned/mac-trust-00000000000000000000000000000000"
        let anchor = "Process " + path + " (PID 123) evaluates 1 rights with flags 00000003 (engine 42, token 8): (\n    \"com.apple.trust-settings.admin\"\n)"
        var valid = Parser(path: path, pid: 123)
        valid.consume(try record(anchor))
        valid.consume(try record("engine 42: running mechanism builtin:authenticate (1 of 2)"))
        valid.consume(try record("User credential for rule failed (-60005) (engine 42)"))
        guard valid.engine == "42", valid.mechanism, valid.credentialRejected, !valid.invalid else {
            throw FixtureFailure(stage: "authorization-evidence-parser")
        }
        for wrong in [anchor.replacingOccurrences(of: path, with: "/other"),
                      anchor.replacingOccurrences(of: "PID 123", with: "PID 124"),
                      anchor.replacingOccurrences(of: "trust-settings.admin", with: "trust-settings.user")] {
            var rejected = Parser(path: path, pid: 123)
            rejected.consume(try record(wrong))
            guard rejected.engine == nil else { throw FixtureFailure(stage: "authorization-evidence-parser") }
        }
        var other = Parser(path: path, pid: 123)
        other.consume(try record(anchor))
        other.consume(try record("Evaluate denied (engine 43)"))
        guard !other.denied else { throw FixtureFailure(stage: "authorization-evidence-parser") }
        other.consume(try record(anchor))
        guard other.invalid else { throw FixtureFailure(stage: "authorization-evidence-parser") }
        var restarted = Parser(path: path, pid: 123)
        restarted.consume(try record(anchor))
        restarted.consume(try record("Evaluate denied (engine 42)", emitter: 8))
        guard restarted.invalid, !restarted.denied else { throw FixtureFailure(stage: "authorization-evidence-parser") }
        var malformed = Parser(path: path, pid: 123)
        malformed.consume(Data("not-json".utf8))
        guard malformed.invalid else { throw FixtureFailure(stage: "authorization-evidence-parser") }
        var oversized = Parser(path: path, pid: 123)
        oversized.consume(Data(repeating: 32, count: 32769))
        guard oversized.invalid else { throw FixtureFailure(stage: "authorization-evidence-parser") }
    }
}
