package net.mullvad.core.model

import android.os.Parcelable
import kotlinx.parcelize.Parcelize

@Parcelize
data class Settings(
    val relaySettings: RelaySettings,
    val allowLan: Boolean,
    val autoConnect: Boolean,
    val tunnelOptions: TunnelOptions,
    val showBetaReleases: Boolean
) : Parcelable
