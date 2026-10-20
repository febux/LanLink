# Spec Delta

## Purpose

The desktop shell integrates with the system tray so the app can run in the background, surface quick status at a glance, and notify the user about membership and path-change events without requiring the window to be visible.

## ADDED Requirements

### Requirement: Tray icon shows quick stats tooltip
The system tray icon SHALL show a tooltip with the current network ID, peer count, and whether the daemon is online or offline.

#### Scenario: Daemon is online
- **WHEN** the user hovers the tray icon while the daemon has joined a network
- **THEN** the tooltip shows the network ID and peer count

#### Scenario: Daemon is offline
- **WHEN** the user hovers the tray icon while no daemon status is available
- **THEN** the tooltip shows that LanLink is offline

### Requirement: Polling rate adapts to window focus
The desktop app SHALL poll the daemon status socket faster when the main window is focused and slower when the window is hidden or only the tray icon is active.

#### Scenario: Window focused
- **WHEN** the main window is focused and visible
- **THEN** snapshot polling runs at approximately 1 second intervals

#### Scenario: Window hidden
- **WHEN** the main window is hidden and only the tray icon remains
- **THEN** snapshot polling runs at approximately 5 second intervals

### Requirement: Toast notifications for peer membership changes
The desktop app SHALL display a toast notification when a peer joins or leaves the network.

#### Scenario: Peer joins
- **WHEN** a new peer appears in the snapshot that was not present in the previous snapshot
- **THEN** a toast notification appears with the peer's virtual IP and the text "joined"

#### Scenario: Peer leaves
- **WHEN** a peer that was present in the previous snapshot is no longer in the current snapshot
- **THEN** a toast notification appears with the peer's virtual IP and the text "left"

### Requirement: Toast notifications for path downgrade
The desktop app SHALL display a warning toast when a peer's path degrades from a direct path to a relay path.

#### Scenario: Direct to relay
- **WHEN** a peer that was on `direct` or `tcp_direct` in the previous snapshot is now on `relay` or `tcp_relay`
- **THEN** a warning toast appears with the peer's virtual IP and the new path kind

### Requirement: Tray menu provides show and quit actions
The tray icon context menu SHALL provide controls to show the main window and to quit the application.

#### Scenario: Show window
- **WHEN** the user selects the show action from the tray menu
- **THEN** the main window is shown, unminimized, and focused

#### Scenario: Quit application
- **WHEN** the user selects the quit action from the tray menu
- **THEN** the application exits cleanly
