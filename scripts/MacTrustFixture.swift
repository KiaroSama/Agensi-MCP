import Darwin
import Foundation
import OpenDirectory
import Security

struct FixtureFailure: Error { let stage: String }
struct AccountIdentity: Codable {
    let name: String
    let uid: String
    let guid: String
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
        var before: SecuritySessionId = 0
        try Self.check(SessionGetInfo(callerSecuritySession, &before, nil), "session-before")
        try Self.check(SessionCreate(SessionCreationFlags(rawValue: 0), SessionAttributeBits(rawValue: 0)), "session-create")
        var after: SecuritySessionId = 0
        try Self.check(SessionGetInfo(callerSecuritySession, &after, nil), "session-after")
        guard after != 0, before != after else { throw FixtureFailure(stage: "session-isolation") }
        node = try ODNode(session: ODSession.default(), type: UInt32(kODNodeTypeLocalNodes))
        admin = try node.record(withRecordType: kODRecordTypeGroups, name: "admin", attributes: kODAttributeTypeAllAttributes)
        event("INFO", "Isolated native authorization session established.")
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
            kODAttributeTypeUserShell: ["/usr/bin/false"], kODAttributeTypeFullName: ["Agensi temporary TLS fixture"]
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
                  try values(record, kODAttributeTypeGUID) == [identity.guid] else { throw FixtureFailure(stage: stage) }
            event("INFO", "Owned temporary admin created and native password verified; no home created.")
            return identity
        } catch let failure as FixtureFailure {
            throw failure
        } catch {
            throw FixtureFailure(stage: stage + ":" + String((error as NSError).code))
        }
    }
    func authorize(_ identity: AccountIdentity, password: String, operation: () throws -> Void) throws {
        var authorization: AuthorizationRef?
        try Self.check(AuthorizationCreate(nil, nil, [], &authorization), "authorization-create")
        guard let authorization else { throw FixtureFailure(stage: "authorization-reference") }
        var operationError: Error?
        let environmentNames: [UnsafeMutablePointer<CChar>] = [kAuthorizationEnvironmentUsername, kAuthorizationEnvironmentPassword, kAuthorizationEnvironmentShared].compactMap { value in
            value.withCString { strdup($0) }
        }
        defer { environmentNames.forEach { free($0) } }
        guard environmentNames.count == 3 else {
            _ = AuthorizationFree(authorization, [.destroyRights])
            throw FixtureFailure(stage: "authorization-allocation")
        }
        do {
            let status = identity.name.withCString { name in
                password.withCString { secret in
                    "com.apple.trust-settings.admin".withCString { right in
                        var item = AuthorizationItem(name: right, valueLength: 0, value: nil, flags: 0)
                        return withUnsafeMutablePointer(to: &item) { itemPointer in
                            var rights = AuthorizationRights(count: 1, items: itemPointer)
                            var environment = [
                                AuthorizationItem(name: UnsafePointer(environmentNames[0]), valueLength: strlen(name), value: UnsafeMutableRawPointer(mutating: name), flags: 0),
                                AuthorizationItem(name: UnsafePointer(environmentNames[1]), valueLength: strlen(secret), value: UnsafeMutableRawPointer(mutating: secret), flags: 0),
                                AuthorizationItem(name: UnsafePointer(environmentNames[2]), valueLength: 0, value: nil, flags: 0)
                            ]
                            return environment.withUnsafeMutableBufferPointer { buffer in
                                var inputs = AuthorizationEnvironment(count: UInt32(buffer.count), items: buffer.baseAddress)
                                return AuthorizationCopyRights(authorization, &rights, &inputs, [.extendRights, .preAuthorize], nil)
                            }
                        }
                    }
                }
            }
            try Self.check(status, "authorization-genuine-admin")
            try operation()
        } catch { operationError = error }
        try Self.check(AuthorizationFree(authorization, [.destroyRights]), "authorization-destroy")
        if let operationError { throw operationError }
        event("INFO", "Owned authorization destroyed after native operation.")
    }
    func removeTrust(path: String) throws {
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
        guard action == "selftest" || (action == "remove" && !paths.isEmpty && paths.count <= 8) else {
            throw FixtureFailure(stage: "arguments")
        }
        var bytes = [UInt8](repeating: 0, count: 32)
        try Self.check(SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes), "password-random")
        var password = bytes.map { String(format: "%02x", $0) }.joined()
        defer { _ = bytes.withUnsafeMutableBytes { $0.initializeMemory(as: UInt8.self, repeating: 0) }; password.removeAll(keepingCapacity: false) }
        var operationError: Error?
        do {
            let identity = try createAccount(password: password)
            if action == "selftest" {
                try authorize(identity, password: password) { throw FixtureFailure(stage: "controlled-restoration") }
            } else {
                for path in paths { try authorize(identity, password: password) { try removeTrust(path: path) } }
            }
        } catch { operationError = error }
        try recover()
        if action == "selftest", let expected = operationError as? FixtureFailure, expected.stage == "controlled-restoration" {
            event("INFO", "Controlled failure restored account and authorization state.")
            return
        }
        if let operationError { throw operationError }
    }
}

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
