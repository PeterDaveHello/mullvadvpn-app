//
//  RelaySelectorResult.swift
//  MullvadTypes
//
//  Created by pronebird on 26/09/2022.
//  Copyright © 2022 Mullvad VPN AB. All rights reserved.
//

import Foundation
import MullvadREST
import MullvadTypes

public struct RelaySelectorResult: Codable {
    public var endpoint: MullvadEndpoint
    public var relay: REST.ServerRelay
    public var location: Location

    public var packetTunnelRelay: PacketTunnelRelay {
        return PacketTunnelRelay(
            ipv4Relay: endpoint.ipv4Relay,
            ipv6Relay: endpoint.ipv6Relay,
            hostname: relay.hostname,
            location: location
        )
    }
}
