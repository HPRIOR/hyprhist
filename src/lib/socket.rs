use std::{ffi::OsStr, os::unix::fs::FileTypeExt, path::Path};

use anyhow::Context;
use hyprland::{
    data::Monitor,
    dispatch::{Dispatch, DispatchType, WindowIdentifier},
    shared::{Address, HyprDataActive},
};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::{self, DirEntry},
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

use crate::{
    cli::{FocusCommand, FocusCommandArgs},
    types::{FocusEvents, HyprEvents, SharedEventHistory, SortedDistinctVec, WindowEvent},
};

const FOCUS_SOCKET_PATH_ALL: &str = "/tmp/hyprhist_focus.sock";
const FOCUS_SOCKET_PREFIX: &str = "hyprhist_focus";
const TMP_PATH: &str = "/tmp";

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum SocketInstruction {
    Next,
    Prev,
}

impl From<&FocusCommand> for SocketInstruction {
    fn from(value: &FocusCommand) -> Self {
        match value {
            FocusCommand::Next(_) => SocketInstruction::Next,
            FocusCommand::Prev(_) => SocketInstruction::Prev,
        }
    }
}

impl SocketInstruction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Next => "next",
            Self::Prev => "prev",
        }
    }
}

fn generate_socket_path(input: &SortedDistinctVec<String>) -> String {
    if input.get().is_empty() {
        FOCUS_SOCKET_PATH_ALL.to_string()
    } else {
        format!(
            "{TMP_PATH}/{FOCUS_SOCKET_PREFIX}::{}.sock",
            input.get().join("::")
        )
    }
}

async fn is_focus_socket_file(entry: &DirEntry) -> anyhow::Result<bool> {
    Ok(entry.file_type().await?.is_socket()
        && entry.path().extension() == Some(OsStr::new("sock"))
        && entry
            .file_name()
            .to_string_lossy()
            .into_owned()
            .starts_with(FOCUS_SOCKET_PREFIX))
}

async fn remove_overlapping_focus_sockets(
    requested_monitors: &SortedDistinctVec<String>,
) -> anyhow::Result<()> {
    let mut directory = fs::read_dir(TMP_PATH).await?;

    if requested_monitors.get().is_empty() {
        // Since all available monitors are being tracked, any existing selective daemon sockets
        // will conflict and need to be removed
        while let Some(entry) = directory.next_entry().await? {
            if is_focus_socket_file(&entry).await?
                && entry.path().to_string_lossy().into_owned() != FOCUS_SOCKET_PATH_ALL
            {
                warn!(
                    "Removing conflicting socket at path '{}'",
                    entry.path().to_string_lossy()
                );
                fs::remove_file(entry.path()).await?;
            }
        }
    } else {
        while let Some(entry) = directory.next_entry().await? {
            if is_focus_socket_file(&entry).await? {
                let file_name = entry.file_name().to_string_lossy().into_owned();
                let file_name_without_ext = file_name
                    .strip_suffix(".sock")
                    .expect("Expected file to end with sock extension");

                if let Some(monitor_strs) =
                    file_name_without_ext.strip_prefix(&format!("{FOCUS_SOCKET_PREFIX}::"))
                {
                    for monitor_str in monitor_strs.split("::") {
                        if requested_monitors.get().contains(&monitor_str.to_owned()) {
                            warn!(
                                "Removing conflicting socket with overlapping monitor '{}' at path '{}'",
                                monitor_str,
                                entry.path().to_string_lossy()
                            );
                            fs::remove_file(entry.path()).await?;
                        }
                    }
                }

                // If there are specific monitor requested in this daemon any existing
                // FOCUS_SOCKET_PATH_ALL will conflict
                if entry.path().to_string_lossy().into_owned() == FOCUS_SOCKET_PATH_ALL {
                    warn!(
                        "Removing conflicting socket for all monitors at path '{}'",
                        entry.path().to_string_lossy()
                    );
                    fs::remove_file(entry.path()).await?;
                }
            }
        }
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

async fn navigate_focus_history(
    instruction: SocketInstruction,
    focus_events: SharedEventHistory<WindowEvent>,
) -> anyhow::Result<()> {
    debug!("Recieved socked instruction of {instruction:?}");

    let next_address = {
        let mut history = focus_events.lock().await;
        match instruction {
            SocketInstruction::Next => history.forward().map(|e| e.address.clone()),
            SocketInstruction::Prev => history.backward().map(|e| e.address.clone()),
        }
    };

    if let Some(addr) = next_address {
        info!(
            "Moved focus history cursor with {} (id {})",
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

async fn handle_focus_stream(
    stream: UnixStream,
    event_history: SharedEventHistory<WindowEvent>,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    while reader.read_line(&mut line).await? != 0 {
        let instruction: Option<SocketInstruction> = serde_json::from_str(line.trim())?;
        if let Some(instruction) = instruction {
            navigate_focus_history(instruction, event_history.clone()).await?;
        }

        line.clear();
    }

    Ok(())
}

#[allow(clippy::missing_errors_doc)]
pub async fn listen(hypr_events: HyprEvents) -> anyhow::Result<()> {
    match hypr_events {
        HyprEvents::Focus(FocusEvents {
            focus_events,
            requested_monitors,
        }) => {
            let socket_path = generate_socket_path(requested_monitors);
            cleanup_socket(&socket_path).await?;
            remove_overlapping_focus_sockets(requested_monitors).await?;
            let listener = UnixListener::bind(&socket_path)
                .with_context(|| format!("Failed to bind to {socket_path}"))?;

            info!("Listening for focus navigation on {socket_path}");

            loop {
                let (stream, _) = listener.accept().await?;
                let focus_events = focus_events.clone();

                tokio::spawn(async move {
                    if let Err(err) = handle_focus_stream(stream, focus_events).await {
                        error!("Failed handling focus socket request: {err:?}");
                    }
                });
            }
        }
    }
}

#[allow(clippy::missing_errors_doc)]
pub async fn send_focus_command(command: &'static FocusCommand) -> anyhow::Result<()> {
    let requested_monitors = match command {
        FocusCommand::Next(FocusCommandArgs {
            requested_monitors: monitors,
        })
        | FocusCommand::Prev(FocusCommandArgs {
            requested_monitors: monitors,
        }) => monitors,
    };

    let current_monitor = Monitor::get_active_async().await?;

    if !requested_monitors.is_empty() && !requested_monitors.contains(&current_monitor.name) {
        info!(
            "Ignoring focus command on untracked monitor: {}",
            current_monitor.name
        );
        return Ok(());
    }

    let socket_path = generate_socket_path(&SortedDistinctVec::new(requested_monitors.clone()));

    let mut stream = UnixStream::connect(&socket_path).await.context(format!(
        "Failed to connect to focus socket at {}",
        &socket_path
    ))?;

    let payload: SocketInstruction = command.into();

    stream
        .write_all(serde_json::to_string(&payload)?.as_bytes())
        .await
        .context("Failed to send focus command")?;

    Ok(())
}
