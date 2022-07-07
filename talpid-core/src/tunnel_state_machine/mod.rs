mod connected_state;
mod connecting_state;
mod disconnected_state;
mod disconnecting_state;
mod error_state;

use self::{
    connected_state::{ConnectedState, ConnectedStateBootstrap},
    connecting_state::ConnectingState,
    disconnected_state::DisconnectedState,
    disconnecting_state::{AfterDisconnect, DisconnectingState},
    error_state::ErrorState,
};
#[cfg(windows)]
use crate::split_tunnel;
use crate::{
    dns::DnsMonitor,
    firewall::{Firewall, FirewallArguments, InitialFirewallState},
    mpsc::Sender,
    offline,
    routing::RouteManager,
    tunnel::{tun_provider::TunProvider, Tunnel, TunnelEvent, TunnelMetadata},
};
#[cfg(windows)]
use std::ffi::OsString;

use futures::{
    channel::{mpsc, oneshot},
    stream, StreamExt,
};
#[cfg(target_os = "android")]
use std::os::unix::io::RawFd;
use std::{
    collections::HashSet,
    future::Future,
    io,
    net::IpAddr,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
#[cfg(target_os = "android")]
use talpid_types::{android::AndroidContext, ErrorExt};
use talpid_types::{
    net::{AllowedEndpoint, AllowedTunnelTraffic, TunnelParameters},
    tunnel::{ActionAfterDisconnect, ErrorStateCause, ParameterGenerationError},
};

const TUNNEL_STATE_MACHINE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Event emitted from the states in `talpid_core::tunnel_state_machine` when the tunnel state
/// machine enters a new state.
#[derive(Clone, Debug, PartialEq)]
pub enum TunnelStateTransition<T: Tunnel> {
    /// No connection is established and network is unsecured.
    Disconnected,
    /// Network is secured but tunnel is still connecting.
    Connecting(T::TunnelEvent),
    /// Tunnel is connected.
    Connected(T::TunnelEvent),
    /// Disconnecting tunnel.
    Disconnecting(ActionAfterDisconnect),
    /// Tunnel is disconnected but usually secured by blocking all connections.
    Error(talpid_types::tunnel::ErrorState<T::Error>),
}

/// Errors that can happen when setting up or using the state machine.
#[derive(err_derive::Error, Debug)]
pub enum Error {
    /// Unable to spawn offline state monitor
    #[error(display = "Unable to spawn offline state monitor")]
    OfflineMonitorError(#[error(source)] crate::offline::Error),

    /// Unable to set up split tunneling
    #[cfg(target_os = "windows")]
    #[error(display = "Failed to initialize split tunneling")]
    InitSplitTunneling(#[error(source)] split_tunnel::Error),

    /// Failed to initialize the system firewall integration.
    #[error(display = "Failed to initialize the system firewall integration")]
    InitFirewallError(#[error(source)] crate::firewall::Error),

    /// Failed to initialize the system DNS manager and monitor.
    #[error(display = "Failed to initialize the system DNS manager and monitor")]
    InitDnsMonitorError(#[error(source)] crate::dns::Error),

    /// Failed to initialize the route manager.
    #[error(display = "Failed to initialize the route manager")]
    InitRouteManagerError(#[error(source)] crate::routing::Error),

    /// Failed to initialize filtering resolver
    #[cfg(target_os = "macos")]
    #[error(display = "Failed to initialize filtering resolver")]
    InitFilteringResolver(#[error(source)] crate::resolver::Error),

    /// Failed to initialize tunnel state machine event loop executor
    #[error(display = "Failed to initialize tunnel state machine event loop executor")]
    ReactorError(#[error(source)] io::Error),

    /// Failed to send state change event to listener
    #[error(display = "Failed to send state change event to listener")]
    SendStateChange,
}

/// Settings used to initialize the tunnel state machine.
pub struct InitialTunnelState {
    /// Whether to allow LAN traffic when not in the (non-blocking) disconnected state.
    pub allow_lan: bool,
    /// Block traffic unless connected to the VPN.
    pub block_when_disconnected: bool,
    /// DNS servers to use. If `None`, the tunnel gateway is used.
    pub dns_servers: Option<Vec<IpAddr>>,
    /// A single endpoint that is allowed to communicate outside the tunnel, i.e.
    /// in any of the blocking states.
    pub allowed_endpoint: AllowedEndpoint,
    /// Whether to reset any existing firewall rules when initializing the disconnected state.
    pub reset_firewall: bool,
    /// Programs to exclude from the tunnel using the split tunnel driver.
    #[cfg(windows)]
    pub exclude_paths: Vec<OsString>,
}

/// Spawn the tunnel state machine thread, returning a channel for sending tunnel commands.
pub async fn spawn<T: Tunnel + 'static>(
    initial_settings: InitialTunnelState,
    tunnel_parameters_generator: impl TunnelParametersGenerator,
    log_dir: Option<PathBuf>,
    resource_dir: PathBuf,
    state_change_listener: impl Sender<TunnelStateTransition<T>> + Send + 'static,
    offline_state_listener: mpsc::UnboundedSender<bool>,
    tunnel: T,
    #[cfg(target_os = "windows")] volume_update_rx: mpsc::UnboundedReceiver<()>,
    #[cfg(target_os = "macos")] exclusion_gid: u32,
    #[cfg(target_os = "android")] android_context: AndroidContext,
) -> Result<TunnelStateMachineHandle<T>, Error> {
    let (command_tx, command_rx) = mpsc::unbounded();
    let command_tx = Arc::new(command_tx);

    let tun_provider = TunProvider::new(
        #[cfg(target_os = "android")]
        android_context.clone(),
        #[cfg(target_os = "android")]
        initial_settings.allow_lan,
        #[cfg(target_os = "android")]
        initial_settings.dns_servers.clone(),
    );

    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let weak_command_tx = Arc::downgrade(&command_tx);
    let state_machine = TunnelStateMachine::new(
        initial_settings,
        weak_command_tx,
        offline_state_listener,
        tunnel_parameters_generator,
        tun_provider,
        log_dir,
        resource_dir,
        command_rx,
        tunnel,
        #[cfg(target_os = "windows")]
        volume_update_rx,
        #[cfg(target_os = "macos")]
        exclusion_gid,
        #[cfg(target_os = "android")]
        android_context,
    )
    .await?;

    #[cfg(windows)]
    let split_tunnel = state_machine.shared_values.split_tunnel.handle();

    tokio::task::spawn_blocking(move || {
        state_machine.run(state_change_listener);
        if shutdown_tx.send(()).is_err() {
            log::error!("Can't send shutdown completion to daemon");
        }
    });

    Ok(TunnelStateMachineHandle {
        command_tx,
        shutdown_rx,
        #[cfg(windows)]
        split_tunnel,
    })
}

/// Representation of external commands for the tunnel state machine.
pub enum TunnelCommand<T: Tunnel> {
    /// Enable or disable LAN access in the firewall.
    AllowLan(bool),
    /// Endpoint that should never be blocked. `()` is sent to the
    /// channel after attempting to set the firewall policy, regardless
    /// of whether it succeeded.
    AllowEndpoint(AllowedEndpoint, oneshot::Sender<()>),
    /// Set DNS servers to use.
    Dns(Option<Vec<IpAddr>>),
    /// Enable or disable the block_when_disconnected feature.
    BlockWhenDisconnected(bool),
    /// Notify the state machine of the connectivity of the device.
    IsOffline(bool),
    /// Open tunnel connection.
    Connect,
    /// Close tunnel connection.
    Disconnect,
    /// Disconnect any open tunnel and block all network access
    Block(ErrorStateCause<T::Error>),
    /// Bypass a socket, allowing traffic to flow through outside the tunnel.
    #[cfg(target_os = "android")]
    BypassSocket(RawFd, oneshot::Sender<()>),
    /// Set applications that are allowed to send and receive traffic outside of the tunnel.
    #[cfg(windows)]
    SetExcludedApps(
        oneshot::Sender<Result<(), split_tunnel::Error>>,
        Vec<OsString>,
    ),
}

type TunnelCommandReceiver<T> = stream::Fuse<mpsc::UnboundedReceiver<TunnelCommand<T>>>;

enum EventResult<T: Tunnel> {
    Command(Option<TunnelCommand<T>>),
    Event(Option<(TunnelEvent<T>, oneshot::Sender<()>)>),
    Close(Result<Option<ErrorStateCause<T::Error>>, oneshot::Canceled>),
}

/// Asynchronous handling of the tunnel state machine.
///
/// This type implements `Stream`, and attempts to advance the state machine based on the events
/// received on the commands stream and possibly on events that specific states are also listening
/// to. Every time it successfully advances the state machine a `TunnelStateTransition` is emitted
/// by the stream.
struct TunnelStateMachine<T: Tunnel> {
    current_state: Option<TunnelStateWrapper<T>>,
    commands: TunnelCommandReceiver<T>,
    shared_values: SharedTunnelStateValues<T>,
}

impl<T: Tunnel + 'static> TunnelStateMachine<T> {
    async fn new(
        settings: InitialTunnelState,
        command_tx: std::sync::Weak<mpsc::UnboundedSender<TunnelCommand<T>>>,
        offline_state_tx: mpsc::UnboundedSender<bool>,
        tunnel_parameters_generator: impl TunnelParametersGenerator,
        tun_provider: TunProvider,
        log_dir: Option<PathBuf>,
        resource_dir: PathBuf,
        commands_rx: mpsc::UnboundedReceiver<TunnelCommand<T>>,
        tunnel: T,
        #[cfg(target_os = "windows")] volume_update_rx: mpsc::UnboundedReceiver<()>,
        #[cfg(target_os = "macos")] exclusion_gid: u32,
        #[cfg(target_os = "android")] android_context: AndroidContext,
    ) -> Result<Self, Error> {
        let runtime = tokio::runtime::Handle::current();

        #[cfg(target_os = "macos")]
        let filtering_resolver = crate::resolver::start_resolver().await?;

        #[cfg(target_os = "windows")]
        let power_mgmt_rx = crate::windows::window::PowerManagementListener::new();

        #[cfg(windows)]
        let split_tunnel =
            split_tunnel::SplitTunnel::new(runtime.clone(), command_tx.clone(), volume_update_rx)
                .map_err(Error::InitSplitTunneling)?;

        let args = FirewallArguments {
            initial_state: if settings.block_when_disconnected || !settings.reset_firewall {
                InitialFirewallState::Blocked(settings.allowed_endpoint.clone())
            } else {
                InitialFirewallState::None
            },
            allow_lan: settings.allow_lan,
        };

        let firewall = Firewall::from_args(args).map_err(Error::InitFirewallError)?;
        let route_manager = RouteManager::new(HashSet::new())
            .await
            .map_err(Error::InitRouteManagerError)?;
        let dns_monitor = DnsMonitor::new(
            #[cfg(target_os = "linux")]
            runtime.clone(),
            #[cfg(target_os = "linux")]
            route_manager
                .handle()
                .map_err(Error::InitRouteManagerError)?,
            #[cfg(target_os = "macos")]
            command_tx.clone(),
        )
        .map_err(Error::InitDnsMonitorError)?;

        let (offline_tx, mut offline_rx) = mpsc::unbounded();
        let initial_offline_state_tx = offline_state_tx.clone();
        tokio::spawn(async move {
            while let Some(offline) = offline_rx.next().await {
                if let Some(tx) = command_tx.upgrade() {
                    let _ = tx.unbounded_send(TunnelCommand::IsOffline(offline));
                } else {
                    break;
                }
                let _ = offline_state_tx.unbounded_send(offline);
            }
        });
        let mut offline_monitor = offline::spawn_monitor(
            offline_tx,
            #[cfg(target_os = "linux")]
            route_manager
                .handle()
                .map_err(Error::InitRouteManagerError)?,
            #[cfg(target_os = "android")]
            android_context,
            #[cfg(target_os = "windows")]
            power_mgmt_rx,
        )
        .await
        .map_err(Error::OfflineMonitorError)?;
        let is_offline = offline_monitor.is_offline().await;
        let _ = initial_offline_state_tx.unbounded_send(is_offline);

        #[cfg(windows)]
        split_tunnel
            .set_paths_sync(&settings.exclude_paths)
            .map_err(Error::InitSplitTunneling)?;

        let mut shared_values = SharedTunnelStateValues {
            #[cfg(windows)]
            split_tunnel,
            runtime,
            firewall,
            dns_monitor,
            route_manager,
            _offline_monitor: offline_monitor,
            allow_lan: settings.allow_lan,
            block_when_disconnected: settings.block_when_disconnected,
            is_offline,
            dns_servers: settings.dns_servers,
            allowed_endpoint: settings.allowed_endpoint,
            tunnel_parameters_generator: Box::new(tunnel_parameters_generator),
            tun_provider: Arc::new(Mutex::new(tun_provider)),
            log_dir,
            resource_dir,
            // TODO: receive the tunnel_provider from the parameters
            tunnel_provider: tunnel,
            #[cfg(target_os = "linux")]
            connectivity_check_was_enabled: None,
            #[cfg(target_os = "macos")]
            filtering_resolver,
            #[cfg(target_os = "macos")]
            _exclusion_gid: exclusion_gid,
        };

        tokio::task::spawn_blocking(move || {
            let (initial_state, _) =
                DisconnectedState::enter(&mut shared_values, settings.reset_firewall);

            Ok(TunnelStateMachine {
                current_state: Some(initial_state),
                commands: commands_rx.fuse(),
                shared_values,
            })
        })
        .await
        .unwrap()
    }

    fn run(mut self, change_listener: impl Sender<TunnelStateTransition<T>> + Send + 'static) {
        use EventConsequence::*;

        let runtime = self.shared_values.runtime.clone();

        while let Some(state_wrapper) = self.current_state.take() {
            match state_wrapper.handle_event(&runtime, &mut self.commands, &mut self.shared_values)
            {
                NewState((state, transition)) => {
                    self.current_state = Some(state);

                    if let Err(error) = change_listener
                        .send(transition)
                        .map_err(|_| Error::SendStateChange)
                    {
                        log::error!("{}", error);
                        break;
                    }
                }
                SameState(state) => {
                    self.current_state = Some(state);
                }
                Finished => (),
            }
        }

        log::debug!("Exiting tunnel state machine loop");
    }
}

/// Trait for any type that can provide a stream of `TunnelParameters` to the `TunnelStateMachine`.
pub trait TunnelParametersGenerator: Send + 'static {
    /// Given the number of consecutive failed retry attempts, it should yield a `TunnelParameters`
    /// to establish a tunnel with.
    /// If this returns `None` then the state machine goes into the `Error` state.
    fn generate(
        &mut self,
        retry_attempt: u32,
    ) -> Pin<Box<dyn Future<Output = Result<TunnelParameters, ParameterGenerationError>>>>;
}

pub struct TunnelStateTx<T: Tunnel> {
    tx: mpsc::Sender<T::TunnelEvent>,
}

impl<T: Tunnel> TunnelStateTx<T> {
    fn new() -> (mpsc::Receiver<T::TunnelEvent>, Self) {
        let (tx, rx) = mpsc::channel(0);
        (rx, Self { tx })
    }
}

/// Values that are common to all tunnel states.
struct SharedTunnelStateValues<T: Tunnel> {
    tunnel_provider: T,
    /// Management of excluded apps.
    /// This object should be dropped before deinitializing WinFw (dropping the `Firewall`
    /// instance), since the driver may add filters to the same sublayer.
    #[cfg(windows)]
    split_tunnel: split_tunnel::SplitTunnel,
    runtime: tokio::runtime::Handle,
    firewall: Firewall,
    dns_monitor: DnsMonitor,
    route_manager: RouteManager,
    _offline_monitor: offline::MonitorHandle,
    /// Should LAN access be allowed outside the tunnel.
    allow_lan: bool,
    /// Should network access be allowed when in the disconnected state.
    block_when_disconnected: bool,
    /// True when the computer is known to be offline.
    is_offline: bool,
    /// DNS servers to use (overriding default).
    dns_servers: Option<Vec<IpAddr>>,
    /// Endpoint that should not be blocked by the firewall.
    allowed_endpoint: AllowedEndpoint,
    /// The generator of new `TunnelParameter`s
    tunnel_parameters_generator: Box<dyn TunnelParametersGenerator>,
    /// The provider of tunnel devices.
    tun_provider: Arc<Mutex<TunProvider>>,
    /// Directory to store tunnel log file.
    log_dir: Option<PathBuf>,
    /// Resource directory path.
    resource_dir: PathBuf,

    /// NetworkManager's connecitivity check state.
    #[cfg(target_os = "linux")]
    connectivity_check_was_enabled: Option<bool>,

    /// Filtering resolver handle
    #[cfg(target_os = "macos")]
    filtering_resolver: crate::resolver::ResolverHandle,

    /// Exclusion GID
    #[cfg(target_os = "macos")]
    _exclusion_gid: u32,
}

impl<T: Tunnel> SharedTunnelStateValues<T> {
    // TODO: remove dummy function
    pub fn get_tunnel_endpoint(&self) -> T::TunnelEvent {
        unimplemented!()
    }

    pub fn set_allow_lan(&mut self, allow_lan: bool) -> Result<(), ErrorStateCause<T::Error>> {
        if self.allow_lan != allow_lan {
            self.allow_lan = allow_lan;

            #[cfg(target_os = "android")]
            {
                if let Err(error) = self.tun_provider.lock().unwrap().set_allow_lan(allow_lan) {
                    log::error!(
                        "{}",
                        error.display_chain_with_msg(&format!(
                            "Failed to restart tunnel after {} LAN connections",
                            if allow_lan { "allowing" } else { "blocking" }
                        ))
                    );
                    return Err(ErrorStateCause::StartTunnelError);
                }
            }
        }

        Ok(())
    }

    pub fn set_dns_servers(
        &mut self,
        dns_servers: Option<Vec<IpAddr>>,
    ) -> Result<bool, ErrorStateCause<T::Error>> {
        if self.dns_servers != dns_servers {
            self.dns_servers = dns_servers;

            #[cfg(target_os = "android")]
            {
                if let Err(error) = self
                    .tun_provider
                    .lock()
                    .unwrap()
                    .set_dns_servers(self.dns_servers.clone())
                {
                    log::error!(
                        "{}",
                        error.display_chain_with_msg(
                            "Failed to restart tunnel after changing DNS servers",
                        )
                    );
                    return Err(ErrorStateCause::StartTunnelError);
                }
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// NetworkManager's connectivity check can get hung when DNS requests fail, thus the TSM
    /// should always disable it before applying firewall rules. The connectivity check should be
    /// reset whenever the firewall is cleared.
    #[cfg(target_os = "linux")]
    pub fn disable_connectivity_check(&mut self) {
        if self.connectivity_check_was_enabled.is_none() {
            if let Ok(nm) = talpid_dbus::network_manager::NetworkManager::new() {
                self.connectivity_check_was_enabled = nm.disable_connectivity_check();
            }
        } else {
            log::trace!("Daemon already disabled connectivity check");
        }
    }

    /// Reset NetworkManager's connectivity check if it was disabled.
    #[cfg(target_os = "linux")]
    pub fn reset_connectivity_check(&mut self) {
        if self.connectivity_check_was_enabled.take() == Some(true) {
            if let Ok(nm) = talpid_dbus::network_manager::NetworkManager::new() {
                nm.enable_connectivity_check();
            }
        } else {
            log::trace!("Connectivity check wasn't disabled by the daemon");
        }
    }

    #[cfg(target_os = "android")]
    pub fn bypass_socket(&mut self, fd: RawFd, tx: oneshot::Sender<()>) {
        if let Err(err) = self.tun_provider.lock().unwrap().bypass(fd) {
            log::error!("Failed to bypass socket {}", err);
        }
        let _ = tx.send(());
    }
}

/// Asynchronous result of an attempt to progress a state.
enum EventConsequence<T>
where
    T: Tunnel,
{
    /// Transition to a new state.
    NewState((TunnelStateWrapper<T>, TunnelStateTransition<T>)),
    /// An event was received, but it was ignored by the state so no transition is performed.
    SameState(TunnelStateWrapper<T>),
    /// The state machine has finished its execution.
    Finished,
}

/// Trait that contains the method all states should implement to handle an event and advance the
/// state machine.
trait TunnelState<T: Tunnel>: Sized {
    /// Type representing extra information required for entering the state.
    type Bootstrap;

    /// Constructor function.
    ///
    /// This is the state entry point. It attempts to enter the state, and may fail by entering an
    /// error or fallback state instead.
    fn enter(
        shared_values: &mut SharedTunnelStateValues<T>,
        bootstrap: Self::Bootstrap,
    ) -> (TunnelStateWrapper<T>, TunnelStateTransition<T>);

    /// Main state function.
    ///
    /// This is state exit point. It consumes itself and returns the next state to advance to when
    /// it has completed, or itself if it wants to ignore a received event or if no events were
    /// ready to be received. See [`EventConsequence`] for more details.
    ///
    /// An implementation can handle events from many sources, but it should also handle command
    /// events received through the provided `commands` stream.
    ///
    /// [`EventConsequence`]: enum.EventConsequence.html
    fn handle_event(
        self,
        runtime: &tokio::runtime::Handle,
        commands: &mut TunnelCommandReceiver<T>,
        shared_values: &mut SharedTunnelStateValues<T>,
    ) -> EventConsequence<T>;
}

// state_wrapper! {
//     enum TunnelStateWrapper<T> {
//         Disconnected(DisconnectedState),
//         Connecting(ConnectingState),
//         Connected(ConnectedState),
//         Disconnecting(DisconnectingState),
//         Error(ErrorState<T>),
//     }
// }

enum TunnelStateWrapper<T: Tunnel> {
    Disconnected(DisconnectedState),
    Connecting(ConnectingState<T>),
    Connected(ConnectedState<T>),
    Disconnecting(DisconnectingState<T>),
    Error(ErrorState<T>),
}

impl<T: Tunnel + 'static> TunnelStateWrapper<T> {
    fn handle_event(
        self,
        runtime: &tokio::runtime::Handle,
        commands: &mut TunnelCommandReceiver<T>,
        shared_values: &mut SharedTunnelStateValues<T>,
    ) -> EventConsequence<T> {
        match self {
            Self::Disconnected(state) => state.handle_event(runtime, commands, shared_values),

            Self::Connecting(state) => state.handle_event(runtime, commands, shared_values),

            Self::Connected(state) => state.handle_event(runtime, commands, shared_values),

            Self::Disconnecting(state) => state.handle_event(runtime, commands, shared_values),

            Self::Error(state) => state.handle_event(runtime, commands, shared_values),
        }
    }
}

/// Handle used to control the tunnel state machine.
pub struct TunnelStateMachineHandle<T: Tunnel> {
    command_tx: Arc<mpsc::UnboundedSender<TunnelCommand<T>>>,
    shutdown_rx: oneshot::Receiver<()>,
    #[cfg(windows)]
    split_tunnel: split_tunnel::SplitTunnelHandle,
}

impl<T: Tunnel> TunnelStateMachineHandle<T> {
    /// Waits for the tunnel state machine to shut down.
    /// This may fail after a timeout of `TUNNEL_STATE_MACHINE_SHUTDOWN_TIMEOUT`.
    pub async fn try_join(self) {
        drop(self.command_tx);

        match tokio::time::timeout(TUNNEL_STATE_MACHINE_SHUTDOWN_TIMEOUT, self.shutdown_rx).await {
            Ok(_) => log::info!("Tunnel state machine shut down"),
            Err(_) => log::error!("Tunnel state machine did not shut down gracefully"),
        }
    }

    /// Returns tunnel command sender.
    pub fn command_tx(&self) -> &Arc<mpsc::UnboundedSender<TunnelCommand<T>>> {
        &self.command_tx
    }

    /// Returns split tunnel object handle.
    #[cfg(windows)]
    pub fn split_tunnel(&self) -> &split_tunnel::SplitTunnelHandle {
        &self.split_tunnel
    }
}
