extern crate libc;
extern crate reopen;

use std::thread;
use std::io::{self, Read, Write};
use std::fs::{File, OpenOptions};
use std::sync::Mutex;
use std::path::Path;
use std::time::Duration;

use subprocess::{Exec, ExitStatus, Popen, PopenError, Redirection};

use self::reopen::Reopen;
use super::ProcessOption;


#[derive(Sync)]
pub struct CodeChainProcess {
    process: Mutex<Popen>,
}

impl CodeChainProcess {
    pub fn new(env: &str, args: &str, option: &ProcessOption) -> Self {

        let args_iter = args.split_whitespace();
        let args_vec: Vec<String> = args_iter.map(|str| str.to_string()).collect();

        let (p2p_port, rpc_port) = parse_ports(&args_vec);

        let envs = Self::parse_env(env)?;

        let mut file = Reopen::new(Box::new(|| {
            OpenOptions::new().append(true).create(true).open(option.log_file_path.clone())
        }))?;
        file.handle().register_signal(libc::SIGHUP).unwrap();

        let mut exec = if Path::new(&option.codechain_dir).join("codechain").exists() {
            Exec::cmd("./codechain")
                .cwd(option.codechain_dir.clone())
                .stdout(Redirection::Pipe)
                .stderr(Redirection::Merge)
                .args(&args_vec)
        } else {
            Exec::cmd("cargo")
                .arg("run")
                .arg("--")
                .cwd(option.codechain_dir.clone())
                .stdout(Redirection::Pipe)
                .stderr(Redirection::Merge)
                .args(&args_vec)
        };

        for (k, v) in envs {
            exec = exec.env(k, v);
        }

        let child = exec.popen()?;

        let process = CodeChainProcess {
            process: Mutex::new(child),
        };

        thread::Builder::new()
            .name("codechain_log_writer".to_string())
            .spawn(move || {
                let mut buf = [u8; 1024];
                loop {
                    let length = match process.read(&mut buf) {
                        Ok(length) => length,
                        Err(err) => {
                            cerror!(PROCESS, "Fail to read stdout of CodeChain : {}", err);
                            return
                        },
                    };

                    if let Err(error) = file.write_all(buf[0..length]) {
                        cerror!(PROCESS, "Fail to write stdout of CodeChain : {}", err);
                    }
                }
            })
            .expect("Should success running process thread");

        process
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        let process = self.process.lock().unwrap();
        process.stdout.expect("Process opened with pipe").read(buf)
    }

    pub fn is_running(&self) -> bool {
        let mut process = self.process.lock().unwrap();
        process.poll().is_none()
    }

    pub fn terminate(&self) -> Result<(), io::Error> {
        let mut process = self.process.lock().unwrap();
        process.terminate()
    }

    pub fn wait_timeout(&self, duration: Duration) -> Result<Option<ExitStatus>, PopenError> {
        let mut process = self.process.lock().unwrap();
        process.wait_timeout(duration)
    }
}

fn parse_ports(args: &[String]) -> (u16, u16) {
    let p2p_port = parse_port(args, "--port");
    let rpc_port = parse_port(args, "--jsonrpc-port");

    (p2p_port.unwrap_or(3485), rpc_port.unwrap_or(8080))
}

fn parse_port(args: &[String], option_name: &str) -> Option<u16> {
    let option_position = args.iter().position(|arg| arg == option_name);
    let interface_pos = option_position.map(|pos| pos + 1);
    let interface_string = interface_pos.and_then(|pos| args.get(pos));
    interface_string.and_then(|port| port.parse().ok())
}
