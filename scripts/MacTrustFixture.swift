import AppKit
import Darwin
import Darwin.membership
import Foundation
import OpenDirectory
import Security

struct FixtureFailure: Error { let stage: String }
struct AccountIdentity: Codable {
    let name: String
    let uid: String
    let guid: String
}

final class InputFixture: NSObject {
    private let username = NSTextField(frame: NSRect(x: 24, y: 112, width: 472, height: 24))
    private let secure = NSSecureTextField(frame: NSRect(x: 24, y: 72, width: 472, height: 24))
    private let expectedName: String
    private let expectedPassword: String
    private var window: NSWindow?
    private var submitted = false
    private var usernameMatch = false
    private var passwordMatch = false
    private var wrongControlRejected = false

    init(name: String, password: String) {
        expectedName = name
        expectedPassword = password
        super.init()
    }

    func prepare() throws {
        guard Thread.isMainThread else { throw FixtureFailure(stage: "input-fixture-main-thread") }
        let application = NSApplication.shared
        guard application.setActivationPolicy(.regular) else { throw FixtureFailure(stage: "input-fixture-activation") }
        application.finishLaunching()
        guard application.windows.isEmpty else { throw FixtureFailure(stage: "input-fixture-window-baseline") }
    }

    @objc private func submit(_ sender: Any?) {
        guard !submitted else { return }
        username.validateEditing()
        secure.validateEditing()
        usernameMatch = username.stringValue == expectedName
        passwordMatch = secure.stringValue == expectedPassword
        wrongControlRejected = secure.stringValue != String(repeating: "f", count: 64)
        submitted = true
        window?.close()
    }

    private func focusState(_ phase: String, report: (String) -> Void) {
        report("Owned input focus phase=" + phase +
            " app_active=" + String(NSApplication.shared.isActive) +
            " window_key=" + String(window?.isKeyWindow ?? false) +
            " window_main=" + String(window?.isMainWindow ?? false) +
            " can_become_key=" + String(window?.canBecomeKey ?? false) +
            " username_accepts_first_responder=" + String(username.acceptsFirstResponder) +
            " secure_accepts_first_responder=" + String(secure.acceptsFirstResponder) +
            " username_editor_present=" + String(username.currentEditor() != nil) +
            " secure_editor_present=" + String(secure.currentEditor() != nil))
    }

