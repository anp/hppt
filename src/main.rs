#[cfg(test)]
#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate log;
#[macro_use]
extern crate mioco;

extern crate chrono;
extern crate clap;
extern crate env_logger;

mod error;
mod files;
mod request;
mod response;
mod server;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::mpsc;

use chrono::Local;
use clap::{App, Arg};
use env_logger::LogBuilder;
use log::{LogLevelFilter, LogRecord};

fn main() {
    let args = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(env!("CARGO_PKG_DESCRIPTION"))

        .arg(Arg::with_name("SERVER_ROOT")
            .takes_value(true)
            .index(1)
            .help("Root directory from which to serve files.")
            .required(true)
            .validator(|s| {
                let p = PathBuf::from(&s);

                if p.exists() && p.is_dir() {
                    Ok(())
                } else if p.exists() {
                    Err(format!("{} exists but is not a directory.", s))
                } else {
                    Err(format!("{} does not exist.", s))
                }
            }))

        .arg(Arg::with_name("LISTEN_ADDRESS")
            .takes_value(true)
            .index(2)
            .help("Address and port to listen on.")
            .default_value("127.0.0.1:8080")
            .required(true)
            .validator(|s| SocketAddr::from_str(&s).map(|_| ()).map_err(|e| format!("{:?}", e))))

        .arg(Arg::with_name("NUM_THREADS")
            .takes_value(true)
            .long("threads")
            .help("Number of threads to use for listening to requests.")
            .required(true)
            .default_value("1")
            .validator(|s| s.parse::<server::NThreads>().map(|_| ()).map_err(|e| format!("{:?}", e))))

        .arg(Arg::with_name("VERBOSE")
            .short("v")
            .long("verbose")
            .help("Enable debug-level logging."))

        .get_matches();

    init_logging(args.is_present("VERBOSE"));

    // these have already been validated by the clap validators, and are required arguments
    let num_threads = args.value_of("NUM_THREADS").unwrap().parse::<server::NThreads>().unwrap();

    let listen_addr = SocketAddr::from_str(&args.value_of("LISTEN_ADDRESS").unwrap()).unwrap();

    let content_dir = PathBuf::from(&args.value_of("SERVER_ROOT").unwrap());

    let (_, recv) = mpsc::channel();

    // will block until exited or until shutdown queue is filled with num_threads items
    match server::run(listen_addr, &content_dir, recv, num_threads) {
        Ok(()) => (),
        Err(why) => error!("Error running server: {:?}", why),
    }
}

pub fn init_logging(verbose: bool) {
    let level = if verbose {
        LogLevelFilter::Debug
    } else {
        LogLevelFilter::Info
    };

    let init_result = LogBuilder::new().filter(None, level)
        .format(|record: &LogRecord| {
            format!("[{} {} {}] {}",
                    record.level(),
                    Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                    record.location().module_path(),
                    record.args())
        })
        .init();

    match init_result {
        Ok(_) => debug!("Initialized logging."),
        Err(why) => println!("Unable to initialize logging: {:?}", why),
    }
}
