## RustChat

### 1. Overview

**2025 Rust Programming Final Project: RustChat**
RustChat is a distributed chat-room system implemented in Rust. It enables multiple clients to communicate asynchronously over the network in a fully non-blocking manner.

### 2. Getting Started

#### 2.1 Build the Project

```bash
# From the project root directory
cargo build --release
```

This produces optimized binaries in `target/release/`.

#### 2.2 Launch the Server

```bash
cargo run --release --bin server
```

By default, the server listens on port **8080** of the local machine.

#### 2.3 Launch the Client

In a new terminal window:

```bash
cargo run --release --bin client
```

Enter your chosen nickname. You may open multiple client instances (in separate terminals) with different usernames.

### 3. Usage

* **Broadcast Message**
  Simply type any line of text (e.g. `Hello everyone`) and press Enter. The server will forward your message to **all** connected clients.

* **Private Message**
  Use the syntax:

  ```
  /w <username> <message>
  ```

  Example:

  ```
  /w Bob Hi Bob, how are you?
  ```

  The server will deliver `<message>` only to the specified `<username>`.

* **List Users**

  ```
  /users
  ```

  The server responds with the current list of online users.

* **Chat History**

  ```
  /history
  ```

  The server returns a selective subset of past messages.

* **Quit Chat**

  ```
  q
  ```

  Typing `q` in any client window disconnects you; the server will broadcast your departure to the remaining clients.

* **Shutdown Server**
  Press `Ctrl+C` in the server terminal to stop the server gracefully.

---