    func run(marker: String, report: (String) -> Void) throws {
        let setupStart = ProcessInfo.processInfo.systemUptime
        var loopStart: TimeInterval?
        defer {
            let now = ProcessInfo.processInfo.systemUptime
            let setup = ((loopStart ?? now) - setupStart) * 1000
            let loop = loopStart.map { (now - $0) * 1000 } ?? 0
            if setup.isFinite && loop.isFinite && setup >= 0 && loop >= 0 {
                report("Owned input timing setup_ms=" + String(Int(min(9999, setup))) +
                    " loop_ms=" + String(Int(min(9999, loop))))
            } else { report("Owned input timing invalid") }
            focusState("final", report: report)
            let editor = secure.currentEditor()
            let received = editor?.string ?? secure.stringValue
            let length = received.count
            let category = length == 0 ? "zero" : (length < 64 ? "partial" : (length == 64 ? "exact" : "excess"))
            let editorFocused = editor.map { window?.firstResponder === $0 } ?? false
            report("Owned input fixture submitted=" + String(submitted) +
                " username_match=" + String(username.stringValue == expectedName) +
                " committed_password_match=" + String(secure.stringValue == expectedPassword) +
                " editor_password_match=" + String(editor?.string == expectedPassword) +
                " wrong_control_rejected=" + String(received != String(repeating: "f", count: 64)) +
                " secure_editor_focused=" + String(editorFocused) + " received_length=" + category)
            window?.close()
            window = nil
            username.stringValue = ""
            secure.stringValue = ""
        }
        guard Thread.isMainThread else { throw FixtureFailure(stage: "input-fixture-main-thread") }
        let application = NSApplication.shared
        let ownedWindow = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 520, height: 210),
            styleMask: [.titled], backing: .buffered, defer: false)
        window = ownedWindow
        ownedWindow.isReleasedWhenClosed = false
        ownedWindow.title = marker
        guard let content = ownedWindow.contentView else { throw FixtureFailure(stage: "input-fixture-content") }
        let label = NSTextField(labelWithString: marker)
        label.frame = NSRect(x: 24, y: 158, width: 472, height: 28)
        let button = NSButton(title: "OK", target: self, action: #selector(submit(_:)))
        button.frame = NSRect(x: 408, y: 24, width: 88, height: 28)
        button.keyEquivalent = "\r"
        content.addSubview(label)
        content.addSubview(username)
        content.addSubview(secure)
        content.addSubview(button)
        username.nextKeyView = secure
        secure.nextKeyView = button
        ownedWindow.initialFirstResponder = username
        ownedWindow.defaultButtonCell = button.cell as? NSButtonCell
        ownedWindow.center()
        ownedWindow.makeKeyAndOrderFront(nil)
        application.activate(ignoringOtherApps: true)
        guard ownedWindow.makeFirstResponder(username) else { throw FixtureFailure(stage: "input-fixture-focus") }
        focusState("initial", report: report)
        let deadline = Date().addingTimeInterval(4)
        loopStart = ProcessInfo.processInfo.systemUptime
        var reportedExtraWindows = false
        while !submitted && Date() < deadline {
            let until = min(deadline, Date().addingTimeInterval(0.01))
            if let event = application.nextEvent(matching: .any, until: until, inMode: .default, dequeue: true) {
                application.sendEvent(event)
            }
            application.updateWindows()
            if !reportedExtraWindows {
                let windows = application.windows
                guard windows.count <= 160 else { throw FixtureFailure(stage: "input-fixture-window-count") }
                let extra = windows.filter { $0 !== ownedWindow }
                if !extra.isEmpty {
                    reportedExtraWindows = true
                    report("Owned input extra windows count=" + String(extra.count) +
                        " overflow=" + String(max(0, extra.count - 4)))
                    let framework = Bundle(for: NSWindow.self)
                    for observed in extra.prefix(4) {
                        let bundle = Bundle(for: type(of: observed))
                        let appKit = bundle == framework
                        let location = bundle.bundleURL.standardizedFileURL.resolvingSymlinksInPath()
                        let systemFramework = bundle.bundleURL.isFileURL &&
                            (location.path.hasPrefix("/System/Library/Frameworks/") ||
                             location.path.hasPrefix("/System/Library/PrivateFrameworks/")) &&
                            (bundle.bundleIdentifier?.hasPrefix("com.apple.") ?? false)
                        let name = appKit || systemFramework ? NSStringFromClass(type(of: observed)) : "other"
                        let valid = name.range(of: "^[A-Za-z_][A-Za-z0-9_.]{0,127}$", options: .regularExpression) != nil
                        report("Owned input extra window class=" + (valid ? name : "other") +
                            " appkit=" + String(appKit) + " system_framework=" + String(systemFramework) +
                            " panel=" + String(observed is NSPanel) +
                            " visible=" + String(observed.isVisible) + " key=" + String(observed.isKeyWindow) +
                            " owned=" + String(observed === ownedWindow))
                    }
                }
            }
        }
        guard submitted else { throw FixtureFailure(stage: "input-fixture-submission-timeout") }
        guard usernameMatch && passwordMatch && wrongControlRejected else {
            throw FixtureFailure(stage: "input-fixture-value-mismatch")
        }
    }
}

final class Fixture {
    let state: URL
    let node: ODNode
    let admin: ODRecord
    let journal: URL
    let log: FileHandle
    let formatter = DateFormatter()

