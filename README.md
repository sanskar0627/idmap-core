# idmap-core

A distributed key generation (DKG) and threshold signing system for Solana transactions using Ed25519 cryptography.

## What This Does

Implements a 2-of-2 threshold signature scheme where two parties can jointly:
1. Generate a shared Ed25519 keypair without either party knowing the full private key
2. Collaboratively sign Solana transactions using their key shares

Neither party can sign alone — both must participate to produce a valid signature.

---

## 🌐 Resources & Links

Here are all the key resources related to **IdMap** and its components:

- 🔗 **IdMap Gateway Repository:** [github.com/akash-R-A-J/idmap-core](https://github.com/akash-R-A-J/idmap-gateway)
- 📘 **IdMap Gateway – Detailed Documentation:** [deepwiki.com/akash-R-A-J/idmap-gateway](https://deepwiki.com/akash-R-A-J/idmap-gateway)
- 📗 **IdMap Core – Detailed Documentation:** [deepwiki.com/akash-R-A-J/idmap-core](https://deepwiki.com/akash-R-A-J/idmap-core)
- 🚀 **Live Website:** [id-map.shop](https://www.id-map.shop/)

---

## Project Structure

```
idmap-core/
├── src/              # Core library (dkg_tcp)
│   ├── keygen.rs     # DKG protocol implementation
│   ├── sign.rs       # Threshold signing logic
│   ├── transport.rs  # TCP message transport layer
│   └── env_loader.rs # Environment configuration
├── server/           # Server binary
│   └── src/
│       └── server.rs # Accepts connections, coordinates protocols
├── client/           # Client binary
│   └── src/
│       └── client.rs # Connects to server, participates in protocols
```

**Workspace Layout:**
- **`src/`** — Reusable library containing core DKG/signing primitives and TCP transport
- **`server/`** — TCP server that accepts connections and coordinates protocol execution
- **`client/`** — Client that connects to the server to participate in key generation and signing

## Architecture

### How It Works

```
┌─────────────────────────────────────────────────────┐
│                    Redis Pub/Sub                    │
│         (triggers keygen/signing sessions)          │
└──────────────┬──────────────────────┬───────────────┘
               │                      │
               ▼                      ▼
       ┌──────────────┐      ┌──────────────┐
       │   Server     │◄────►│   Client     │
       │  (NODE_ID=0) │ TCP  │  (NODE_ID=1) │
       └──────────────┘      └──────────────┘
               │                      │
               └──────────┬───────────┘
                          ▼
                  MPC Protocol Execution
                  (keygen or signing)
```

**Flow:**
1. Both server and client listen to Redis pub/sub channels
2. External trigger publishes to `keygen:start:<session_id>` or `signing:start:<session_id>`
3. Server accepts TCP connection from client
4. Parties execute threshold protocol over TCP using the `round-based` MPC framework
5. Results are stored locally and published back to Redis

**Communication:**
- **Redis:** Coordination and triggering (pub/sub)
- **TCP:** Party-to-party MPC protocol messages
- **Solana RPC:** Fetching blockhash, submitting transactions

## Prerequisites

- Rust 1.70+ (edition 2024)
- Redis server running locally or remotely
- Access to Solana devnet RPC (for testing)

## Installation

### 1. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Clone and Build

```bash
git clone <repo-url>
cd idmap-core
cargo build --release
```

## Configuration

Each binary requires its own `.env` file for configuration.

### Server Configuration (`server/.env`)

```env
NODE_ID=0
N=2
REDIS_URL=redis://127.0.0.1:6379
DKG_SERVER_ADDR=0.0.0.0:7001
SIGN_SERVER_ADDR=0.0.0.0:7002
DEFAULT_SESSION_ID=session-001
```

### Client Configuration (`client/.env`)

```env
NODE_ID=1
N=2
REDIS_URL=redis://127.0.0.1:6379
DKG_SERVER_ADDR=127.0.0.1:7001
SIGN_SERVER_ADDR=127.0.0.1:7002
DEFAULT_SESSION_ID=session-001
```

**Variables:**
- `NODE_ID` — Unique party identifier (0 for server, 1 for client)
- `N` — Total number of participants (currently 2)
- `REDIS_URL` — Redis connection string
- `DKG_SERVER_ADDR` — TCP address for DKG phase
- `SIGN_SERVER_ADDR` — TCP address for signing phase
- `DEFAULT_SESSION_ID` — Session identifier for coordinating rounds

## Running Locally

### 1. Start Redis

```bash
redis-server
```

### 2. Run Server (Terminal 1)

```bash
cargo run -p server
```

### 3. Run Client (Terminal 2)

```bash
cargo run -p client
```

### 4. Trigger Protocols via Redis

```bash
# Trigger key generation
redis-cli PUBLISH "keygen:start:session-001" ""

# Trigger signing (after keygen completes)
redis-cli PUBLISH "signing:start:session-001" ""
```

## Development Commands

```bash
# Build all crates
cargo build

# Build specific binary
cargo build -p server
cargo build -p client

# Run tests
cargo test

# Check without building
cargo check

# Format code
cargo fmt

# Lint
cargo clippy
```

## How Protocols Work

### Key Generation (DKG)

1. External system publishes to `keygen:start:<session_id>`
2. Server listens on `DKG_SERVER_ADDR` for incoming connection
3. Client connects to server's DKG endpoint
4. Both parties run the CGGMP21 keygen protocol over TCP
5. Each party stores their key share locally in `ShareStore`
6. Public key can be derived from either share

### Threshold Signing

1. External system publishes to `signing:start:<session_id>` with message data
2. Server listens on `SIGN_SERVER_ADDR` for incoming connection
3. Client connects to server's signing endpoint
4. Both parties load their key shares for the session
5. Run threshold signing protocol on the message
6. Each party produces signature components (r, z)
7. Signature is valid and can be used in Solana transactions

## Core Library (`idmap-core`)

The `src/` directory contains reusable components:

**`keygen.rs`**
- `generate_private_share()` — Runs DKG protocol, returns key share
- `airdrop_funds()` — Helper for getting devnet SOL

**`sign.rs`**
- `run_signing_phase()` — Executes threshold signing, returns signature
- `create_transfer_message()` — Builds Solana transfer transactions
- `send_message_to_other_server()` — Coordinates message exchange

**`transport.rs`**
- `TcpIncoming<M>` — Deserializes incoming MPC messages from TCP
- `TcpOutgoing<M>` — Serializes outgoing MPC messages to TCP
- Length-delimited framing using `tokio_util::codec`

**`env_loader.rs`**
- Loads `.env` files from multiple locations (local, root)
- Thread-safe initialization

## Key Dependencies

- **givre** — Threshold cryptography (CGGMP21 protocol, Ed25519)
- **round-based** — MPC framework for multi-party computation
- **tokio** — Async runtime
- **redis** — Pub/sub coordination between parties
- **solana-sdk** — Blockchain transaction creation
- **serde/bincode** — Message serialization


## Extending the System

**To support different thresholds:**
- Modify `keygen.rs:42` to parameterize `.set_threshold(2)`
- Update `sign.rs:56` to dynamically set `parties_indexes_at_keygen`

**To add key persistence:**
- Integrate the SQLx database layer (already imported in dependencies)
- Serialize/deserialize `Valid<DirtyKeyShare<Ed25519>>` to database

**To secure TCP connections:**
- Add TLS wrapper around `TcpStream` in `transport.rs`
- Implement mutual authentication between parties

## Troubleshooting

**Connection refused:**
- Ensure server is running before client
- Check that addresses in `.env` match between server/client

**Redis errors:**
- Verify Redis is running: `redis-cli ping`
- Check `REDIS_URL` is correct in both `.env` files

**DKG/Signing failures:**
- Ensure both parties use the same `DEFAULT_SESSION_ID`
- Check that `NODE_ID` is unique (0 and 1)
- Verify network connectivity between server and client

**Environment not loading:**
- Confirm `.env` files exist in `server/` and `client/` directories
- Check file permissions

---

## Future Roadmap / Upcoming Features

- **Client-side WASM & IndexedDB**: Execute part of the DKG in-browser and store encrypted key shares client-side to enhance transparency and trust.  
- **Rust-SGX Enclave**: Securely persist server-side key shares within Intel SGX enclaves for stronger protection and decentralization.  
- **Mobile App Integration**: Enable biometric WebAuthn and local encrypted key storage for a seamless mobile identity experience.  
- **Security & TLS Layer**: Implement TLS, encryption, and request signing across all inter-service communications.  
- **Key Recovery Protocol**: Develop a decentralized recovery mechanism using threshold cryptography and multi-party consent.

---

### 📄 License

This project is part of an open-source initiative for passwordless Web3 authentication.

### 🤝 Contributing

Contributions are welcome! Feel free to open issues or submit pull requests.

---

**Built with ❤️ for secure, passwordless Solana transactions**
