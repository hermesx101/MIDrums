//! MIDI input backend.
//!
//! Wraps midir to enumerate, connect to and monitor MIDI input devices. All
//! real-time work (receiving a callback, parsing bytes, stamping a timestamp)
//! happens on the OS MIDI thread inside the callback; parsed events are pushed
//! onto a thread-safe channel and drained later by the GUI. The backend does
//! no heavy work on the GUI thread, only an occasional, inexpensive device
//! re-enumeration used for disconnection detection.

use std::fmt;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use midir::{MidiInput, MidiInputConnection};

use super::event::MidiEvent;
use super::parser::MidiParser;
use crate::time::SharedClock;

/// Thread-safe callback invoked on the MIDI thread after a batch of events is
/// enqueued. The GUI wires this to `egui::Context::request_repaint()` so newly
/// received events trigger an immediate repaint rather than waiting for user
/// input. Contains no egui types, keeping `drums-core` front-end agnostic.
pub type RepaintFn = Arc<dyn Fn() + Send + Sync>;

/// Description of an available MIDI input port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortInfo {
    pub index: usize,
    pub name: String,
}

/// Error type for the MIDI backend.
#[derive(Clone, Debug)]
pub enum MidiError {
    /// midir could not initialise (no MIDI support available).
    Init(String),
    /// The requested port index does not exist.
    PortNotFound { index: usize, available: usize },
    /// Could not read a port's name (commonly because it was just unplugged).
    PortInfo(String),
    /// Could not open / connect to the port.
    Connect(String),
}

impl fmt::Display for MidiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MidiError::Init(e) => write!(f, "MIDI initialisation failed: {e}"),
            MidiError::PortNotFound { index, available } => {
                write!(
                    f,
                    "MIDI port {index} is no longer available (only {available} present)"
                )
            }
            MidiError::PortInfo(e) => write!(f, "failed to read MIDI port info: {e}"),
            MidiError::Connect(e) => write!(f, "failed to connect to MIDI device: {e}"),
        }
    }
}

impl std::error::Error for MidiError {}

/// State owned (and mutated) by the MIDI callback thread.
struct CallbackState {
    sender: Sender<MidiEvent>,
    parser: MidiParser,
    clock: SharedClock,
    repaint: Option<RepaintFn>,
}

/// Holds an open connection together with the port's known name.
struct ConnectionInfo {
    _connection: MidiInputConnection<CallbackState>,
    port_name: String,
}

/// High-level manager over MIDI input devices and the event channel.
pub struct MidiBackend {
    clock: SharedClock,
    connection: Option<ConnectionInfo>,
    events_rx: Option<Receiver<MidiEvent>>,
    repaint: Option<RepaintFn>,
}

impl MidiBackend {
    pub fn new(clock: SharedClock) -> Self {
        Self {
            clock,
            connection: None,
            events_rx: None,
            repaint: None,
        }
    }

    /// True while a device connection is open.
    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }

    /// Name of the currently connected port, if any.
    pub fn connected_port_name(&self) -> Option<&str> {
        self.connection.as_ref().map(|c| c.port_name.as_str())
    }

    /// Enumerates all currently available MIDI input ports.
    pub fn list_ports(&self) -> Result<Vec<PortInfo>, MidiError> {
        let input = MidiInput::new("drums-app").map_err(|e| MidiError::Init(e.to_string()))?;
        let ports = input.ports();
        let mut out = Vec::with_capacity(ports.len());
        for (index, port) in ports.iter().enumerate() {
            let name = input
                .port_name(port)
                .map_err(|e| MidiError::PortInfo(e.to_string()))?;
            out.push(PortInfo { index, name });
        }
        Ok(out)
    }
    /// Opens a connection to the port at `port_index` and starts listening.
    ///
    /// Subsequent MIDI events are queued onto an internal channel; drain them
    /// with [`Self::poll_events`]. On success returns the connected name.
    pub fn connect(&mut self, port_index: usize) -> Result<String, MidiError> {
        let input = MidiInput::new("drums-app").map_err(|e| MidiError::Init(e.to_string()))?;
        let ports = input.ports();
        let port = ports.get(port_index).ok_or(MidiError::PortNotFound {
            index: port_index,
            available: ports.len(),
        })?;
        let port_name = input
            .port_name(port)
            .map_err(|e| MidiError::PortInfo(e.to_string()))?;

        let (tx, rx) = channel();
        let state = CallbackState {
            sender: tx,
            parser: MidiParser::new(),
            clock: Arc::clone(&self.clock),
            repaint: self.repaint.clone(),
        };

        // The callback runs on a real-time OS thread. It only samples the
        // high-resolution clock, parses (cheap; allocation-free for short
        // messages) and enqueues — it never blocks or does heavy work. It then
        // nudges the front-end to repaint, without transferring any payload.
        let callback = |_midi_timestamp: u64, bytes: &[u8], state: &mut CallbackState| {
            let ts = state.clock.now_micros();
            let events = state.parser.push_bytes(ts, bytes);
            let emitted = !events.is_empty();
            for event in events {
                // If the GUI dropped the receiver the send fails; that is fine.
                let _ = state.sender.send(event);
            }
            if emitted {
                if let Some(repaint) = &state.repaint {
                    repaint();
                }
            }
        };

        let connection = input
            .connect(port, "drums-app-midi-in", callback, state)
            .map_err(|e| MidiError::Connect(e.to_string()))?;

        self.connection = Some(ConnectionInfo {
            _connection: connection,
            port_name: port_name.clone(),
        });
        self.events_rx = Some(rx);
        Ok(port_name)
    }

    /// Closes any open connection and discards queued events.
    pub fn disconnect(&mut self) {
        if let Some(rx) = self.events_rx.as_ref() {
            while rx.try_recv().is_ok() {}
        }
        self.connection = None;
        self.events_rx = None;
    }

    /// Non-blocking drain of all currently queued events (GUI thread).
    pub fn poll_events(&self) -> Vec<MidiEvent> {
        match &self.events_rx {
            Some(rx) => rx.try_iter().collect(),
            None => Vec::new(),
        }
    }

    /// Sets a thread-safe callback invoked on the MIDI thread after each batch
    /// of events is enqueued. The GUI uses this to schedule an immediate repaint
    /// via `egui::Context::request_repaint()` so events display without waiting
    /// for user input. Pass `None` to disable the notification.
    pub fn set_on_event(&mut self, callback: Option<RepaintFn>) {
        self.repaint = callback;
    }

    /// Detects device disconnection by re-enumerating and checking whether the
    /// connected port still exists. If it is gone, the connection is closed.
    /// Returns `true` if the connection is still alive.
    pub fn refresh_connection(&mut self) -> bool {
        if self.connection.is_none() {
            return false;
        }
        let name = self.connected_port_name().unwrap_or_default().to_string();
        let still_there = match self.list_ports() {
            Ok(ports) => ports.iter().any(|p| p.name == name),
            Err(_) => false,
        };
        if !still_there {
            self.disconnect();
        }
        still_there
    }
}