    init(statePath: String) throws {
        let env = ProcessInfo.processInfo.environment
        guard getuid() == 0, env["GITHUB_ACTIONS"] == "true",
              env["RUNNER_ENVIRONMENT"] == "github-hosted",
              let workspace = env["GITHUB_WORKSPACE"] else { throw FixtureFailure(stage: "host-guard") }
        try checkPromptFrames()
        let root = URL(fileURLWithPath: workspace).standardizedFileURL
        state = URL(fileURLWithPath: statePath).standardizedFileURL
        let stateName = state.lastPathComponent
        guard state.deletingLastPathComponent() == root.appendingPathComponent(".tmp"),
              stateName.hasPrefix("tls-ci-"), stateName.count == 39,
              stateName.dropFirst(7).allSatisfy({ $0.isHexDigit }),
              state.resolvingSymlinksInPath().path == state.path else { throw FixtureFailure(stage: "state-guard") }
        let stateAttributes = try FileManager.default.attributesOfItem(atPath: state.path)
        guard stateAttributes[.type] as? FileAttributeType == .typeDirectory,
              (stateAttributes[.ownerAccountID] as? NSNumber)?.intValue == 0 else {
            throw FixtureFailure(stage: "state-owner")
        }
        journal = state.appendingPathComponent("mac-trust-account.json")
        formatter.dateFormat = "yyyy-MM-dd HH:mm:ss 'UTC'"
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        let filename = "mac-trust-fixture_" + UUID().uuidString + ".log"
        let fd = open(state.appendingPathComponent(filename).path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
        guard fd >= 0 else { throw FixtureFailure(stage: "log-create") }
        log = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        node = try ODNode(session: ODSession.default(), type: UInt32(kODNodeTypeLocalNodes))
        admin = try node.record(withRecordType: kODRecordTypeGroups, name: "admin", attributes: kODAttributeTypeAllAttributes)
        event("INFO", "Native fixture ownership and account recovery ready.")
    }

    static func check(_ status: OSStatus, _ stage: String) throws {
        guard status == errSecSuccess else { throw FixtureFailure(stage: stage + ":" + String(status)) }
    }
    func event(_ level: String, _ message: String) {
        let text = "[\(formatter.string(from: Date()))] [\(level)] [mac-trust-fixture] \(message)\n"
        let data = Data(text.utf8)
        try? log.write(contentsOf: data)
        try? FileHandle.standardError.write(contentsOf: data)
    }
    func records(attribute: String, value: String) throws -> [ODRecord] {
        let query = try ODQuery(node: node, forRecordTypes: kODRecordTypeUsers,
            attribute: attribute, matchType: ODMatchType(kODMatchEqualTo), queryValues: value,
            returnAttributes: kODAttributeTypeAllAttributes, maximumResults: 2)
        guard let result = try query.resultsAllowingPartial(false) as? [ODRecord] else {
            throw FixtureFailure(stage: "account-query")
        }
        return result
    }
    func values(_ record: ODRecord, _ attribute: String) throws -> [String] {
        try record.values(forAttribute: attribute) as? [String] ?? []
    }
    func save(_ identity: AccountIdentity) throws {
        let data = try JSONEncoder().encode(identity)
        let fd = open(journal.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
        guard fd >= 0 else { throw FixtureFailure(stage: "journal-create") }
        let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        try handle.write(contentsOf: data)
        try handle.synchronize()
        try handle.close()
    }
    func recover() throws {
        guard FileManager.default.fileExists(atPath: journal.path) else { return }
        let attrs = try FileManager.default.attributesOfItem(atPath: journal.path)
        guard attrs[.type] as? FileAttributeType == .typeRegular,
              (attrs[.ownerAccountID] as? NSNumber)?.intValue == 0,
              (attrs[.posixPermissions] as? NSNumber)?.intValue == 0o600,
              ((attrs[.size] as? NSNumber)?.intValue ?? 4097) <= 4096 else {
            throw FixtureFailure(stage: "journal-permissions")
        }
        let identity = try JSONDecoder().decode(AccountIdentity.self, from: Data(contentsOf: journal))
        guard identity.name.hasPrefix("_agensitls"), identity.name.count == 26,
              identity.name.dropFirst(10).allSatisfy({ $0.isHexDigit }),
              let uid = UInt32(identity.uid), (60000...65000).contains(uid),
              UUID(uuidString: identity.guid) != nil else { throw FixtureFailure(stage: "journal-identity") }
        let found = try records(attribute: kODAttributeTypeRecordName, value: identity.name)
        guard found.count <= 1 else { throw FixtureFailure(stage: "account-duplicate") }
        if let record = found.first {
            guard try values(record, kODAttributeTypeUniqueID) == [identity.uid],
                  try values(record, kODAttributeTypeGUID) == [identity.guid] else {
                throw FixtureFailure(stage: "account-owner-mismatch")
            }
            if try values(admin, kODAttributeTypeGroupMembers).contains(identity.guid) ||
                values(admin, kODAttributeTypeGroupMembership).contains(identity.name) {
                try admin.removeMemberRecord(record)
            }
            try record.delete()
        }
        // A killed create/delete may leave only group membership; remove exact journal-owned values.
        for (attribute, value) in [(kODAttributeTypeGroupMembership, identity.name), (kODAttributeTypeGroupMembers, identity.guid)] {
            if try values(admin, attribute).contains(value) { try admin.removeValue(value, fromAttribute: attribute) }
            guard try !values(admin, attribute).contains(value) else { throw FixtureFailure(stage: "membership-residue") }
        }
        guard try records(attribute: kODAttributeTypeRecordName, value: identity.name).isEmpty,
              try records(attribute: kODAttributeTypeGUID, value: identity.guid).isEmpty else {
            throw FixtureFailure(stage: "account-delete-readback")
        }
        try FileManager.default.removeItem(at: journal)
        event("INFO", "Owned temporary account and membership absent; recovery journal cleared.")
    }
    func createAccount(password: String) throws -> AccountIdentity {
        try recover()
        setusershell()
        var shellAccepted = false
        var shellCount = 0
        defer { endusershell() }
        while let shell = getusershell() {
            shellCount += 1
            guard shellCount <= 128 else { throw FixtureFailure(stage: "account-shell-list-bound") }
            if strcmp(shell, "/bin/sh") == 0 { shellAccepted = true }
        }
        event("INFO", "Owned account shell accepted=" + String(shellAccepted))
        guard shellAccepted else { throw FixtureFailure(stage: "account-shell-unavailable") }
        let suffix = UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased().prefix(16)
        let name = "_agensitls" + suffix
        guard try records(attribute: kODAttributeTypeRecordName, value: name).isEmpty else {
            throw FixtureFailure(stage: "account-name-collision")
        }
        var selected: String?
        for _ in 0..<16 {
            let candidate = String(UInt32.random(in: 60000...65000))
            if try records(attribute: kODAttributeTypeUniqueID, value: candidate).isEmpty { selected = candidate; break }
        }
        guard let uid = selected else { throw FixtureFailure(stage: "account-uid-collision") }
        let identity = AccountIdentity(name: name, uid: uid, guid: UUID().uuidString)
        try save(identity)
        let attributes: [String: [String]] = [
            kODAttributeTypeUniqueID: [uid], kODAttributeTypePrimaryGroupID: ["20"],
            kODAttributeTypeGUID: [identity.guid], kODAttributeTypeNFSHomeDirectory: ["/var/empty"],
            kODAttributeTypeUserShell: ["/bin/sh"], kODAttributeTypeFullName: ["Agensi temporary TLS fixture"]
        ]
        var stage = "account-create"
        do {
            let record = try node.createRecord(withRecordType: kODRecordTypeUsers, name: name, attributes: attributes)
            stage = "account-password-set"
            try record.changePassword(nil, toPassword: password)
            stage = "account-password-verify"
            try record.verifyPassword(password)
            stage = "account-admin-add"
            try admin.addMemberRecord(record)
            stage = "account-create-readback"
            guard try values(admin, kODAttributeTypeGroupMembers).contains(identity.guid),
                  try values(admin, kODAttributeTypeGroupMembership).contains(identity.name),
                  try values(record, kODAttributeTypeUniqueID) == [uid],
                  try values(record, kODAttributeTypeGUID) == [identity.guid],
                  try values(record, kODAttributeTypeUserShell) == ["/bin/sh"] else { throw FixtureFailure(stage: stage) }
            event("INFO", "Owned temporary admin created and native password verified; no home created.")
            return identity
        } catch let failure as FixtureFailure {
            throw failure
        } catch {
            throw FixtureFailure(stage: stage + ":" + String((error as NSError).code))
        }
    }
    func verifyAccountLookup(_ identity: AccountIdentity) throws {
        guard let uid = uid_t(identity.uid) else { throw FixtureFailure(stage: "account-posix-uid") }
        errno = 0
        let entry = getpwuid(uid)
        let lookupStatus = entry == nil ? errno : 0
        let found = entry != nil
        let uidMatch = entry?.pointee.pw_uid == uid
        let nameMatch = entry?.pointee.pw_name.map { actual in
            identity.name.withCString { strcmp(actual, $0) == 0 }
        } ?? false
        let passwordEligible = entry.map { record in
            record.pointee.pw_passwd.map { strcmp($0, "*") != 0 } ?? true
        } ?? false
        event("INFO", "Owned POSIX lookup found=" + String(found) + " name_match=" + String(nameMatch) +
            " uid_match=" + String(uidMatch) + " password_eligible=" + String(passwordEligible) +
            " status=" + String(lookupStatus))
        guard found && nameMatch && uidMatch && passwordEligible else {
            throw FixtureFailure(stage: "account-posix-lookup")
        }
        errno = 0
        let group = getgrgid(80)
        let groupStatus = group == nil ? errno : 0
        let adminGroup = group?.pointee.gr_gid == 80 &&
            (group?.pointee.gr_name.map { strcmp($0, "admin") == 0 } ?? false)
        event("INFO", "Owned native admin group match=" + String(adminGroup) + " status=" + String(groupStatus))
        guard adminGroup else { throw FixtureFailure(stage: "account-native-admin-group") }
        var userUUID = UUID().uuid
        var groupUUID = UUID().uuid
        let userStatus = mbr_uid_to_uuid(uid, &userUUID)
        event("INFO", "Owned native user mapping status=" + String(userStatus))
        guard userStatus == 0 else { throw FixtureFailure(stage: "account-native-user-mapping") }
        let mappingStatus = mbr_gid_to_uuid(80, &groupUUID)
        event("INFO", "Owned native group mapping status=" + String(mappingStatus))
        guard mappingStatus == 0 else { throw FixtureFailure(stage: "account-native-group-mapping") }
        var member: Int32 = 0
        let membershipStatus = mbr_check_membership(&userUUID, &groupUUID, &member)
        event("INFO", "Owned native admin membership status=" + String(membershipStatus) +
            " member=" + String(membershipStatus == 0 && member == 1))
        guard membershipStatus == 0 && member == 1 else {
            throw FixtureFailure(stage: "account-native-admin-membership")
        }
    }
    func certificate(path: String) throws -> SecCertificate {
        let url = URL(fileURLWithPath: path).standardizedFileURL
        guard url.deletingLastPathComponent() == state,
              url.resolvingSymlinksInPath() == url else { throw FixtureFailure(stage: "certificate-path") }
        let fd = open(url.path, O_RDONLY | O_NOFOLLOW)
        guard fd >= 0 else { throw FixtureFailure(stage: "certificate-open") }
        let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? handle.close() }
        var attributes = stat()
        guard fstat(fd, &attributes) == 0, attributes.st_mode & S_IFMT == S_IFREG,
              attributes.st_size <= 16384,
              let data = try handle.read(upToCount: 16385), data.count <= 16384,
              let text = String(data: data, encoding: .utf8) else { throw FixtureFailure(stage: "certificate-size-or-type") }
        let body = text.replacingOccurrences(of: "-----BEGIN CERTIFICATE-----", with: "")
            .replacingOccurrences(of: "-----END CERTIFICATE-----", with: "")
            .components(separatedBy: .whitespacesAndNewlines).joined()
        guard let der = Data(base64Encoded: body), let certificate = SecCertificateCreateWithData(nil, der as CFData) else {
            throw FixtureFailure(stage: "certificate-decode")
        }
        return certificate
    }
    func trustPresent(_ certificate: SecCertificate) throws -> Bool {
        var settings: CFArray?
        let status = SecTrustSettingsCopyTrustSettings(certificate, .admin, &settings)
        if status == errSecItemNotFound || status == errSecNoTrustSettings { return false }
        try Self.check(status, "trust-presence")
        guard settings != nil else { throw FixtureFailure(stage: "trust-presence-value") }
        return true
    }
    func removeTrust(_ certificate: SecCertificate) throws {
        let removal = SecTrustSettingsRemoveTrustSettings(certificate, .admin)
        guard removal == errSecSuccess || removal == errSecItemNotFound || removal == errSecNoTrustSettings else {
            throw FixtureFailure(stage: "trust-remove:" + String(removal))
        }
        var settings: CFArray?
        let status = SecTrustSettingsCopyTrustSettings(certificate, .admin, &settings)
        guard status == errSecItemNotFound || status == errSecNoTrustSettings else {
            throw FixtureFailure(stage: "trust-removal-readback:" + String(status))
        }
        event("INFO", "Exact owned administrative trust record absent.")
    }
    func run(action: String, paths: [String]) throws {
        if action == "recover" { try recover(); return }
        guard ((action == "selftest" || action == "cancel-selftest") && paths.isEmpty) || (action == "input-selftest" && paths.isEmpty) ||
              (action == "remove" && !paths.isEmpty && paths.count <= 8) else {
            throw FixtureFailure(stage: "arguments")
        }
        var certificates: [SecCertificate] = []
        var pending: [SecCertificate] = []
        if action == "remove" {
            try recover()
            certificates = try paths.map { try certificate(path: $0) }
            for certificate in certificates {
                if try trustPresent(certificate) { pending.append(certificate) }
                else if try trustPresent(certificate) { throw FixtureFailure(stage: "trust-absence-changed") }
            }
            if pending.isEmpty {
                event("INFO", "Exact owned administrative trust already absent; no authorization requested.")
                return
            }
        }
        var session: SecuritySessionId = 0
        var attributes = SessionAttributeBits(rawValue: 0)
        try Self.check(SessionGetInfo(callerSecuritySession, &session, &attributes), "session-inherited")
        guard session != 0, attributes.contains(.sessionHasGraphicAccess) else {
            throw FixtureFailure(stage: "session-inherited-graphics")
        }
        event("INFO", "Inherited graphical security session prerequisite verified; native prompt still required.")
        if action == "input-selftest" {
            let name = "_agensitls0000000000000000"
            let password = String(repeating: "0123456789abcdef", count: 4)
            let input = InputFixture(name: name, password: password)
            try input.prepare()
            let identity = AccountIdentity(name: name, uid: "0", guid: "00000000-0000-0000-0000-000000000000")
            try withPrompt(identity, password: password, inputFixture: true) {
                try input.run(marker: URL(fileURLWithPath: CommandLine.arguments[0]).lastPathComponent) {
                    event("INFO", $0)
                }
            }
            return
        }
        var bytes = [UInt8](repeating: 0, count: 32)
        try Self.check(SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes), "password-random")
        var password = bytes.map { String(format: "%02x", $0) }.joined()
        defer { _ = bytes.withUnsafeMutableBytes { $0.initializeMemory(as: UInt8.self, repeating: 0) }; password.removeAll(keepingCapacity: false) }
        var operationError: Error?
        do {
            let identity = try createAccount(password: password)
            try verifyAccountLookup(identity)
            if action == "selftest" || action == "cancel-selftest" {
                let cancelOnly = action == "cancel-selftest"
                try withPrompt(identity, password: password, observeAuthorization: !cancelOnly, cancelOnly: cancelOnly) {
                    try verifyPrompt(expectedCancellation: cancelOnly)
                }
                throw FixtureFailure(stage: "controlled-restoration")
            } else {
                for certificate in pending { try withPrompt(identity, password: password, allowNoPrompt: true) { try removeTrust(certificate) } }
                for certificate in certificates {
                    if try trustPresent(certificate) { throw FixtureFailure(stage: "trust-final-absence") }
                }
            }
        } catch { operationError = error }
        try recover()
        if action == "selftest" || action == "cancel-selftest",
           let expected = operationError as? FixtureFailure, expected.stage == "controlled-restoration" {
            event("INFO", "Controlled failure restored account and authorization state.")
            return
        }
        if let operationError { throw operationError }
    }
}

@main
enum MacTrustFixtureMain {
    static func main() {
        var fixture: Fixture?
        do {
            let arguments = Array(CommandLine.arguments.dropFirst())
            guard arguments.count >= 2 else { throw FixtureFailure(stage: "arguments") }
            let owner = try Fixture(statePath: arguments[1])
            fixture = owner
            try owner.run(action: arguments[0], paths: Array(arguments.dropFirst(2)))
            owner.event("INFO", "Native trust fixture helper completed.")
        } catch {
            let stage = (error as? FixtureFailure)?.stage ?? "native-api"
            fixture?.event("ERROR", "Native helper failed stage=" + stage + "; private details omitted.")
            if fixture == nil { try? FileHandle.standardError.write(contentsOf: Data("Native helper initialization failed stage=\(stage); private details omitted.\n".utf8)) }
            exit(1)
        }
    }
}
