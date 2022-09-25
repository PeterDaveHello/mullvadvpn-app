//
//  RESTConfiguration.swift
//  MullvadREST
//
//  Created by pronebird on 25/09/2022.
//  Copyright © 2022 Mullvad VPN AB. All rights reserved.
//

import Foundation

public extension REST {
    enum Configuration {
        /// Default API hostname.
        static let defaultAPIHostname = "api.mullvad.net"

        /// Default API endpoint.
        static let defaultAPIEndpoint = AnyIPEndpoint(string: "45.83.222.100:443")!

        /// Default network timeout for API requests.
        static let defaultAPINetworkTimeout: TimeInterval = 10
    }
}
