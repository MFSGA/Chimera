import Foundation
import XCTest
@testable import ChimeraDNSProxy

final class DNSProxyProviderTests: XCTestCase {
    func testBridgeAcceptsLoopbackAndValidPort() throws {
        let configuration = try DNSProxyBridgeConfiguration(providerOptions: [
            DNSProxyBridgeConfiguration.hostKey: "127.0.0.1",
            DNSProxyBridgeConfiguration.portKey: NSNumber(value: 1053),
        ])

        XCTAssertEqual(configuration.host, "127.0.0.1")
        XCTAssertEqual(configuration.port, 1053)
    }

    func testBridgeRejectsNonLoopbackAddress() {
        XCTAssertThrowsError(try DNSProxyBridgeConfiguration(providerOptions: [
            DNSProxyBridgeConfiguration.hostKey: "192.0.2.1",
            DNSProxyBridgeConfiguration.portKey: NSNumber(value: 1053),
        ])) { error in
            XCTAssertEqual(error as? DNSProxyConfigurationError, .missingLoopbackAddress)
        }
    }

    func testBridgeRejectsZeroAndOutOfRangePorts() {
        for port in [0, 65_536] {
            XCTAssertThrowsError(try DNSProxyBridgeConfiguration(providerOptions: [
                DNSProxyBridgeConfiguration.hostKey: "127.0.0.1",
                DNSProxyBridgeConfiguration.portKey: NSNumber(value: port),
            ])) { error in
                XCTAssertEqual(error as? DNSProxyConfigurationError, .invalidPort)
            }
        }
    }

    func testBridgeRejectsBooleanAsPort() {
        XCTAssertThrowsError(try DNSProxyBridgeConfiguration(providerOptions: [
            DNSProxyBridgeConfiguration.hostKey: "127.0.0.1",
            DNSProxyBridgeConfiguration.portKey: NSNumber(value: true),
        ])) { error in
            XCTAssertEqual(error as? DNSProxyConfigurationError, .invalidPort)
        }
    }
}
