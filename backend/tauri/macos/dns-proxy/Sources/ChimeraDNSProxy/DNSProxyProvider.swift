import CoreFoundation
import Foundation
import Network
import NetworkExtension

enum DNSProxyConfigurationError: Error, Equatable {
    case missingLoopbackAddress
    case invalidPort
    case invalidEndpoint
}

private enum DNSProxyFlowError: Error {
    case timedOut
    case malformedDatagrams
    case emptyResponse
}

struct DNSProxyBridgeConfiguration: Equatable {
    static let hostKey = "resolverHost"
    static let portKey = "resolverPort"

    let host: String
    let port: UInt16

    init(providerOptions: [String: Any]?) throws {
        guard let host = providerOptions?[Self.hostKey] as? String,
              host == "127.0.0.1"
        else {
            throw DNSProxyConfigurationError.missingLoopbackAddress
        }

        guard let portValue = providerOptions?[Self.portKey] as? NSNumber,
              CFGetTypeID(portValue) == CFNumberGetTypeID()
        else {
            throw DNSProxyConfigurationError.invalidPort
        }

        let rawPort = portValue.intValue
        guard portValue.doubleValue == Double(rawPort),
              rawPort > 0,
              rawPort <= Int(UInt16.max)
        else {
            throw DNSProxyConfigurationError.invalidPort
        }

        self.host = host
        self.port = UInt16(rawPort)
    }

    func networkEndpoint() throws -> Network.NWEndpoint {
        guard let address = Network.IPv4Address(host),
              let port = Network.NWEndpoint.Port(rawValue: port)
        else {
            throw DNSProxyConfigurationError.invalidEndpoint
        }
        return .hostPort(host: .ipv4(address), port: port)
    }
}

/// macOS DNS Proxy extension that forwards the intercepted UDP/TCP 53 flows
/// to Chimera's loopback DNS listener. It deliberately accepts only 127.0.0.1
/// as the bridge address so provider configuration cannot turn this into a
/// general-purpose relay or send DNS queries to a remote resolver.
final class DNSProxyProvider: NEDNSProxyProvider {
    private let stateQueue = DispatchQueue(label: "com.chimera.dns-proxy.state")
    private var bridgeEndpoint: Network.NWEndpoint?
    private var isRunning = false
    private var relays: [UUID: DNSProxyFlowRelay] = [:]

    override func startProxy(
        options: [String: Any]?,
        completionHandler: @escaping (Error?) -> Void
    ) {
        do {
            let configuration = try DNSProxyBridgeConfiguration(providerOptions: options)
            let endpoint = try configuration.networkEndpoint()
            stateQueue.sync {
                bridgeEndpoint = endpoint
                isRunning = true
            }
            completionHandler(nil)
        } catch {
            completionHandler(error)
        }
    }

    override func stopProxy(
        with reason: NEProviderStopReason,
        completionHandler: @escaping () -> Void
    ) {
        stateQueue.sync {
            isRunning = false
            bridgeEndpoint = nil
            let activeRelays = Array(relays.values)
            relays.removeAll()
            activeRelays.forEach { $0.stopOnQueue() }
        }
        completionHandler()
    }

    override func handleNewFlow(_ flow: NEAppProxyFlow) -> Bool {
        let state = stateQueue.sync { (isRunning, bridgeEndpoint) }
        guard state.0, let endpoint = state.1 else {
            return false
        }

        let relay: DNSProxyFlowRelay
        if let udpFlow = flow as? NEAppProxyUDPFlow {
            relay = DNSProxyFlowRelay(
                id: UUID(),
                flow: udpFlow,
                endpoint: endpoint,
                queue: stateQueue,
                onFinish: { [weak self] id in self?.removeRelay(id) }
            )
        } else if let tcpFlow = flow as? NEAppProxyTCPFlow {
            relay = DNSProxyFlowRelay(
                id: UUID(),
                flow: tcpFlow,
                endpoint: endpoint,
                queue: stateQueue,
                onFinish: { [weak self] id in self?.removeRelay(id) }
            )
        } else {
            return false
        }

        let accepted = stateQueue.sync { () -> Bool in
            guard isRunning else { return false }
            relays[relay.id] = relay
            return true
        }
        guard accepted else { return false }

        relay.start()
        return true
    }

    private func removeRelay(_ id: UUID) {
        stateQueue.async { [weak self] in
            self?.relays.removeValue(forKey: id)
        }
    }
}

