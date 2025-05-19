# Local Network Chat

## Project Overview

Local Network Chat is a Rust-based application designed for peer-to-peer messaging over a local network. It consists of two main components:

1.  **`localchat_gui`**: A graphical user interface (GUI) built with `egui` for user interaction, displaying messages, and managing connections.
2.  **`localchat_daemon`**: A background process (daemon) that handles network communication, peer discovery using mDNS, message routing, and communication with the GUI via Unix sockets.

The application allows users on the same local network to discover each other and exchange text messages without relying on a central server. It supports running multiple instances on the same machine, each with its own identity and communication channels.

## Features

*   **Peer Discovery**: Automatically discovers other users on the local network using mDNS (Multicast DNS).
*   **Real-time Messaging**: Send and receive messages instantly with other discovered peers.
*   **GUI Interface**: User-friendly interface built with `egui` to display chat conversations, online peers, and settings.
*   **Daemon Process**: Handles all networking and background tasks, ensuring the GUI remains responsive.
*   **Multiple Instance Support**: Run multiple chat instances on the same machine (e.g., for different user profiles or testing), each with a unique TCP port and Unix socket path.
*   **Persistent Username**: Remembers the user's chosen username across sessions for each instance.
*   **Desktop Notifications**: Provides desktop notifications for new messages.
*   **Cross-Platform (Potentially)**: Built with Rust and `egui`, which are cross-platform, though current setup and scripts might have OS-specific considerations (e.g., Unix sockets).

## Project Structure

The project is organized into two main crates:

*   `localchat_gui/`: Contains the source code for the Egui-based graphical user interface.
    *   `src/main.rs`: Entry point for the GUI application, manages the main application loop, state, and IPC with the daemon.
    *   `src/components/`: UI components for different parts of the chat interface (chat area, side menu for peers, top navigation, settings).
    *   `Cargo.toml`: Defines GUI dependencies like `eframe`, `egui`, `tokio` (for async operations), `serde` (for serialization).
*   `localchat_daemon/`: Contains the source code for the background daemon process.
    *   `src/main.rs`: Entry point for the daemon, manages peer discovery (mDNS), TCP server for peer-to-peer messaging, and IPC with the GUI via Unix sockets.
    *   `Cargo.toml`: Defines daemon dependencies like `tokio`, `mdns-sd`, `serde`, `network-interface` (for network information).
*   `README.md`: (This file) Project overview and instructions.

## Core Technologies Used

*   **Rust**: The programming language used for both the GUI and daemon.
*   **Tokio**: Asynchronous runtime for managing concurrent operations (networking, IPC).
*   **Egui (via eframe)**: Immediate mode GUI library for creating the user interface.
*   **mDNS-SD**: For service discovery (finding other chat clients) on the local network.
*   **Serde**: For serializing and deserializing data structures (messages, commands) for IPC and network communication.
*   **Unix Sockets**: Used for Inter-Process Communication (IPC) between the `localchat_gui` and `localchat_daemon` on the same machine.
*   **TCP Sockets**: Used for direct peer-to-peer message exchange between daemons on the network.

## How It Works

1.  **Initialization**:
    *   The GUI (`localchat_gui`) starts and attempts to launch its corresponding `localchat_daemon` instance.
    *   The GUI and daemon are configured with unique TCP ports and Unix socket paths based on an `--instance <N>` command-line argument. This allows multiple independent instances to run.
    *   The daemon attempts to load a persistent user identity (username and unique suffix). If not found, it waits for the GUI to provide one.

2.  **GUI-Daemon Communication (IPC)**:
    *   The GUI connects to the daemon via a Unix domain socket.
    *   They exchange messages defined in `GuiToDaemonCommand` (GUI -> Daemon) and `DaemonToGuiMessage` (Daemon -> GUI) enums. These messages are serialized/deserialized using JSON.
    *   Commands include setting a username, requesting the peer list, sending a message, etc.
    *   Messages include daemon status, peer list updates, new incoming messages, identity confirmation, etc.

3.  **Username and Identity**:
    *   The GUI prompts the user for a username if one isn't already saved for the current instance.
    *   The chosen username is sent to the daemon.
    *   The daemon generates a unique `UserIdentity` (combining the username with a random suffix for mDNS and message identification) and saves it persistently (e.g., in `/tmp/localchat_daemon_identity_<instance_num>.json`). This full identity is sent back to the GUI.

4.  **Peer Discovery (mDNS)**:
    *   Once the daemon has a user identity, it registers an mDNS service (e.g., `_localchat._tcp.local.`) on the local network. The service announcement includes the user's display name, full message ID, IP address, and TCP port.
    *   The daemon also browses for other instances of the `_localchat._tcp.local.` service.
    *   Discovered peers are added to a list, which is then sent to the GUI to update its peer list.

5.  **Messaging**:
    *   When a user sends a message via the GUI:
        *   The GUI sends a `SendMessage` command to its daemon via the Unix socket. This command includes the recipient's ID (their full message ID) and the message content.
        *   The local message is immediately displayed in the GUI as "self" sent.
    *   The daemon looks up the recipient peer's IP address and port from its mDNS-discovered peer list.
    *   It establishes a direct TCP connection to the recipient peer's daemon.
    *   The message (a `Message` struct containing sender ID, recipient ID, content, timestamp) is serialized to JSON and sent over the TCP connection.
    *   The recipient daemon receives the TCP message, deserializes it, and forwards it to its connected GUI via the Unix socket as a `NewMessage`.
    *   The recipient's GUI displays the incoming message.

