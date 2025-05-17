# Project Plan: Local Network Chat (macOS Native + egui)

## 1. Project Overview

**Goal:** To create a high-performance, native macOS desktop chat application using `egui`. It will enable secure communication *only* on the user's currently active local Wi-Fi network, featuring a persistent background daemon for continuous message reception and local history storage.

**Core Principles:**
* **macOS Native:** Optimized specifically for macOS.
* **Local & Private:** Communication confined to the current Wi-Fi network.
* **Performance:** Leverage Rust and native macOS capabilities.
* **Always-On Receiver:** A `launchd` agent ensures the daemon runs reliably.
* **Secure:** Local communication scope.
* **Local History:** Chat history stored locally via SQLite.
* **GUI:** Native Rust GUI using `egui`.

**Chosen Technology Stack:**
* **Language:** Rust
* **GUI:** `egui` (via `eframe`)
* **Backend Logic/Daemon:** Headless Rust application.
* **Async Runtime:** Tokio
* **Networking:** `tokio::net` (TCP), `network-interface` crate (or macOS native APIs via `SystemConfiguration` bindings if needed).
* **Discovery:** `mdns-sd` crate.
* **Serialization:** `serde`, `serde_json` (or `bincode`).
* **Persistence:** `rusqlite` (with `bundled` feature).
* **Notifications:** `std::process::Command` calling `osascript`, or native bindings (`user-notification-sys`, `objc`).
* **Inter-Process Communication (IPC):** Unix Domain Sockets (`tokio::net::UnixStream`, `tokio::net::UnixListener`).
* **Background Persistence:** macOS `launchd` agent (`.plist` file).
* **Packaging:** `.app` bundle for GUI, `.dmg` or `.pkg` installer.

**Key Architectural Pattern:**
* **Two Processes:**
    * `localchat_daemon`: Background `launchd` agent. Handles networking, discovery, notifications, SQLite history. Listens on a Unix domain socket for IPC.
    * `localchat_gui`: Foreground `.app` bundle using `eframe`/`egui`. Connects to the daemon via its Unix domain socket. Handles UI rendering and user interaction, relaying actions/data via IPC.
* **Network Binding:** Daemon binds network services (mDNS, TCP) to the active Wi-Fi interface IP.
* **IPC Socket Location:** Typically `~/Library/Application Support/YourAppName/daemon.sock`.

**Limitations:**
* macOS only.
* Installation requires setting up the `launchd` agent.

## 2. Phase 1: Background Daemon - Core Networking & Discovery (macOS)

**Objective:** Create the headless daemon, implement network interface detection, mDNS, and TCP listening bound to the active Wi-Fi on macOS.

**Tasks:**
1.  **Daemon Project Setup:**
    * `cargo new localchat_daemon --bin`.
    * Add dependencies: `tokio`, `mdns-sd`, `serde`, `serde_json`, `network-interface`, `tracing`, `tracing-subscriber`.
2.  **Network Interface Monitoring:**
    * Implement logic using `network-interface` or macOS native APIs to find the active Wi-Fi interface's IPv4 address. Handle cases where Wi-Fi might not be the primary interface or is inactive.
    * Need robust error handling and potentially periodic checks/updates if the network changes.
3.  **mDNS/Bonjour (Bound to Interface):**
    * Spawn Tokio task.
    * Configure `mdns-sd` to use the specific IP address of the active Wi-Fi interface for registration/Browse.
    * Register `_localchat._tcp.local.` service (daemon ID, username, TCP port).
    * Browse for peers, maintain shared list (`tokio::sync::Mutex<HashMap<...>>`).
4.  **TCP Listener (Bound to Interface):**
    * Spawn Tokio task.
    * Bind `tokio::net::TcpListener` to the Wi-Fi IP and chosen port.
    * Basic `accept()` loop, spawn tasks for connections (log data for now).
5.  **Logging:** Setup `tracing` subscriber for file-based logging (e.g., to `~/Library/Logs/YourAppName/daemon.log`).

