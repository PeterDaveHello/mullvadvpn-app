//
//  PacketTunnelStatus.swift
//  MullvadVPN
//
//  Created by pronebird on 27/07/2021.
//  Copyright © 2021 Mullvad VPN AB. All rights reserved.
//

import Foundation

/// Struct describing packet tunnel process status.
public struct PacketTunnelStatus: Codable, Equatable {
    /// Last tunnel error.
    public var lastError: String? = nil

    /// Flag indicating whether network is reachable.
    public var isNetworkReachable = true

    /// Current relay.
    public var tunnelRelay: PacketTunnelRelay?
}

/// Struct holding tunnel relay information.
public struct PacketTunnelRelay: Codable, Equatable {
    /// IPv4 relay endpoint.
    public let ipv4Relay: IPv4Endpoint

    /// IPv6 relay endpoint.
    public let ipv6Relay: IPv6Endpoint?

    /// Relay hostname.
    public let hostname: String

    /// Relay location.
    public let location: Location
}
