// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: MIT

//! PTY management: spawn a child process in a pseudo-terminal, send input
//! to it, and read output from it via a background reader thread.
//!
//! # The reader bug we avoid
//!
//! A naive implementation calls `try_clone_reader()` on every `read()`, which
//! creates multiple reader handles competing for the same PTY master data
//! stream — data is silently lost. (This was the fatal bug in
//! `ratatui-testlib`.)
//!
//! Our approach: clone the reader **once** in [`Pty::spawn`], then hand it to
//! a dedicated background thread that continuously reads and forwards each
//! chunk through an [`mpsc::Receiver`]. The main thread only drains the
//! channel — no PTY fd contention.

use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, ExitStatus, MasterPty, PtySize, native_pty_system};

use crate::error::{Error, Result};

/// A pseudo-terminal connected to a child process.
///
/// Created by [`Pty::spawn`]; use [`Pty::write`] to send input and
/// [`Pty::drain`] to collect output.
pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    /// Output chunks forwarded by the background reader thread.
    output_rx: mpsc::Receiver<Vec<u8>>,
    /// Keeps the reader thread alive for the lifetime of this struct.
    _reader_thread: thread::JoinHandle<()>,
}

impl Pty {
    /// Create a PTY with the given size and spawn a child process in it.
    pub fn spawn(cmd: CommandBuilder, cols: u16, rows: u16) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| Error::Pty(e.to_string()))?;

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| Error::Pty(e.to_string()))?;

        // Clone the reader ONCE — this is the only call to try_clone_reader.
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| Error::Pty(e.to_string()))?;
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let reader_thread = thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF — child closed stdout
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break; // receiver dropped — harness is gone
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });

        // Drop the slave so the only remaining reference is inside the child.
        // Without this the PTY never reports EOF when the child exits.
        drop(pair.slave);

        // Take the writer once — take_writer panics if called again.
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| Error::Pty(e.to_string()))?;

        Ok(Pty {
            master: pair.master,
            writer: Some(writer),
            child: Some(child),
            output_rx: rx,
            _reader_thread: reader_thread,
        })
    }

    /// Write bytes to the PTY master (the child reads them from its stdin).
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        let writer = self.writer.as_mut().ok_or(Error::ProcessExited)?;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    /// Collect all output chunks that the background thread has forwarded so
    /// far. Returns an empty vec if nothing is available (non-blocking).
    pub fn drain(&mut self) -> Vec<u8> {
        let mut buf = Vec::new();
        while let Ok(chunk) = self.output_rx.try_recv() {
            buf.extend_from_slice(&chunk);
        }
        buf
    }

    /// Check if the child process is still running.
    pub fn is_running(&mut self) -> bool {
        match &mut self.child {
            Some(child) => match child.try_wait() {
                Ok(None) => true,
                Ok(Some(_)) | Err(_) => false,
            },
            None => false,
        }
    }

    /// Block until the child exits and return its status.
    pub fn wait_exit(&mut self) -> Result<ExitStatus> {
        let child = self.child.as_mut().ok_or(Error::ProcessExited)?;
        Ok(child.wait()?)
    }

    /// Wait for the child to exit, but at most `timeout`.
    ///
    /// Polls `try_wait` so a stuck child can't hang the caller (a plain
    /// [`wait`](Self::wait_exit) blocks forever in that case). Returns the
    /// exit status if the child exited in time, `None` on timeout. The PTY
    /// master is kept open for the duration so the child can shut down
    /// cleanly (rather than being SIGHUP'd when it closes) — important when
    /// the child writes an LLVM coverage profile on exit, which is only
    /// finalized by a clean exit.
    pub fn wait_for_exit_timeout(&mut self, timeout: Duration) -> Option<ExitStatus> {
        let child = self.child.as_mut()?;
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if Instant::now() >= deadline => return None,
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(_) => return None,
            }
        }
    }

    /// Resize the PTY window.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| Error::Pty(e.to_string()))?;
        Ok(())
    }
}
