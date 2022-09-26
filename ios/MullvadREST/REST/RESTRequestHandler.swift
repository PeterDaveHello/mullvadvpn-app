//
//  RESTRequestHandler.swift
//  MullvadVPN
//
//  Created by pronebird on 20/04/2022.
//  Copyright © 2022 Mullvad VPN AB. All rights reserved.
//

import Foundation
import MullvadTypes

public protocol RESTRequestHandler {
    func createURLRequest(
        endpoint: AnyIPEndpoint,
        authorization: REST.Authorization?
    ) throws -> REST.Request

    var authorizationProvider: RESTAuthorizationProvider? { get }
}

public extension REST {
    struct Request {
        public var urlRequest: URLRequest
        public var pathTemplate: URLPathTemplate
    }

    final class AnyRequestHandler: RESTRequestHandler {
        private let _createURLRequest: (AnyIPEndpoint, REST.Authorization?) throws -> REST.Request

        public let authorizationProvider: RESTAuthorizationProvider?

        public init(createURLRequest: @escaping (AnyIPEndpoint) throws -> REST.Request) {
            _createURLRequest = { endpoint, authorization in
                return try createURLRequest(endpoint)
            }
            authorizationProvider = nil
        }

        public init(
            createURLRequest: @escaping (AnyIPEndpoint, REST.Authorization) throws -> REST.Request,
            authorizationProvider: RESTAuthorizationProvider
        ) {
            _createURLRequest = { endpoint, authorization in
                return try createURLRequest(endpoint, authorization!)
            }
            self.authorizationProvider = authorizationProvider
        }

        public func createURLRequest(
            endpoint: AnyIPEndpoint,
            authorization: REST.Authorization?
        ) throws -> REST.Request {
            return try _createURLRequest(endpoint, authorization)
        }
    }
}
