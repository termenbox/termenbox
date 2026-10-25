use std::io::{Read, Write};

use anyhow::{anyhow, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc;

/// A workspace-attach client (e.g. `tmux attach`) running in a PTY. A multiplexer repaints the whole
/// screen when a client attaches, so re-attaching after a dropped connection is also the screen
/// replay. Dropping this detaches; the workspace itself keeps running.
pub struct Attachment {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    pub rx: mpsc::Receiver<Vec<u8>>,
}

impl Attachment {
    /// Runs `program args` in a PTY. Backends decide the command (e.g. `tmux attach-session -t =name`).
    pub fn spawn(program: &str, args: &[String], cols: u16, rows: u16) -> Result<Self> {
        let pair = native_pty_system().openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.env("TERM", "xterm-256color");
        for (k, v) in crate::locale::utf8_env() {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        // Bounded: a slow client back-pressures the PTY instead of growing memory (512MB budget).
        let (tx, rx) = mpsc::channel(64);
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            master: pair.master,
            writer,
            child,
            rx,
        })
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow!("resize failed: {e}"))
    }
}

impl Drop for Attachment {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{TmuxBackend, WorkspaceBackend};
    use std::time::Duration;

    fn tmux_available() -> bool {
        std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    async fn read_until(att: &mut Attachment, needle: &str) -> bool {
        let mut seen = String::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if let Ok(Some(chunk)) =
                tokio::time::timeout(Duration::from_millis(500), att.rx.recv()).await
            {
                seen.push_str(&String::from_utf8_lossy(&chunk));
                if seen.contains(needle) {
                    return true;
                }
            }
        }
        false
    }

    #[tokio::test]
    async fn attach_types_and_survives_reattach() {
        if !tmux_available() {
            eprintln!("tmux not installed; skipping");
            return;
        }
        let backend = TmuxBackend::default();
        let name = format!("tt{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        backend.create(&name).await.unwrap();

        let mut a = backend.attach(&name, 80, 24).unwrap();
        a.write(b"echo marker$((20+22))\n").unwrap();
        assert!(
            read_until(&mut a, "marker42").await,
            "typed command output arrives"
        );
        drop(a); // connection dropped: detach only

        assert!(
            backend.list().await.unwrap().iter().any(|s| s.id == name),
            "workspace survives detach"
        );

        let mut b = backend.attach(&name, 80, 24).unwrap();
        assert!(
            read_until(&mut b, "marker42").await,
            "reattach repaints earlier output"
        );
        drop(b);

        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", &name])
            .output();
    }
}
