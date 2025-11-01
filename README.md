# idmap-core

A robust, production-ready distributed key generation (DKG) and threshold signing framework for Solana transactions using Ed25519 cryptography.

---

## 🚀 Overview

**idmap-core** enables two parties to:

- **Jointly generate a shared Ed25519 keypair** without exposing the full private key to either participant.
- **Collaboratively sign Solana transactions** using a 2-of-2 threshold signature scheme, ensuring that neither party can unilaterally sign.

This architecture ensures secure, non-custodial management of Solana keys, ideal for high-trust applications such as wallets, blockchain identity, and decentralized finance.

---

## 📚 Table of Contents

- [Features](#features)
- [Related Resources](#related-resources)
- [Architecture](#architecture)
- [Project Structure](#project-structure)
- [Quickstart](#quickstart)
- [Configuration](#configuration)
- [Library API Highlights](#library-api-highlights)
- [Extensibility](#extensibility)
- [Troubleshooting](#troubleshooting)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [License](#license)

---

## ✨ Features

- **2-of-2 Threshold DKG:** Secure, distributed keypair generation (Ed25519, CGGMP21 protocol).
- **Collaborative Signing:** Both parties must participate to produce a valid Solana signature.
- **Redis Pub/Sub Orchestration:** Session-based protocol triggering and coordination.
- **TCP-based MPC Transport:** Reliable, length-delimited message framing over async sockets.
- **Modular Workspace:** Clean separation between library, client, and server components.
- **Production-Grade Primitives:** Built on top of industry-standard cryptography and async Rust.
- **Extensible:** Designed for future threshold configurations, key storage, and secure enclaves.

---

## 🌐 Related Resources

- **Gateway Repository:** [idmap-gateway](https://github.com/akash-R-A-J/idmap-gateway)
- **Core Documentation:** [deepwiki.com/akash-R-A-J/idmap-core](https://deepwiki.com/akash-R-A-J/idmap-core)
- **Gateway Documentation:** [deepwiki.com/akash-R-A-J/idmap-gateway](https://deepwiki.com/akash-R-A-J/idmap-gateway)
- **Live Demo:** [id-map.shop](https://www.id-map.shop/)

---

## 🏛 Architecture

### High-Level Workflow

```
┌────────────────────────────────────────────┐
│                Redis Pub/Sub               │
│  (Triggers keygen/signing sessions)        │
└─────────┬──────────────────────┬───────────┘
          │                      │
          ▼                      ▼
    ┌─────────────┐       ┌─────────────┐
    │   Server    │◄─────►│   Client    │
    │ NODE_ID=0   │  TCP  │ NODE_ID=1   │
    └─────────────┘       └─────────────┘
          │                      │
          └──────────┬───────────┘
                     ▼
         MPC Protocol Execution
```

**Flow:**

1. Both server and client listen to Redis channels for protocol triggers.
2. External systems publish to `keygen:start:<session_id>` or `signing:start:<session_id>`.
3. Server accepts TCP connections from the client for each session.
4. Parties execute the round-based MPC protocol for DKG or signing.
5. Results are persisted locally and can be reported back via Redis.

**Communication:**

- **Redis:** Session coordination and external orchestration.
- **TCP:** Secure MPC message exchange.
- **Solana RPC:** Transaction creation and submission.

---

## 📁 Project Structure

```
idmap-core/
├── src/              # Core library (dkg_tcp)
│   ├── keygen.rs     # DKG protocol implementation
│   ├── sign.rs       # Threshold signing logic
│   ├── transport.rs  # TCP message transport layer
│   └── env_loader.rs # Environment configuration loader
├── server/           # Server binary
│   └── src/
│       └── server.rs # Handles protocol orchestration
├── client/           # Client binary
│   └── src/
│       └── client.rs # Initiates keygen/signing as a participant
```

- **src/** — Reusable library with DKG/signing primitives and TCP transport.
- **server/** — Orchestrates sessions, listens for connections.
- **client/** — Connects as a participant to run keygen/signing.

---

## ⚡ Quickstart

### Prerequisites

- [Rust](https://rustup.rs/) 1.70+ (edition 2024)
- [Redis server](https://redis.io/) running locally or remotely
- Solana devnet RPC endpoint (for end-to-end testing)

### 1. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Clone and Build

```bash
git clone https://github.com/akash-R-A-J/idmap-core.git
cd idmap-core
cargo build --release
```

### 3. Configure `.env` Files

Each binary requires its own environment file.

#### Server (`server/.env`):

```env
NODE_ID=0
N=2
REDIS_URL=redis://127.0.0.1:6379
DKG_SERVER_ADDR=0.0.0.0:7001
SIGN_SERVER_ADDR=0.0.0.0:7002
DEFAULT_SESSION_ID=session-001
```

#### Client (`client/.env`):

```env
NODE_ID=1
N=2
REDIS_URL=redis://127.0.0.1:6379
DKG_SERVER_ADDR=127.0.0.1:7001
SIGN_SERVER_ADDR=127.0.0.1:7002
DEFAULT_SESSION_ID=session-001
```

### 4. Run Redis

```bash
redis-server
```

### 5. Start the Server

```bash
cargo run -p server
```

### 6. Start the Client

```bash
cargo run -p client
```

### 7. Trigger Protocols with Redis CLI

```bash
# Initiate key generation
redis-cli PUBLISH "keygen:start:session-001" ""

# Initiate signing (after keygen completes)
redis-cli PUBLISH "signing:start:session-001" ""
```

---

## ⚙️ Configuration Reference

**Environment variables:**

| Variable           | Description                                         |
|--------------------|-----------------------------------------------------|
| `NODE_ID`          | Unique party identifier (0 = server, 1 = client)    |
| `N`                | Total number of participants (currently 2)          |
| `REDIS_URL`        | Redis connection URL                                |
| `DKG_SERVER_ADDR`  | TCP address for DKG protocol                        |
| `SIGN_SERVER_ADDR` | TCP address for signing protocol                    |
| `DEFAULT_SESSION_ID` | Default session identifier                        |

---

## 📖 Library API Highlights

- `keygen.rs`
    - `generate_private_share()` — Executes DKG, returns key share.
    - `airdrop_funds()` — Helper for devnet SOL.
- `sign.rs`
    - `run_signing_phase()` — Performs threshold signing.
    - `create_transfer_message()` — Builds Solana transfer transactions.
- `transport.rs`
    - `TcpIncoming<T>`/`TcpOutgoing<T>` — Async, length-delimited TCP framing with `tokio_util::codec`.
- `env_loader.rs`
    - Loads and merges `.env` configurations from multiple paths.

---

## ⚒️ Extensibility & Customization

- **Threshold adjustment:**  
  Parameterize threshold and participant count in `keygen.rs` and `sign.rs` for N-of-M signatures.

- **Key persistence:**  
  Integrate SQLx (already included in dependencies) to persist and restore key shares securely.

- **Enhanced security:**  
  Add TLS to all TCP streams in `transport.rs` for encrypted, mutually authenticated communication.

- **Platform support:**  
  Planned WASM/IndexedDB for browser-based DKG, SGX enclaves for secure server-side key storage, and mobile device integration.

---

## 🩺 Troubleshooting

| Problem                    | Solution                                                                 |
|----------------------------|-------------------------------------------------------------------------|
| Connection refused         | Ensure the server is running before the client. Verify TCP addresses.   |
| Redis errors               | Confirm Redis is running and accessible. Check `REDIS_URL` values.      |
| Protocol failures          | Use matching `DEFAULT_SESSION_ID` and unique `NODE_ID` values.          |
| Env file not loaded        | Ensure `.env` files exist and are readable in both `server/` and `client/` folders. |

---

## 🗺️ Roadmap

- **WASM/IndexedDB client:** In-browser DKG and secure key storage.
- **SGX enclave support:** Hardware-backed key protection on server.
- **Mobile integration:** Biometric authentication and local key vault.
- **TLS/Mutual Auth:** Full-stack encrypted transport.
- **Key recovery:** Decentralized, multi-party recovery protocols.

---

## 🤝 Contributing

We welcome contributions! Please open [issues](https://github.com/akash-R-A-J/idmap-core/issues) or submit [pull requests](https://github.com/akash-R-A-J/idmap-core/pulls).

---

## 📄 License

This project is open-source and part of an initiative for secure, passwordless Web3 authentication.

---

**Built with ❤️ for secure, decentralized Solana transactions.**
