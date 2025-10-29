use anyhow::Result;
use futures::StreamExt;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use dkg_tcp::{keygen, sign};
use solana_sdk::signature::Signature;

use redis::aio::{MultiplexedConnection, PubSub};
use redis::{AsyncCommands, Client};
use tokio::sync::RwLock;
use tokio::{net::TcpStream, task};

use givre::generic_ec::curves::Ed25519;
use givre::key_share::DirtyKeyShare;
use givre::keygen::key_share::Valid;

type ShareStore = Arc<RwLock<HashMap<(u64, String), Valid<DirtyKeyShare<Ed25519>>>>>;

/// Central configuration structure for environment-based values.
#[derive(Debug, Clone)]
struct EnvConfig {
    n: u16,
    node_id: u64,
    redis_url: String,
    dkg_server_addr: String,
    sign_server_addr: String,
    default_session_id: String,
}

impl EnvConfig {
    /// Load all environment variables with fallbacks.
    fn load() -> Result<Self> {
        Ok(Self {
            n: env::var("N")
                .unwrap_or_else(|_| "1".into())
                .parse::<u16>()
                .expect("N must be a number"),

            node_id: env::var("NODE_ID")
                .unwrap_or_else(|_| "1".into())
                .parse::<u64>()
                .expect("NODE_ID must be a number"),

            redis_url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into()),

            dkg_server_addr: env::var("DKG_SERVER_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:7001".into()),

            sign_server_addr: env::var("SIGN_SERVER_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:7002".into()),

            default_session_id: env::var("DEFAULT_SESSION_ID")
                .unwrap_or_else(|_| "session-001".into()),
        })
    }
}

pub async fn run_client() -> Result<()> {
    // Load configuration from env
    let env_config = EnvConfig::load()?;

    // Initialize Redis clients
    let redis_client_dkg = Arc::new(Client::open(env_config.redis_url.clone())?);
    let redis_client_sign = Arc::new(Client::open(env_config.redis_url.clone())?);

    // Shared in-memory key store
    let share_store: ShareStore = Arc::new(RwLock::new(HashMap::new()));

    // Run both DKG and SIGN clients concurrently
    let dkg_client = {
        let redis = redis_client_dkg.clone();
        let store = share_store.clone();
        let dkg_addr = env_config.dkg_server_addr.clone();
        let id = env_config.node_id;
        let n = env_config.n;
        let session_id = env_config.default_session_id.clone();

        task::spawn(async move {
            if let Err(e) = run_dkg_client(redis, store, id, n, &dkg_addr, &session_id).await {
                error!("[CLIENT-DKG] Error: {:?}", e);
            }
        })
    };

    let sign_client = {
        let redis = redis_client_sign.clone();
        let store = share_store.clone();
        let sign_addr = env_config.sign_server_addr.clone();
        let id = env_config.node_id;

        task::spawn(async move {
            if let Err(e) = run_sign_client(redis, store, id, &sign_addr).await {
                error!("[CLIENT-SIGN] Error: {:?}", e);
            }
        })
    };

    info!("[CLIENT] DKG + SIGN clients running concurrently...");
    let _ = tokio::join!(dkg_client, sign_client);
    Ok(())
}

///  Handles DKG phase client logic.
async fn run_dkg_client(
    redis_client: Arc<Client>,
    share_store: ShareStore,
    id: u64,
    n: u16,
    dkg_server_addr: &str,
    default_session: &str,
) -> Result<()> {
    let mut pubsub: PubSub = redis_client.get_async_pubsub().await?;
    pubsub.subscribe("dkg-start").await?;
    info!("[CLIENT-DKG] Subscribed to `dkg-start`");

    let mut pub_conn: MultiplexedConnection =
        redis_client.get_multiplexed_async_connection().await?;
    let mut on_msg = pubsub.on_message();

    while let Some(msg) = on_msg.next().await {
        let payload: String = msg.get_payload()?;
        debug!("[CLIENT-DKG] Received: {}", payload);

        let parsed: serde_json::Value = serde_json::from_str(&payload)?;
        if parsed["action"] == "startdkg" {
            let session = parsed["session"].as_str().unwrap_or(default_session);
            info!("[CLIENT-DKG] Starting DKG session {}", session);

            let socket: TcpStream = TcpStream::connect(dkg_server_addr).await?;
            let shares = keygen::generate_private_share(socket, id, n, session.as_bytes()).await?;
            let pubkey = bs58::encode(shares.shared_public_key().to_bytes(true)).into_string();

            {
                let mut store = share_store.write().await;
                store.insert((id, session.to_string()), shares.clone());
            }
            info!("[DKG] Stored share in memory for session {}", session);

            let response = serde_json::json!({
                "id": parsed["id"], // node backend id
                "result_type": "dkg-result",
                "data": pubkey,
                "server_id": id,
            });
            pub_conn
                .publish::<_, _, ()>("dkg-result", response.to_string())
                .await?;
            info!("[CLIENT-DKG] DKG result published!");
        }
    }
    Ok(())
}