## Setup and Running Instructions

### Prerequisites

*   **Rust**: Ensure you have Rust installed. You can get it from [rustup.rs](https://rustup.rs/).
*   **Build tools**: Depending on your OS, you might need standard build tools (e.g., `build-essential` on Debian/Ubuntu, Xcode command-line tools on macOS).
*   **For Linux (mDNS)**: You might need Avahi daemon running. Install `libavahi-compat-libdnssd-dev` (or similar package providing `dns_sd.h`) for `mdns-sd` crate.
    ```bash
    sudo apt-get install libavahi-compat-libdnssd-dev
    ```

### Building the Project

The project uses a Cargo workspace.

1.  **Navigate to the project root directory.**
2.  **Build the entire project (both GUI and daemon):**
    ```bash
    cargo build
    ```
    For a release build (recommended for performance):
    ```bash
    cargo build --release
    ```
    The binaries will be located in `target/debug/` or `target/release/`. The GUI will be `localchat_gui` and the daemon `localchat_daemon`.

### Running the Application

The GUI application is designed to launch its corresponding daemon.

1.  **Run the GUI application from the project root or by specifying its path:**
    ```bash
    cargo run -p localchat_gui
    ```
    Or, if already built:
    ```bash
    ./target/debug/localchat_gui
    ```

2.  **Running Multiple Instances**:
    The application supports an `--instance` argument to run multiple, isolated instances. Each instance will use a different TCP port for peer-to-peer communication and a different Unix socket for GUI-daemon IPC.

    *   Instance 1 (default if no argument is given):
        ```bash
        ./target/debug/localchat_gui --instance 1
        # or simply ./target/debug/localchat_gui
        ```
        Daemon will use TCP port `12345` and socket `/tmp/localchat_daemon1.sock`.
        Username is stored in `~/.localchat_gui_username_1`.
        Daemon identity in `/tmp/localchat_daemon_identity_1.json`.

    *   Instance 2:
        ```bash
        ./target/debug/localchat_gui --instance 2
        ```
        Daemon will use TCP port `12346` and socket `/tmp/localchat_daemon2.sock`.
        Username is stored in `~/.localchat_gui_username_2`.
        Daemon identity in `/tmp/localchat_daemon_identity_2.json`.

    And so on. When the GUI starts, it attempts to spawn the `localchat_daemon` and sets environment variables `LOCALCHAT_TCP_PORT` and `LOCALCHAT_SOCKET_PATH` for the daemon process based on the instance number. In debug mode (`cargo run`), it uses `cargo run -p localchat_daemon` to start the daemon. In release mode, it looks for the `localchat_daemon` binary in the same directory as the `localchat_gui` executable.

### Notes:
*   The daemon will attempt to remove any pre-existing Unix socket file at its path on startup.
*   Usernames are persisted per instance in a file like `~/.localchat_gui_username_<instance_number>`.
*   Daemon identities (which include the mDNS service name and full message ID) are persisted in `/tmp/localchat_daemon_identity_<instance_number>.json`.

## IPC Details

Communication between the GUI and its local daemon is handled via Unix Domain Sockets. This is efficient for same-machine communication.

*   **Socket Path**: Dynamically generated based on instance number, e.g., `/tmp/localchat_daemon<N>.sock`.
*   **Message Format**: JSON serialized strings, newline-delimited.
*   **Core Structures**:
    *   `GuiToDaemonCommand`: Enum defining messages from GUI to Daemon (e.g., `GetPeers`, `SendMessage`, `SetUsername`).
    *   `DaemonToGuiMessage`: Enum defining messages from Daemon to GUI (e.g., `PeerList`, `NewMessage`, `IdentityInfo`).
    *   `IpcPeer`: Struct representing a discovered peer, containing ID, username, IP, and port.
    *   `Message`: Struct representing a chat message, with ID, sender, recipient, content, timestamp, and `is_self` flag.

## Future Enhancements (Potential Ideas)

*   **Message History/Persistence**: Store and load chat history (currently messages are in-memory for the session).
*   **File Sharing**: Allow users to send/receive files.
*   **End-to-End Encryption**: Implement encryption for messages.
*   **Group Chats**: Support for chat rooms with multiple participants.
*   **Improved Error Handling and Resilience**: More robust handling of network issues and disconnections.
*   **Packaging**: Create distributable packages for different operating systems (the `localchat_gui/Cargo.toml` has some initial bundle metadata).
*   **Cross-compilation for Windows/macOS**: Ensure build and run scripts are fully cross-platform.
*   **Direct GUI-to-GUI fallback**: If daemon connection fails or for simpler scenarios, explore direct mDNS discovery and messaging from the GUI (though this loses the benefits of a separate daemon).
*   **Database for Messages/Peers**: Utilize SQLite or a similar embedded database for more robust storage. (The daemon `Cargo.toml` includes `rusqlite` but it doesn't seem to be used yet).

---

This README provides a comprehensive overview of the Local Network Chat application. For more specific details, please refer to the source code and inline comments. 