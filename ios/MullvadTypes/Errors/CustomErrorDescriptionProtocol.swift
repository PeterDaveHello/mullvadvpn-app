//
//  CustomErrorDescription.swift
//  MullvadVPN
//
//  Created by pronebird on 23/09/2022.
//  Copyright © 2022 Mullvad VPN AB. All rights reserved.
//

import Foundation

/// A protocol providing error a way to override error description when printing error chain.
public protocol CustomErrorDescriptionProtocol {
    var customErrorDescription: String? { get }
}
