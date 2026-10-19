import AppKit
import Darwin
import Foundation
import Security

func promptFrames(_ data: Data) throws -> (boundary: String, code: String, focus: String, timing: String, cancellation: String, noPrompt: Bool) {
    guard data.count < 64, let text = String(data: data, encoding: .utf8),
          text.isEmpty || text.hasSuffix("\n") else { throw FixtureFailure(stage: "prompt-tail-protocol") }
    if text.contains("N") {
        guard text == "N1\nE4:69\n" else { throw FixtureFailure(stage: "prompt-tail-protocol") }
        return ("none", "E4:69", "none", "none", "none", true)
    }
    let lines = text.split(separator: "\n", omittingEmptySubsequences: false).dropLast()
    guard lines.allSatisfy({ $0.range(of: "^(B[1-8]|C[01]|E[1-4]:-?[0-9]{1,7}|D[0-7]:[0-9]{1,3}|T[0-9]{1,4}(:[0-9]{1,4}){8})$",
        options: .regularExpression) != nil }) else { throw FixtureFailure(stage: "prompt-tail-protocol") }
    let cancellations = lines.filter { $0.hasPrefix("C") }
    guard cancellations.count <= 1 else { throw FixtureFailure(stage: "prompt-tail-protocol") }
    if !cancellations.isEmpty {
        let exits = lines.filter { $0.hasPrefix("E") }
        guard exits.count == 1, let last = lines.last, last == exits[0],
              last.hasPrefix("E4:"), let code = Int(last.dropFirst(3)), code != 0 else { throw FixtureFailure(stage: "prompt-tail-protocol") }
    }
    let diagnostics = lines.filter { $0.hasPrefix("D") }
    guard diagnostics.count <= 1 else { throw FixtureFailure(stage: "prompt-tail-protocol") }
    if let frame = diagnostics.first {
        let parts = frame.dropFirst().split(separator: ":")
        guard let snapshot = Int(parts[1]), snapshot <= 511 else { throw FixtureFailure(stage: "prompt-tail-protocol") }
    }
    let timings = lines.filter { $0.hasPrefix("T") }
    guard timings.count <= 1 else { throw FixtureFailure(stage: "prompt-tail-protocol") }
    if let frame = timings.first {
        let parts = frame.dropFirst().split(separator: ":")
        guard let count = Int(parts[3]), count <= 999,
              !lines.contains(where: { $0.hasPrefix("B") || $0.hasPrefix("D") || $0.hasPrefix("C") }) else {
            throw FixtureFailure(stage: "prompt-tail-protocol")
        }
    }
    return (lines.last(where: { $0.hasPrefix("B") }).map(String.init) ?? "none",
        lines.last(where: { $0.hasPrefix("E") }).map(String.init) ?? "none",
        diagnostics.first.map(String.init) ?? "none", timings.first.map(String.init) ?? "none",
        cancellations.first.map(String.init) ?? "none", false)
}

func checkPromptFrames() throws {
    let positive = try promptFrames(Data("B8\nC1\nE4:108\n".utf8))
    guard positive.cancellation == "C1", positive.code == "E4:108",
          try promptFrames(Data("N1\nE4:69\n".utf8)).noPrompt else {
        throw FixtureFailure(stage: "prompt-parser-selftest")
    }
    for value in ["C2\n", "C1\nC0\n", "C1", "C1\n", "C1\nE4:0\n", "C1\nE4:00\n", "C1\nE4:-0\n", "C1\nE3:65\n",
        "E4:108\nC1\n", "C1\nE4:108\nE4:108\n", "C1\nT0:0:0:0:0:0:0:0:0\n", String(repeating: "C0\n", count: 22),
        "N0\nE4:69\n", "N1\nE4:0\n", "N1\n", "N1\nE4:69\nC1\n", "B8\nN1\nE4:69\n", "N1\nE4:69\nN1\n"] {
        do {
            _ = try promptFrames(Data(value.utf8))
        } catch { continue }
        throw FixtureFailure(stage: "prompt-parser-negative")
    }
}

