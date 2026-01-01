# Hyprhist

A utility for traversing focus history across Hyprland windows.

<!--toc:start-->

- [Hyprhist](#hyprhist)
  - [Commands](#commands)
  - [Hyprland configuration](#hyprland-configuration)
  - [TODO](#todo)
  <!--toc:end-->

## Commands

Start daemon:

```shell
hyprhist daemon focus
```

Execute next/prev commands:

```shell
hyprhist focus next
```

```shell
hyprhist focus prev
```

> If new events are added when traversing focus history, the history will be truncated to that point, and the new event will be added.

Window events can be tracked and traversed on independent monitor groups:

```shell
hyprhist daemon focus --monitor HDMI-1-A --monitor DP-1
```

```shell
hyprhist focus next --monitor HDMI-1-A --monitor DP-1
```

```shell
hyprhist daemon focus --monitor DP-2
```

```shell
hyprhist focus next --monitor DP-2
```

> The monitors specified in the `next`/`prev` and `daemon` arguments much match exactly.

> Window focus history is preserved when moving windows between tracked and untracked monitors. Historical focus events for windows residing on an untracked monitor will be ignored by the daemon when traversing with `next`/`prev` until the window is moved back to a tracked monitor.

If two daemons have an overlapping monitor specified, only the most recent daemon will work.

```shell
hyprhist daemon focus --monitor HDMI-1-A --monitor DP-1
hyprhist daemon focus --monitor HDMI-1-A --monitor DP-2  # HDMI-1-A overlaps, only this daemon will work
```

If no monitors are specified then events on all monitors are tracked. The above rule then applies to the set of all monitors available.

```shell
hyprhist daemon focus                                    # Contains every monitor available
hyprhist daemon focus --monitor HDMI-1-A --monitor DP-2  # Mutually exclusive configurations, only the latter daemon will work
```

The maximum number of events to track can be specified (defaults to 300):

```shell
hyprhist daemon focus --history-size 10
```

## Hyprland configuration

```config
exec-once = ~/path/to/hyprhist daemon focus

bind = $mainMod, I, exec, ~/path/to/hyprhist focus next
bind = $mainMod, O, exec, ~/path/to/hyprhist focus prev
```

## TODO

- Track and traverse other Hyprland events
