import Foundation
import IrohLib

/// How this phone talks to the computer: iroh, dialled by the computer's public key, with
/// one QUIC stream per API request (`mimi/1`, see `crates/core/src/remote/wire.rs`).
///
/// The phone has its own key (kept in the Keychain); the computer only lets in keys it
/// paired. The connection is opened lazily and opened again after it drops.
actor DaemonLink {
    static let alpn = Data("mimi/1".utf8)
    private static let keyAccount = "endpoint-key"
    /// Bodies are JSON, or at most an attachment.
    private static let maxBody: UInt32 = 64 * 1024 * 1024

    private let target: String
    private let relay: String?
    private var token: String?
    private var endpoint: Endpoint?
    private var connection: Connection?
    private var connecting: Task<Connection, Error>?

    init(endpointId: String, relay: String?, token: String?) {
        self.target = endpointId
        self.relay = relay
        self.token = token
    }

    func setToken(_ token: String?) { self.token = token }

    /// Drops the connection, e.g. after the network changed or the app came back.
    func reset() {
        if let connection {
            try? connection.close(errorCode: 0, reason: Data())
        }
        connection = nil
        connecting?.cancel()
        connecting = nil
    }

    /// How the open connection reaches the computer, if one is open.
    func path() -> LinkPath? {
        guard let connection, connection.closeReason() == nil else { return nil }
        let selected = connection.paths().first { $0.isSelected }
        return selected.map { $0.isIp ? .direct : .relayed }
    }

    // MARK: Requests

    nonisolated struct Response: Sendable {
        var status: Int
        var headers: [String: String]
        var body: Data
    }

    func send(_ method: String, _ path: String, body: Data? = nil, headers: [String: String] = [:]) async throws -> Response {
        do {
            return try await attempt(method, path, body: body, headers: headers)
        } catch let error as MimiError {
            throw error
        } catch {
            // A dead connection shows up on the first stream: try once more on a new one.
            connection = nil
            do {
                return try await attempt(method, path, body: body, headers: headers)
            } catch let error as MimiError {
                throw error
            } catch {
                connection = nil
                throw MimiError.unreachable
            }
        }
    }

    private func attempt(_ method: String, _ path: String, body: Data?, headers: [String: String]) async throws -> Response {
        let conn = try await open()
        let bi = try await conn.openBi()
        let send = bi.send()
        let recv = bi.recv()
        try await writeHead(send, method: method, path: path, body: body, headers: headers)
        if let body, !body.isEmpty { try await send.writeAll(buf: body) }
        try await send.finish()
        let head = try await readHead(recv)
        let data = try await recv.readToEnd(sizeLimit: Self.maxBody)
        return Response(status: head.status, headers: head.headers, body: data)
    }

    /// A streamed response, line by line (the event feed). Blank lines are heartbeats and
    /// are passed on as empty strings, so the caller can tell the feed is alive.
    func lines(_ path: String, headers: [String: String] = [:]) async throws -> AsyncThrowingStream<String, Error> {
        let conn = try await open()
        let bi = try await conn.openBi()
        let send = bi.send()
        let recv = bi.recv()
        try await writeHead(send, method: "GET", path: path, body: nil, headers: headers)
        try await send.finish()
        let head = try await readHead(recv)
        guard (200..<300).contains(head.status) else {
            throw head.status == 401 ? MimiError.unpaired : MimiError.http(head.status)
        }
        return AsyncThrowingStream { continuation in
            let task = Task {
                var buffer = Data()
                do {
                    while !Task.isCancelled {
                        let chunk = try await recv.read(sizeLimit: 64 * 1024)
                        if chunk.isEmpty { break }
                        buffer.append(chunk)
                        while let newline = buffer.firstIndex(of: 0x0A) {
                            let line = buffer[buffer.startIndex..<newline]
                            buffer.removeSubrange(buffer.startIndex...newline)
                            continuation.yield(String(decoding: line, as: UTF8.self))
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in
                task.cancel()
                Task { try? await recv.stop(errorCode: 0) }
            }
        }
    }

    // MARK: Connection

    private func open() async throws -> Connection {
        if let connection, connection.closeReason() == nil { return connection }
        if let connecting { return try await connecting.value }
        let task = Task { [target, relay] in
            let endpoint = try await self.boundEndpoint()
            let id = try EndpointId.fromString(s: target)
            let addr = EndpointAddr(id: id, relayUrl: relay, addresses: [])
            return try await withTimeout(seconds: 20) {
                try await endpoint.connect(addr: addr, alpn: Self.alpn)
            }
        }
        connecting = task
        defer { connecting = nil }
        do {
            let conn = try await task.value
            connection = conn
            return conn
        } catch {
            throw MimiError.unreachable
        }
    }

    private func boundEndpoint() async throws -> Endpoint {
        if let endpoint, !endpoint.isClosed() { return endpoint }
        let key = Self.phoneKey()
        let options: EndpointOptions
        if let relay {
            // The user's own relay: nothing is looked up or published anywhere else.
            options = EndpointOptions(
                preset: presetMinimal(),
                secretKey: key,
                alpns: [Self.alpn],
                relayMode: try RelayMode.customFromUrls(urls: [relay])
            )
        } else {
            options = EndpointOptions(preset: presetN0(), secretKey: key, alpns: [Self.alpn])
        }
        let bound = try await Endpoint.bind(options: options)
        endpoint = bound
        return bound
    }

    /// This phone's key, made on first use. Its public half is what the computer knows
    /// this phone by.
    nonisolated static func phoneKey() -> Data {
        if let key = Keychain.data(keyAccount), key.count == 32 { return key }
        let key = SecretKey.generate().toBytes()
        Keychain.set(key, for: keyAccount)
        return key
    }

    /// A new phone identity, after it was removed from the computer.
    nonisolated static func forgetPhoneKey() { Keychain.delete(keyAccount) }

    // MARK: Wire format

    nonisolated private struct RequestHead: Encodable {
        var method: String
        var path: String
        var headers: [String: String]
    }

    nonisolated private struct ResponseHead: Decodable {
        var status: Int
        var headers: [String: String]
    }

    private func writeHead(_ send: SendStream, method: String, path: String, body: Data?, headers: [String: String]) async throws {
        var all = headers
        if let token { all["authorization"] = "Bearer \(token)" }
        if body != nil, all["content-type"] == nil { all["content-type"] = "application/json" }
        let json = try JSONEncoder().encode(RequestHead(method: method, path: path, headers: all))
        var frame = Data()
        var length = UInt32(json.count).bigEndian
        frame.append(Data(bytes: &length, count: 4))
        frame.append(json)
        try await send.writeAll(buf: frame)
    }

    private func readHead(_ recv: RecvStream) async throws -> ResponseHead {
        let lengthBytes = try await recv.readExact(size: 4)
        let length = lengthBytes.reduce(UInt32(0)) { $0 << 8 | UInt32($1) }
        guard length <= 64 * 1024 else { throw MimiError.unreachable }
        let json = try await recv.readExact(size: length)
        return try JSONDecoder().decode(ResponseHead.self, from: json)
    }
}

nonisolated enum LinkPath: Sendable { case direct, relayed }

/// Runs `body`, giving up after `seconds`.
nonisolated func withTimeout<T: Sendable>(seconds: Double, _ body: @escaping @Sendable () async throws -> T) async throws -> T {
    try await withThrowingTaskGroup(of: T.self) { group in
        group.addTask { try await body() }
        group.addTask {
            try await Task.sleep(for: .seconds(seconds))
            throw MimiError.unreachable
        }
        defer { group.cancelAll() }
        return try await group.next()!
    }
}
