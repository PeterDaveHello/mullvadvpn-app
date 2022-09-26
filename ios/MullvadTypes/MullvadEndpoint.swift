//
//  MullvadEndpoint.swift
//  MullvadVPN
//
//  Created by pronebird on 12/06/2019.
//  Copyright © 2019 Mullvad VPN AB. All rights reserved.
//

import Foundation
import Network

/// Contains server data needed to connect to a single mullvad endpoint
public struct MullvadEndpoint: Equatable, Codable {
    public let ipv4Relay: IPv4Endpoint
    public let ipv6Relay: IPv6Endpoint?
    public let ipv4Gateway: IPv4Address
    public let ipv6Gateway: IPv6Address
    public let publicKey: Data
}
