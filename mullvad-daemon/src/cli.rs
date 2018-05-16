use clap::{App, Arg};
use log;

use std::path::PathBuf;

use version;

pub struct Config {
    pub log_level: log::LevelFilter,
    pub log_file: Option<PathBuf>,
    pub tunnel_log_file: Option<PathBuf>,
    pub resource_dir: Option<PathBuf>,
    pub log_stdout_timestamps: bool,
    pub run_as_service: bool,
    pub register_service: bool,
}

pub fn get_config() -> Config {
    let app = create_app();
    let matches = app.get_matches();

    let log_level = match matches.occurrences_of("v") {
        0 => log::LevelFilter::Info,
        1 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };
    let log_file = matches.value_of_os("log_file").map(PathBuf::from);
    let tunnel_log_file = matches.value_of_os("tunnel_log_file").map(PathBuf::from);
    let resource_dir = matches.value_of_os("resource_dir").map(PathBuf::from);
    let log_stdout_timestamps = !matches.is_present("disable_stdout_timestamps");

    let run_as_service = cfg!(windows) && matches.is_present("run_as_service");
    let register_service = cfg!(windows) && matches.is_present("register_service");

    Config {
        log_level,
        log_file,
        tunnel_log_file,
        resource_dir,
        log_stdout_timestamps,
        run_as_service,
        register_service,
    }
}

fn create_app() -> App<'static, 'static> {
    let app = App::new(crate_name!())
        .version(version::current())
        .author(crate_authors!())
        .about(crate_description!())
        .arg(
            Arg::with_name("v")
                .short("v")
                .multiple(true)
                .help("Sets the level of verbosity."),
        )
        .arg(
            Arg::with_name("log_file")
                .long("log")
                .takes_value(true)
                .value_name("PATH")
                .help("Activates file logging to the given path."),
        )
        .arg(
            Arg::with_name("tunnel_log_file")
                .long("tunnel-log")
                .takes_value(true)
                .value_name("PATH")
                .help("Save log from tunnel implementation process to this file path."),
        )
        .arg(
            Arg::with_name("resource_dir")
                .long("resource-dir")
                .takes_value(true)
                .value_name("DIR")
                .help("Uses the given directory to read needed resources, such as certificates."),
        )
        .arg(
            Arg::with_name("disable_stdout_timestamps")
            .long("disable-stdout-timestamps")
            .help("Don't log timestamps when logging to stdout, useful when running as a systemd service")
            );

    if cfg!(windows) {
        app.arg(
            Arg::with_name("run_as_service")
                .long("run-as-service")
                .help("Run as a system service. On Windows this option must be used when running a system service"),
        ).arg(
            Arg::with_name("register_service")
                .long("register-service")
                .help("Register itself as a system service"),
        )
    } else {
        app
    }
}