extension Fixture {
    func withPrompt(_ identity: AccountIdentity, password: String, inputFixture: Bool = false,
                    observeAuthorization: Bool = false, cancelOnly: Bool = false,
                    allowNoPrompt: Bool = false, operation: () throws -> Void) throws {
        guard !allowNoPrompt || (!inputFixture && !cancelOnly && !observeAuthorization) else {
            throw FixtureFailure(stage: "prompt-mode")
        }
        let marker = URL(fileURLWithPath: CommandLine.arguments[0]).lastPathComponent
        guard marker.hasPrefix("mac-trust-"), marker.count == 42,
              marker.dropFirst(10).allSatisfy({ $0.isHexDigit }) else { throw FixtureFailure(stage: "prompt-marker") }
        let script = state.deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent(inputFixture ? "scripts/mac-input-fixture.js" : "scripts/mac-trust-ui.js")
        guard script.resolvingSymlinksInPath() == script else { throw FixtureFailure(stage: "prompt-script") }
        let evidence = observeAuthorization ? try AuthorizationEvidence() : nil
        defer {
            if let evidence {
                let observation = evidence.finish()
                event(observation.clean ? "INFO" : "ERROR", observation.summary)
                if !observation.clean { _exit(1) }
            }
        }
        try evidence?.start()
        let worker = Process()
        let input = Pipe()
        let output = Pipe()
        worker.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        worker.arguments = ["-l", "JavaScript", script.path]
        worker.standardInput = input
        worker.standardOutput = output
        worker.standardError = FileHandle.nullDevice
        worker.environment = ["PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "en_US.UTF-8"]
        try worker.run()
        let deadline = DispatchSource.makeTimerSource(queue: .global())
        deadline.schedule(deadline: .now() + 7)
        deadline.setEventHandler {
            let killed = worker.isRunning
            if killed { kill(worker.processIdentifier, SIGKILL); worker.waitUntilExit() }
            let tail = try? output.fileHandleForReading.read(upToCount: 64)
            var diagnostic = "invalid"
            if let tail, let frames = try? promptFrames(tail) {
                diagnostic = "boundary=" + frames.boundary + " worker=" + frames.code + " focus=" + frames.focus + " timing=" + frames.timing
            }
            if let evidence {
                let observation = evidence.finish()
                self.event(observation.clean ? "INFO" : "ERROR", observation.summary)
            }
            self.event("ERROR", "Native prompt deadline " + diagnostic +
                " reason=" + String(worker.terminationReason.rawValue) +
                " status=" + String(worker.terminationStatus) + " watchdog_killed=" + String(killed))
            _exit(1)
        }
        deadline.resume()
        defer {
            deadline.cancel()
            if worker.isRunning { kill(worker.processIdentifier, SIGKILL); worker.waitUntilExit() }
            try? input.fileHandleForWriting.close()
            try? output.fileHandleForReading.close()
        }
        var values: [String: Any] = ["marker": marker, "name": identity.name, "password": password]
        if inputFixture { values["fixturePID"] = Int(getpid()) }
        else { values["cancelOnly"] = cancelOnly; values["allowNoPrompt"] = allowNoPrompt }
        let payload = try JSONSerialization.data(withJSONObject: values)
        try input.fileHandleForWriting.write(contentsOf: payload)
        try input.fileHandleForWriting.close()
        let readyDeadline = Date().addingTimeInterval(2)
        var handshake = Data()
        while handshake.count < 32 && !handshake.contains(10) {
            let remaining = max(0, Int32(readyDeadline.timeIntervalSinceNow * 1000))
            var ready = pollfd(fd: output.fileHandleForReading.fileDescriptor, events: Int16(POLLIN), revents: 0)
            guard remaining > 0, poll(&ready, 1, remaining) == 1 else {
                throw FixtureFailure(stage: "prompt-readiness-timeout")
            }
            guard let byte = try output.fileHandleForReading.read(upToCount: 1), !byte.isEmpty else { break }
            handshake.append(byte)
        }
        guard handshake == Data([82, 10]) else {
            if let text = String(data: handshake, encoding: .utf8),
               text.trimmingCharacters(in: .newlines).range(of: "^E[1-4]:-?[0-9]{1,7}$", options: .regularExpression) != nil {
                throw FixtureFailure(stage: "prompt-readiness-" + text.trimmingCharacters(in: .newlines))
            }
            throw FixtureFailure(stage: "prompt-readiness-protocol")
        }
        event("INFO", "Owned prompt worker ready with existing consent; exact native operation starting.")
        var operationError: Error?
        do { try operation() } catch {
            operationError = error
            if inputFixture {
                let stage = (error as? FixtureFailure)?.stage ?? "native-operation"
                let allowed = ["input-fixture-window-count", "input-fixture-submission-timeout",
                    "input-fixture-value-mismatch", "input-fixture-main-thread", "input-fixture-content",
                    "input-fixture-focus", "input-fixture-unexpected-window"]
                event("ERROR", "Owned input operation failed stage=" + (allowed.contains(stage) ? stage : "native-operation"))
            }
        }
        event("INFO", "Native operation returned; awaiting owned worker exit.")
        if inputFixture {
            let application = NSApplication.shared
            while worker.isRunning {
                if let event = application.nextEvent(matching: .any, until: Date().addingTimeInterval(0.01),
                    inMode: .default, dequeue: true) {
                    application.sendEvent(event)
                }
                application.updateWindows()
            }
        }
        worker.waitUntilExit()
        event("INFO", "Owned prompt worker reaped.")
        let frames = try promptFrames(try output.fileHandleForReading.read(upToCount: 64) ?? Data())
        if frames.focus != "none" { event("INFO", "Owned focus diagnostic=" + frames.focus) }
        if frames.timing != "none" { event("INFO", "Owned timing diagnostic=" + frames.timing) }
        if frames.code != "none" { event("INFO", "Owned worker diagnostic=" + frames.code) }
        if inputFixture && frames.cancellation != "none" { throw FixtureFailure(stage: "prompt-tail-protocol") }
        if frames.cancellation != "none" { event("INFO", "Owned cancellation diagnostic=" + frames.cancellation) }
        if frames.noPrompt {
            guard allowNoPrompt, worker.terminationStatus == 69, operationError == nil else {
                throw FixtureFailure(stage: "prompt-noninteractive-result")
            }
            event("INFO", "Native trust operation completed without a validated input target; fresh trust absence still required.")
            return
        }
        if cancelOnly {
            guard worker.terminationStatus == 108, frames.code == "E4:108", frames.cancellation == "C1" else {
                throw FixtureFailure(stage: "prompt-cancellation-worker")
            }
        } else if worker.terminationStatus != 0 {
            throw FixtureFailure(stage: "prompt-worker:" + String(worker.terminationStatus))
        }
        if let operationError { throw operationError }
        if cancelOnly {
            event("INFO", "Exact owned prompt canceled; native cancellation returned and worker reaped.")
            return
        }
        event("INFO", inputFixture ? "Owned public-input fixture verified and worker reaped; no authorization tested." :
            "Exact native prompt authenticated and owned worker reaped.")
    }
    func verifyPrompt(expectedCancellation: Bool = false) throws {
        var authorization: AuthorizationRef?
        try Self.check(AuthorizationCreate(nil, nil, [], &authorization), "authorization-create")
        guard let authorization else { throw FixtureFailure(stage: "authorization-reference") }
        let marker = URL(fileURLWithPath: CommandLine.arguments[0]).lastPathComponent
        event("INFO", "Native authorization request starting.")
        let status = marker.withCString { prompt in
            "prompt".withCString { promptName in
                "com.apple.trust-settings.admin".withCString { right in
                    var item = AuthorizationItem(name: right, valueLength: 0, value: nil, flags: 0)
                    var context = AuthorizationItem(name: promptName, valueLength: strlen(prompt), value: UnsafeMutableRawPointer(mutating: prompt), flags: 0)
                    return withUnsafeMutablePointer(to: &item) { pointer in
                        var rights = AuthorizationRights(count: 1, items: pointer)
                        return withUnsafeMutablePointer(to: &context) { contextPointer in
                            var environment = AuthorizationEnvironment(count: 1, items: contextPointer)
                            return AuthorizationCopyRights(authorization, &rights, &environment, [.extendRights, .interactionAllowed], nil)
                        }
                    }
                }
            }
        }
        event("INFO", "Native authorization request returned.")
        event("INFO", "Owned authorization destruction starting.")
        let cleanup = AuthorizationFree(authorization, [.destroyRights])
        event("INFO", "Owned authorization destruction returned.")
        try Self.check(cleanup, "authorization-destroy")
        if expectedCancellation {
            guard status == errAuthorizationCanceled else { throw FixtureFailure(stage: "authorization-cancel-result") }
        } else { try Self.check(status, "authorization-native-prompt") }
    }
}