///  Handles SIGN phase client logic.
async fn run_sign_client(
    redis_client: Arc<Client>,
    share_store: ShareStore,
    id: u64,
    sign_server_addr: &str,
) -> Result<()> {
    let mut pubsub: PubSub = redis_client.get_async_pubsub().await?;
    pubsub.subscribe("sign-start").await?;
    info!("[CLIENT-SIGN] Subscribed to `sign-start`");

    let mut pub_conn: MultiplexedConnection =
        redis_client.get_multiplexed_async_connection().await?;
    let mut on_msg = pubsub.on_message();

    while let Some(msg) = on_msg.next().await {
        let payload: String = match msg.get_payload() {
            Ok(p) => p,
            Err(e) => {
                error!("[CLIENT-SIGN] Failed to parse payload: {:?}", e);
                continue;
            }
        };

        debug!("[CLIENT-SIGN] Received: {}", payload);

        let parsed: serde_json::Value = match serde_json::from_str(&payload) {
            Ok(p) => p,
            Err(e) => {
                warn!("[CLIENT-SIGN] Invalid JSON payload: {:?}", e);
                continue;
            }
        };

        if parsed["action"] != "sign" {
            debug!("[CLIENT-SIGN] Ignored unrelated message.");
            continue;
        }

        let s = parsed["session"].as_str().unwrap();
        info!("[CLIENT-SIGN] original {}", s);
        let session = parsed["session"].as_str().unwrap_or("session-001");
        info!("[CLIENT-SIGN] Signing for session {}", session);

        let maybe_share = {
            let store = share_store.read().await;
            store.get(&(id, session.to_string())).cloned()
        };

        let valid_share = match maybe_share {
            Some(s) => s,
            None => {
                warn!(
                    "[SIGN] No in-memory share found for node {} and session {}",
                    id, session
                );
                let error_ack = serde_json::json!({
                    "id": parsed["id"],
                    "result_type": "sign-error",
                    "error": format!("No share found for node {} session {}", id, session),
                    "server_id": id,
                });
                let _ = pub_conn
                    .publish::<_, _, ()>("sign-result", error_ack.to_string())
                    .await;
                continue;
            }
        };

        let message_base64 = parsed["message"].as_str().unwrap_or_default();

        let message_bytes = match base64::decode(message_base64) {
            Ok(m) => m,
            Err(e) => {
                error!("[CLIENT-SIGN] Failed to decode message: {:?}", e);
                continue;
            }
        };

        match TcpStream::connect(sign_server_addr).await {
            Ok(socket) => {
                match sign::run_signing_phase(id, valid_share, socket, message_bytes).await {
                    Ok((r, z)) => {
                        let sig = [r, z].concat();
                        let sol_sig: Signature = Signature::try_from(sig.clone())
                            .expect("Invalid Solana signature conversion");
                        let response = serde_json::json!({
                            "id": parsed["id"],
                            "result_type": "sign-result",
                            "data": sol_sig.to_string(),
                            "server_id": id,
                        });
                        pub_conn
                            .publish::<_, _, ()>("sign-result", response.to_string())
                            .await?;
                        info!("[CLIENT-SIGN] Signature published!");
                    }
                    Err(e) => {
                        error!("[CLIENT-SIGN] Signing phase failed: {:?}", e);
                        let fail_ack = serde_json::json!({
                            "id": parsed["id"],
                            "result_type": "sign-error",
                            "error": format!("Signing failed: {}", e),
                            "server_id": id,
                        });
                        let _ = pub_conn
                            .publish::<_, _, ()>("sign-result", fail_ack.to_string())
                            .await;
                    }
                }
            }
            Err(e) => {
                error!("[CLIENT-SIGN] TCP connection error: {:?}", e);
                continue;
            }
        };
    }

    Ok(())
}