private final class DNSProxyFlowRelay {
    private enum Transport {
        case udp(NEAppProxyUDPFlow)
        case tcp(NEAppProxyTCPFlow)
    }

    private struct Datagram {
        let data: Data
        let writeResponse: (Data, @escaping (Error?) -> Void) -> Void
    }

    let id: UUID
    private let transport: Transport
    private let endpoint: Network.NWEndpoint
    private let queue: DispatchQueue
    private let onFinish: (UUID) -> Void
    private var connection: NWConnection?
    private var pendingDatagrams: [Datagram] = []
    private var nextDatagramIndex = 0
    private var isFinished = false
    private var timeoutWorkItem: DispatchWorkItem?

    init(
        id: UUID,
        flow: NEAppProxyUDPFlow,
        endpoint: Network.NWEndpoint,
        queue: DispatchQueue,
        onFinish: @escaping (UUID) -> Void
    ) {
        self.id = id
        transport = .udp(flow)
        self.endpoint = endpoint
        self.queue = queue
        self.onFinish = onFinish
    }

    init(
        id: UUID,
        flow: NEAppProxyTCPFlow,
        endpoint: Network.NWEndpoint,
        queue: DispatchQueue,
        onFinish: @escaping (UUID) -> Void
    ) {
        self.id = id
        transport = .tcp(flow)
        self.endpoint = endpoint
        self.queue = queue
        self.onFinish = onFinish
    }

    func start() {
        queue.async { [weak self] in
            self?.startOnQueue()
        }
    }

    private func startOnQueue() {
        guard !isFinished else { return }
        switch transport {
        case let .udp(flow):
            scheduleTimeout(after: 5)
            flow.open(withLocalEndpoint: nil) { [weak self] error in
                guard let self else { return }
                queue.async {
                    if let error {
                        self.finish(error)
                    } else {
                        self.cancelTimeout()
                        self.readNextUDPBatch()
                    }
                }
            }
        case let .tcp(flow):
            startTCP(flow)
        }
    }

    func stopOnQueue() {
        finish(nil)
    }

    private func startTCP(_ flow: NEAppProxyTCPFlow) {
        let connection = NWConnection(to: endpoint, using: .tcp)
        self.connection = connection
        scheduleTimeout(after: 5)
        connection.stateUpdateHandler = { [weak self, weak connection] state in
            guard let self, let connection else { return }
            switch state {
            case .ready:
                self.cancelTimeout()
                self.openTCPFlow(flow, connection: connection)
            case let .failed(error):
                self.finish(error)
            case .cancelled:
                self.finish(nil)
            default:
                break
            }
        }
        connection.start(queue: queue)
    }

    private func openTCPFlow(_ flow: NEAppProxyTCPFlow, connection: NWConnection) {
        scheduleTimeout(after: 10)
        flow.open(withLocalEndpoint: nil) { [weak self, weak connection] error in
            guard let self, let connection else { return }
            self.queue.async {
                if let error {
                    self.finish(error)
                    return
                }
                self.scheduleTimeout(after: 120)
                self.readNextTCPClientChunk(flow, connection: connection)
                self.readNextTCPServerChunk(flow, connection: connection)
            }
        }
    }

    private func readNextTCPClientChunk(
        _ flow: NEAppProxyTCPFlow,
        connection: NWConnection
    ) {
        guard !isFinished else { return }
        flow.readData { [weak self, weak connection] data, error in
            guard let self, let connection else { return }
            self.queue.async {
                if let error {
                    self.finish(error)
                    return
                }
                guard let data, !data.isEmpty else {
                    connection.send(
                        content: nil,
                        contentContext: .finalMessage,
                        isComplete: true,
                        completion: .contentProcessed { [self] error in
                            self.queue.async {
                                if let error { self.finish(error) }
                            }
                        }
                    )
                    return
                }
                self.scheduleTimeout(after: 120)
                connection.send(content: data, completion: .contentProcessed { [self] error in
                    self.queue.async {
                        if let error {
                            self.finish(error)
                        } else {
                            self.readNextTCPClientChunk(flow, connection: connection)
                        }
                    }
                })
            }
        }
    }

