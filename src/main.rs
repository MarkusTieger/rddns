extern crate tokio;
extern crate hyper;
extern crate hyper_rustls;
extern crate futures;
extern crate base64;
extern crate pnet;
extern crate ipnetwork;

#[macro_use]
extern crate serde_derive;
extern crate toml;
extern crate regex;
#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate log;
extern crate simplelog;

#[macro_use]
extern crate clap;


mod command_line;
mod server;
mod config;
mod resolver;
mod update_executer;
mod updater;
mod basic_auth_header;

use std::{time::Duration};
use tokio::runtime::Runtime;
use tokio::time::interval;
use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use std::collections::HashMap;
use std::net::IpAddr;

use simplelog::{SimpleLogger, TermLogger, CombinedLogger, LevelFilter, Config as SimpleLogConfig};

use config::{read_config, Config, Trigger};
use command_line::{ExecutionMode, parse_command_line};
use updater::Updater;
use server::create_server;

fn main() -> Result<(), String> {
    init_logging();

    let cmd_args = parse_command_line();

    let config = read_config(&cmd_args.config_file).map_err(|err| err.to_string())?;

    let rt = Runtime::new().unwrap();

    match cmd_args.execution_mode {
        ExecutionMode::TRIGGER => {
            if config.triggers.is_empty() {
                return Err("In trigger mode at least one trigger must be configured.".to_string())
            }
            let triggers = config.triggers.clone();
            let jobs = triggers.into_iter().map(move |trigger| create_trigger_future(trigger, config.clone()))
                .collect::<FuturesUnordered<_>>() .collect::<Vec<_>>();
            let result = rt.block_on(jobs);
            combine_errors(result)
        },
        ExecutionMode::UPDATE => {
            let updater = Updater::new(config.clone());
            rt.block_on(updater.do_update(cmd_args.addresses))
        }
    }
}

fn init_logging() {
    let term_logger = TermLogger::new(LevelFilter::Info, SimpleLogConfig::default());
    let logger = if term_logger.is_some() {
        CombinedLogger::init(vec![term_logger.unwrap()])
    } else {
        SimpleLogger::init(LevelFilter::Info, SimpleLogConfig::default())
    };
    if logger.is_err() {
        eprintln!("Failed to initialize logging framework. Nothing will be logged. Error was: {}", logger.unwrap_err());
    }
}


async fn create_trigger_future(trigger: Trigger, config: Config) -> Result<(), String> {
    lazy_static! {
        static ref EMPTY: HashMap<String, IpAddr> = HashMap::new();
    }
    let updater = Updater::new(config.clone());
    match trigger {
        Trigger::HTTP(server) => {
            create_server( move |addr| {
                let updater = updater.clone();
                async move {
                    updater.do_update(addr).await
                }
            }, server.clone()).await
        },
        Trigger::TIMED(timed) => {
            let mut timer = interval(Duration::from_secs(timed.interval as u64));
            loop {
                timer.tick().await;
                updater.do_update(EMPTY.clone()).await.unwrap();
            }
        }
    }
}


fn combine_errors(results: Vec<Result<(), String>>) -> Result<(), String> {
    let error = results.into_iter().filter(|res| res.is_err()).map(|res| res.unwrap_err()).collect::<Vec<_>>().join("\n");

    if error.is_empty() || error == "\n" {
        Ok(())
    } else {
        Err(error.to_string())
    }
}