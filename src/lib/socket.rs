use std::path::Path;

use anyhow::Context;
use hyprland::{
    dispatch::{Dispatch, DispatchType, WindowIdentifier},
    shared::Address,
};
use log::{error, info, warn};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

use crate::{
    cli::{DaemonCommand, FocusCommand},
    types::HyprEventHistory,
};

const FOCUS_SOCKET_PATH: &str = "/tmp/hyprhist_focus.sock";

#[derive(Clone, Copy, Debug)]
enum SocketInstruction {
    Next,
    Prev,
}

impl SocketInstruction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Next => "next",
            Self::Prev => "prev",
        }
    }
}

#[allow(clippy::missing_errors_doc)]
pub async fn listen(command: DaemonCommand, event_history: HyprEventHistory) -> anyhow::Result<()> {
    match command {
        DaemonCommand::Focus(_) => listen_for_focus(event_history).await,
    }
}

#[allow(clippy::missing_errors_doc)]
pub async fn send_focus_command(command: FocusCommand) -> anyhow::Result<()> {
    let mut stream = UnixStream::connect(FOCUS_SOCKET_PATH)
        .await
        .context("Failed to connect to focus socket")?;

    let payload = match command {
        FocusCommand::Next => "next\n",
        FocusCommand::Prev => "prev\n",
    };

    stream
        .write_all(payload.as_bytes())
        .await
        .context("Failed to send focus command")?;

    Ok(())
}

async fn listen_for_focus(event_history: HyprEventHistory) -> anyhow::Result<()> {
    cleanup_socket(FOCUS_SOCKET_PATH).await?;
    let listener = UnixListener::bind(FOCUS_SOCKET_PATH)
        .with_context(|| format!("Failed to bind to {FOCUS_SOCKET_PATH}"))?;
    info!("Listening for focus navigation on {FOCUS_SOCKET_PATH}");

    loop {
        let (stream, _) = listener.accept().await?;
        let history = event_history.clone();

        tokio::spawn(async move {
            if let Err(err) = handle_focus_stream(stream, history).await {
                error!("Failed handling focus socket request: {err:?}");
            }
        });
    }
}

async fn handle_focus_stream(
    stream: UnixStream,
    event_history: HyprEventHistory,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    while reader.read_line(&mut line).await? != 0 {
        let instruction = match line.trim() {
            "next" => Some(SocketInstruction::Next),
            "prev" => Some(SocketInstruction::Prev),
            other => {
                warn!("Unknown focus socket command: {other}");
                None
            }
        };

        if let Some(instruction) = instruction {
            navigate_focus_history(instruction, &event_history).await?;
        }

        line.clear();
    }

    Ok(())
}

async fn navigate_focus_history(
    instruction: SocketInstruction,
    HyprEventHistory { focus_events }: &HyprEventHistory,
) -> anyhow::Result<()> {
    let Some(focus_events) = focus_events else {
        warn!("Received focus navigation without focus event history configured");
        return Ok(());
    };

    let next_address = {
        let mut history = focus_events.lock().await;
        match instruction {
            SocketInstruction::Next => history.forward().map(|e| e.address.clone()),
            SocketInstruction::Prev => history.backward().map(|e| e.address.clone()),
        }
    };

    if let Some(addr) = next_address {
        info!(
            "Moved focus history cursor with {} (address {})",
            instruction.as_str(),
            addr
        );
        let _ = Dispatch::call_async(DispatchType::FocusWindow(WindowIdentifier::Address(
            Address::new(addr),
        )))
        .await;
    } else {
        info!(
            "No focus history item available for {} request",
            instruction.as_str()
        );
    }

    Ok(())
}

async fn cleanup_socket(path: &str) -> anyhow::Result<()> {
    if Path::new(path).exists() {
        fs::remove_file(path)
            .await
            .with_context(|| format!("Failed to remove stale socket at {path}"))?;
    }

    Ok(())
}
