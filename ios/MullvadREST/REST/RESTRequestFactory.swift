//
//  RESTRequestFactory.swift
//  MullvadVPN
//
//  Created by pronebird on 16/04/2022.
//  Copyright © 2022 Mullvad VPN AB. All rights reserved.
//

import Foundation
import MullvadTypes

public extension REST {
    class RequestFactory {
        public let hostname: String
        public let pathPrefix: String
        public let networkTimeout: TimeInterval
        public let bodyEncoder: JSONEncoder

        public class func withDefaultAPICredentials(
            pathPrefix: String,
            bodyEncoder: JSONEncoder
        ) -> RequestFactory {
            return RequestFactory(
                hostname: REST.Configuration.defaultAPIHostname,
                pathPrefix: pathPrefix,
                networkTimeout: REST.Configuration.defaultAPINetworkTimeout,
                bodyEncoder: bodyEncoder
            )
        }

        public init(
            hostname: String,
            pathPrefix: String,
            networkTimeout: TimeInterval,
            bodyEncoder: JSONEncoder
        ) {
            self.hostname = hostname
            self.pathPrefix = pathPrefix
            self.networkTimeout = networkTimeout
            self.bodyEncoder = bodyEncoder
        }

        public func createRequest(
            endpoint: AnyIPEndpoint,
            method: HTTPMethod,
            pathTemplate: URLPathTemplate
        ) throws -> REST.Request {
            var urlComponents = URLComponents()
            urlComponents.scheme = "https"
            urlComponents.path = pathPrefix
            urlComponents.host = "\(endpoint.ip)"
            urlComponents.port = Int(endpoint.port)

            let pathString = try pathTemplate.pathString()
            let requestURL = urlComponents.url!.appendingPathComponent(pathString)

            var request = URLRequest(
                url: requestURL,
                cachePolicy: .useProtocolCachePolicy,
                timeoutInterval: networkTimeout
            )
            request.httpShouldHandleCookies = false
            request.addValue(hostname, forHTTPHeaderField: HTTPHeader.host)
            request.addValue("application/json", forHTTPHeaderField: HTTPHeader.contentType)
            request.httpMethod = method.rawValue

            let prefixedPathTemplate = URLPathTemplate(stringLiteral: pathPrefix) + pathTemplate

            return REST.Request(
                urlRequest: request,
                pathTemplate: prefixedPathTemplate
            )
        }

        public func createRequestBuilder(
            endpoint: AnyIPEndpoint,
            method: HTTPMethod,
            pathTemplate: URLPathTemplate
        ) throws -> RequestBuilder {
            let request = try createRequest(
                endpoint: endpoint,
                method: method,
                pathTemplate: pathTemplate
            )

            return RequestBuilder(
                restRequest: request,
                bodyEncoder: bodyEncoder
            )
        }
    }

    struct RequestBuilder {
        private var restRequest: REST.Request
        private let bodyEncoder: JSONEncoder

        public init(restRequest: REST.Request, bodyEncoder: JSONEncoder) {
            self.restRequest = restRequest
            self.bodyEncoder = bodyEncoder
        }

        public mutating func setHTTPBody<T: Encodable>(value: T) throws {
            restRequest.urlRequest.httpBody = try bodyEncoder.encode(value)
        }

        public mutating func setETagHeader(etag: String) {
            var etag = etag
            // Enforce weak validator to account for some backend caching quirks.
            if etag.starts(with: "\"") {
                etag.insert(contentsOf: "W/", at: etag.startIndex)
            }
            restRequest.urlRequest.setValue(etag, forHTTPHeaderField: HTTPHeader.ifNoneMatch)
        }

        public mutating func setAuthorization(_ authorization: REST.Authorization) {
            let value: String
            switch authorization {
            case let .accountNumber(accountNumber):
                value = "Token \(accountNumber)"

            case let .accessToken(accessToken):
                value = "Bearer \(accessToken)"
            }

            restRequest.urlRequest.addValue(value, forHTTPHeaderField: HTTPHeader.authorization)
        }

        public func getRequest() -> REST.Request {
            return restRequest
        }
    }

    struct URLPathTemplate: ExpressibleByStringLiteral {
        private enum Component {
            case literal(String)
            case placeholder(String)
        }

        public enum Error: LocalizedError {
            /// Replacement value is not provided for placeholder.
            case noReplacement(_ name: String)

            /// Failure to perecent encode replacement value.
            case percentEncoding

            public var errorDescription: String? {
                switch self {
                case let .noReplacement(placeholder):
                    return "Replacement is not provided for \(placeholder)."

                case .percentEncoding:
                    return "Failed to percent encode replacement value."
                }
            }
        }

        private var components: [Component]
        private var replacements = [String: String]()

        public init(stringLiteral value: StringLiteralType) {
            let slashCharset = CharacterSet(charactersIn: "/")

            components = value.split(separator: "/").map { subpath -> Component in
                if subpath.hasPrefix("{"), subpath.hasSuffix("}") {
                    let name = String(subpath.dropFirst().dropLast())

                    return .placeholder(name)
                } else {
                    return .literal(
                        subpath.trimmingCharacters(in: slashCharset)
                    )
                }
            }
        }

        private init(components: [Component]) {
            self.components = components
        }

        mutating func addPercentEncodedReplacement(
            name: String,
            value: String,
            allowedCharacters: CharacterSet
        ) throws {
            let encoded = value.addingPercentEncoding(
                withAllowedCharacters: allowedCharacters
            )

            if let encoded = encoded {
                replacements[name] = encoded
            } else {
                throw Error.percentEncoding
            }
        }

        public var templateString: String {
            var combinedString = ""

            for component in components {
                combinedString += "/"

                switch component {
                case let .literal(string):
                    combinedString += string
                case let .placeholder(name):
                    combinedString += "{\(name)}"
                }
            }

            return combinedString
        }

        public func pathString() throws -> String {
            var combinedPath = ""

            for component in components {
                combinedPath += "/"

                switch component {
                case let .literal(string):
                    combinedPath += string

                case let .placeholder(name):
                    if let string = replacements[name] {
                        combinedPath += string
                    } else {
                        throw Error.noReplacement(name)
                    }
                }
            }

            return combinedPath
        }

        public static func + (lhs: URLPathTemplate, rhs: URLPathTemplate) -> URLPathTemplate {
            return URLPathTemplate(components: lhs.components + rhs.components)
        }
    }
}
