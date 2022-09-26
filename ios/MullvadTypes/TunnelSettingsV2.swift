//
//  TunnelSettingsV2.swift
//  MullvadVPN
//
//  Created by pronebird on 27/04/2022.
//  Copyright © 2022 Mullvad VPN AB. All rights reserved.
//

import Foundation
import struct Network.IPv4Address
import struct WireGuardKitTypes.IPAddressRange
import class WireGuardKitTypes.PrivateKey
import class WireGuardKitTypes.PublicKey

public struct TunnelSettingsV2: Codable, Equatable {
    /// Relay constraints.
    public var relayConstraints = RelayConstraints()

    /// DNS settings.
    public var dnsSettings = DNSSettings()
}

public struct StoredAccountData: Codable, Equatable {
    /// Account identifier.
    public var identifier: String

    /// Account number.
    public var number: String

    /// Account expiry.
    public var expiry: Date
}

public enum DeviceState: Codable, Equatable {
    case loggedIn(StoredAccountData, StoredDeviceData)
    case loggedOut
    case revoked

    private enum LoggedInCodableKeys: String, CodingKey {
        case _0 = "account"
        case _1 = "device"
    }

    public var isLoggedIn: Bool {
        switch self {
        case .loggedIn:
            return true
        case .loggedOut, .revoked:
            return false
        }
    }

    public var accountData: StoredAccountData? {
        switch self {
        case let .loggedIn(accountData, _):
            return accountData
        case .loggedOut, .revoked:
            return nil
        }
    }

    public var deviceData: StoredDeviceData? {
        switch self {
        case let .loggedIn(_, deviceData):
            return deviceData
        case .loggedOut, .revoked:
            return nil
        }
    }
}

public struct StoredDeviceData: Codable, Equatable {
    /// Device creation date.
    public var creationDate: Date

    /// Device identifier.
    public var identifier: String

    /// Device name.
    public var name: String

    /// Whether relay hijacks DNS from this device.
    public var hijackDNS: Bool

    /// IPv4 address assigned to device.
    public var ipv4Address: IPAddressRange

    /// IPv6 address assignged to device.
    public var ipv6Address: IPAddressRange

    /// WireGuard key data.
    public var wgKeyData: StoredWgKeyData
}

public struct StoredWgKeyData: Codable, Equatable {
    /// Private key creation date.
    public var creationDate: Date

    /// Private key.
    public var privateKey: PrivateKey
}