    private func readNextTCPServerChunk(
        _ flow: NEAppProxyTCPFlow,
        connection: NWConnection
    ) {
        guard !isFinished else { return }
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65_535) {
            [weak self, weak connection] data, _, isComplete, error in
            guard let self, let connection else { return }
            self.queue.async {
                if let data, !data.isEmpty {
                    self.scheduleTimeout(after: 120)
                    flow.write(data) { [self] error in
                        self.queue.async {
                            if let error {
                                self.finish(error)
                            } else if isComplete {
                                self.finish(nil)
                            } else {
                                self.readNextTCPServerChunk(flow, connection: connection)
                            }
                        }
                    }
                } else if let error {
                    self.finish(error)
                } else if isComplete {
                    self.finish(nil)
                } else {
                    self.readNextTCPServerChunk(flow, connection: connection)
                }
            }
        }
    }

    private func readNextUDPBatch() {
        guard !isFinished, case let .udp(flow) = transport else { return }
        scheduleTimeout(after: 120)
        flow.readDatagrams { [weak self] datagrams, endpoints, error in
            guard let self else { return }
            self.queue.async {
                if let error {
                    self.finish(error)
                    return
                }
                self.cancelTimeout()
                guard let datagrams, let endpoints,
                      datagrams.count == endpoints.count
                else {
                    self.finish(DNSProxyFlowError.malformedDatagrams)
                    return
                }
                guard !datagrams.isEmpty else {
                    self.finish(nil)
                    return
                }
                self.pendingDatagrams = zip(datagrams, endpoints).map { data, endpoint in
                    Datagram(
                        data: data,
                        writeResponse: { response, completion in
                            flow.writeDatagrams(
                                [response],
                                sentBy: [endpoint],
                                completionHandler: completion
                            )
                        }
                    )
                }
                self.nextDatagramIndex = 0
                self.sendNextUDPDatagram()
            }
        }
    }

    private func sendNextUDPDatagram() {
        guard !isFinished, case .udp = transport else { return }
        guard nextDatagramIndex < pendingDatagrams.count else {
            pendingDatagrams.removeAll(keepingCapacity: true)
            readNextUDPBatch()
            return
        }

        let datagram = pendingDatagrams[nextDatagramIndex]
        nextDatagramIndex += 1
        let connection = NWConnection(to: endpoint, using: .udp)
        self.connection = connection
        scheduleTimeout(after: 5)
        connection.stateUpdateHandler = { [weak self, weak connection] state in
            guard let self, let connection else { return }
            switch state {
            case .ready:
                self.cancelTimeout()
                self.scheduleTimeout(after: 10)
                connection.send(content: datagram.data, completion: .contentProcessed { [weak self] error in
                    guard let self else { return }
                    self.queue.async {
                        if let error {
                            self.finish(error)
                        } else {
                            self.readUDPResponse(
                                connection: connection,
                                datagram: datagram
                            )
                        }
                    }
                })
            case let .failed(error):
                self.finish(error)
            case .cancelled:
                self.finish(nil)
            default:
                break
            }
        }
        connection.start(queue: queue)
    }

    private func readUDPResponse(
        connection: NWConnection,
        datagram: Datagram
    ) {
        connection.receiveMessage { [weak self, weak connection] data, _, _, error in
            guard let self, let connection else { return }
            self.queue.async {
                if let error {
                    self.finish(error)
                    return
                }
                guard let data, !data.isEmpty else {
                    self.finish(DNSProxyFlowError.emptyResponse)
                    return
                }
                datagram.writeResponse(data) { [self] error in
                    self.queue.async {
                        if let error {
                            self.finish(error)
                        } else {
                            self.cancelTimeout()
                            connection.stateUpdateHandler = nil
                            connection.cancel()
                            self.connection = nil
                            self.sendNextUDPDatagram()
                        }
                    }
                }
            }
        }
    }

    private func finish(_ error: Error?) {
        guard !isFinished else { return }
        isFinished = true
        cancelTimeout()
        connection?.stateUpdateHandler = nil
        connection?.cancel()
        connection = nil
        switch transport {
        case let .udp(flow):
            flow.closeReadWithError(error)
            flow.closeWriteWithError(error)
        case let .tcp(flow):
            flow.closeReadWithError(error)
            flow.closeWriteWithError(error)
        }
        onFinish(id)
    }

    private func scheduleTimeout(after seconds: TimeInterval) {
        timeoutWorkItem?.cancel()
        let timeout = DispatchWorkItem { [weak self] in
            guard let self, !self.isFinished else { return }
            self.timeoutWorkItem = nil
            self.finish(DNSProxyFlowError.timedOut)
        }
        timeoutWorkItem = timeout
        queue.asyncAfter(deadline: .now() + seconds, execute: timeout)
    }

    private func cancelTimeout() {
        timeoutWorkItem?.cancel()
        timeoutWorkItem = nil
    }
}