**Outcome:** A headless daemon executable that performs discovery and listens for connections on the active macOS Wi-Fi interface.

## 3. Phase 2: Background Daemon - History, Notifications, IPC (macOS)

**Objective:** Add SQLite storage, macOS notifications, and the Unix domain socket IPC server.

**Tasks:**
1.  **SQLite History Storage:**
    * Add `rusqlite` (with `bundled`).
    * Add `dirs-next` to find appropriate Application Support directory (`~/Library/Application Support/YourAppName/`).
    * Define DB schema, initialize DB in the App Support dir.
    * Implement storing received messages in the TCP handler.
2.  **macOS Notifications:**
    * Implement `send_desktop_notification(sender, message)` function using `std::process::Command` to execute `osascript`:
      ```rust
      use std::process::Command;
      fn send_desktop_notification(sender: &str, message: &str) -> std::io::Result<()> {
          let script = format!(
              "display notification \"{}\" with title \"Message from {}\"",
              message.replace('"', "\\\""), // Basic escaping
              sender.replace('"', "\\\"")
          );
          Command::new("osascript").arg("-e").arg(script).status()?;
          Ok(())
      }
      ```
    * Call this function when messages are received/stored.
3.  **IPC Server (Unix Domain Socket):**
    * Determine socket path (e.g., `~/Library/Application Support/YourAppName/daemon.sock`). Ensure directory exists. Clean up old socket file on startup.
    * Spawn Tokio task to run `tokio::net::UnixListener::bind`.
    * Accept incoming `UnixStream` connections.
    * Define JSON-based IPC protocol (requests like `{"command": "get_peers"}` and responses/events).
    * Implement basic command handlers (`"get_peers"`, `"get_status"`).

**Outcome:** Daemon stores messages, sends native macOS notifications, and allows basic queries via a Unix domain socket.

## 4. Phase 3: Foreground GUI - Basic Setup & IPC Connection (`egui` on macOS)

**Objective:** Create the `egui` app bundle structure and connect it to the daemon via IPC.

**Tasks:**
1.  **GUI Project Setup:** [COMPLETED]
    * `cargo new localchat_gui --bin`.
    * Add dependencies: `eframe`, `egui`, `tokio` (for async IPC client), `serde`, `serde_json`, `dirs-next`, logging.
    * Set up basic `eframe::App` struct and `main` function using `eframe::run_native`.
2.  **IPC Client:** [PENDING]
    * On app startup, determine the daemon's socket path (`dirs-next`).
    * Spawn a Tokio task (using `eframe` integration or `std::thread` + `tokio::runtime::Runtime`) to connect `tokio::net::UnixStream` to the daemon socket.
    * Handle connection logic and reading/writing JSON messages over the stream.
    * Use channels (e.g., `tokio::sync::mpsc` or `crossbeam-channel`) to send data/status between the IPC task and the main `egui` UI thread/state.
3.  **`egui` State:** [IN PROGRESS] Store daemon connection status, received peer list in the `App` struct.
4.  **Basic UI:** [IN PROGRESS]
    * Display daemon connection status (`egui::Label`).
    * On connect (or periodically), send `"get_peers"` command via IPC.
    * Update `App` state when peer list is received via IPC channel.
    * Display peer list using `egui` widgets.

**Outcome:** An `egui` application window that connects to the daemon and displays discovered peers.

## 5. Phase 4: Foreground GUI - Chat Interface & Interaction (`egui` on macOS)

**Objective:** Build the main chat UI within `egui` and enable message sending/receiving/history viewing via IPC.

**Tasks:**
1.  **`egui` UI:** [IN PROGRESS] Create chat view, message input widgets.
2.  **Sending Messages:** [PLANNED] UI Input -> IPC Command (`"send_message"`, ...) -> Daemon -> TCP Send -> Daemon stores outgoing in DB.
3.  **Receiving Messages:** [PLANNED] Daemon receives TCP -> Stores in DB -> Sends via IPC -> GUI IPC Task -> Channel -> `App` State Update -> `egui` redraws.
4.  **Displaying History:** [PLANNED] UI requests history (`"get_history"`, ...) via IPC -> Daemon queries DB -> Daemon sends data via IPC -> GUI displays.

