use std::{
    env,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{self, Child, Command},
    thread,
};

use anyhow::{Context, Ok};
use tracing::debug;

pub struct Server {
    process: Option<Child>,
    file: PathBuf,
}

impl Server {
    pub fn new() -> Self {
        let server_path = env::var("SERVER_PATH").expect("Failed to read SERVER_PATH env");
        let file = PathBuf::from(&server_path);

        Self {
            process: None,
            file,
        }
    }

    pub fn start(&mut self, dev: bool) -> anyhow::Result<()> {
        // If a server is already listening on the port, nothing to do.
        if std::net::TcpStream::connect("127.0.0.1:11470").is_ok() {
            tracing::info!("Server already running on port 11470, skipping start");
            return Ok(());
        }

        let is_js = self
            .file
            .extension()
            .map(|ext| ext == "js")
            .unwrap_or(false);

        let mut cmd = if is_js {
            let mut c = Command::new("node");
            c.arg(self.file.as_os_str());
            c
        } else {
            Command::new(self.file.as_os_str())
        };

        let mut child = cmd
            .env("NO_CORS", (dev as i32).to_string())
            .stdout(process::Stdio::piped())
            .spawn()
            .context("Failed to start server")?;

        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            thread::spawn(move || {
                while let Some(Result::Ok(line)) = lines.next() {
                    debug!(target: "server", "{}", line);
                }
            });
        }

        self.process = Some(child);

        Ok(())
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(mut process) = self.process.take() {
            process.kill().context("Failed to kill server process")?;
        }

        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop().expect("Failed to stop server");
    }
}