**Outcome:** Functional chat interface within the `egui` app.

## 6. Phase 5: Refinements & Background Persistence (macOS `launchd`)

**Objective:** Enhance reliability and set up the daemon to run persistently using `launchd`.

**Tasks:**
1.  **Error Handling/Comms:** Improve daemon/GUI error handling and IPC communication robustness.
2.  **Network Change Handling:** Daemon should gracefully handle Wi-Fi disconnects/reconnects/IP changes, updating its state and notifying the GUI.
3.  **`launchd` Agent Setup:**
    * Create a `.plist` file (`com.yourcompany.localchat_daemon.plist`). Define:
        * `Label`: Unique identifier.
        * `ProgramArguments`: Path to the daemon executable + any needed args.
        * `RunAtLoad`: `true` (start on load/login).
        * `KeepAlive`: `true` (restart if it crashes).
        * `StandardOutPath`/`StandardErrorPath`: Paths for logs (optional).
    * **Installation:** The installer (Phase 7) will need to:
        * Place this `.plist` file in `~/Library/LaunchAgents/`.
        * Ensure the `ProgramArguments` path points to the installed daemon location.
        * Load the agent using `launchctl load ~/Library/LaunchAgents/com.yourcompany.localchat_daemon.plist`.
4.  **Daemon Shutdown:** Implement graceful shutdown logic in the daemon (cleaning up IPC socket, saving state) when it receives `SIGTERM` (which `launchctl unload` sends).

**Outcome:** Daemon runs reliably in the background as a user agent, managed by `launchd`.

## 7. Phase 6: Build & Packaging (macOS Native)

**Objective:** Create a distributable macOS application bundle and installer.

**Tasks:**
1.  **Build Binaries:** Use `cargo build --release` for both daemon and GUI.
2.  **Create `.app` Bundle (GUI):**
    * Use `cargo bundle --release` (if using `cargo-bundle`) OR manually create the structure: `YourApp.app/Contents/MacOS/localchat_gui`, `YourApp.app/Contents/Info.plist`, `YourApp.app/Contents/Resources/icon.icns`.
    * Ensure `Info.plist` is correctly configured.
3.  **Create Installer (`.dmg` or `.pkg`):**
    * **Option A (`.dmg`):** Simpler. Create a DMG containing:
        * The `localchat_gui.app` bundle (user drags to /Applications).
        * The `localchat_daemon` binary.
        * The `com.yourcompany.localchat_daemon.plist` file.
        * An installation script (`.command` or `.sh`) or clear instructions for the user to:
            * Copy the `.app`.
            * Copy the daemon binary to a stable location (e.g., `~/Library/Application Support/YourAppName/`).
            * Copy the `.plist` file to `~/Library/LaunchAgents/`, ensuring the path inside it is correct.
            * Run `launchctl load ~/Library/LaunchAgents/...`.
    * **Option B (`.pkg`):** More complex, better user experience. Use `pkgbuild` / `productbuild` or tools like Packages.app to create a package that automatically:
        * Installs the `.app` to `/Applications`.
        * Installs the daemon to `/Library/Application Support/YourAppName/` (or similar).
        * Installs the `.plist` to `/Library/LaunchAgents/` (requires admin for system-wide daemon) or `~/Library/LaunchAgents/` (user agent).
        * Runs post-install scripts to set permissions and load the `launchd` agent.
        * Handles uninstallation cleanly (runs scripts to unload agent, remove files).
4.  **Code Signing/Notarization:** Sign the `.app` bundle and the installer package with an Apple Developer ID certificate for distribution without Gatekeeper warnings. Notarize the application with Apple.
5.  **Testing:** Test the entire installation, execution, and uninstallation process thoroughly.

**Outcome:** A standard, distributable macOS application installer (`.dmg` or `.pkg`).